from __future__ import annotations

import os
import time
from pathlib import Path

from .artifacts import atomic_write_text, utc_now, write_json
from .config import BenchmarkConfig
from .process import inherited_environment, resolve_executable, run_command


CONFIG_START = "# ZVEC_GREP_START"
CONFIG_END = "# ZVEC_GREP_END"
AGENTS_START = "<!-- ZVEC_GREP_START -->"
AGENTS_END = "<!-- ZVEC_GREP_END -->"


def _link_if_present(source: Path, target: Path) -> None:
    if not source.exists():
        return
    if target.is_symlink() and target.resolve() == source.resolve():
        return
    if target.exists() or target.is_symlink():
        raise RuntimeError(f"refusing to replace profile path: {target}")
    target.symlink_to(source, target_is_directory=source.is_dir())


def _write_clean_config(path: Path) -> None:
    atomic_write_text(
        path,
        "\n".join(
            (
                'web_search = "disabled"',
                'sandbox_mode = "read-only"',
                "allow_login_shell = false",
                "analytics.enabled = false",
                "feedback.enabled = false",
                "agents.enabled = false",
                'history.persistence = "none"',
                "",
            )
        ),
    )


def prepare_profiles(
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    codex_bin: str = "codex",
    zg_bin: str = "zg",
    source_codex_home: Path | None = None,
) -> Path:
    codex = resolve_executable(codex_bin)
    zg = resolve_executable(zg_bin)
    if codex is None:
        raise RuntimeError(f"Codex executable not found: {codex_bin}")
    if zg is None:
        raise RuntimeError(f"zvec-grep executable not found: {zg_bin}")

    source_home = (
        (
            source_codex_home
            or Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
        )
        .expanduser()
        .resolve()
    )
    root = artifacts / "profiles"
    baseline = root / "baseline" / "codex-home"
    treatment = root / "zvec-grep" / "codex-home"
    for home in (baseline, treatment):
        home.mkdir(parents=True, exist_ok=True)
        _write_clean_config(home / "config.toml")
        (home / "AGENTS.md").unlink(missing_ok=True)
        _link_if_present(source_home / "auth.json", home / "auth.json")

    environment = inherited_environment()
    environment.update(
        {
            "CODEX_HOME": str(treatment),
            "HOME": str(treatment),
            "ZVEC_GREP_HOME": str((artifacts / "runtime" / "zvec-home").resolve()),
            "NO_COLOR": "1",
        }
    )
    install = run_command(
        [
            zg,
            "install",
            "--target",
            "codex",
            "--mcp-tool-timeout",
            str(config.zvec_grep.mcp_tool_timeout_seconds),
            "--yes",
        ],
        cwd=artifacts / "corpus",
        env=environment,
        timeout=180,
    )
    if not install.ok:
        raise RuntimeError(install.stderr.strip() or install.stdout.strip())

    baseline_config = (baseline / "config.toml").read_text(encoding="utf-8")
    baseline_agents = (
        (baseline / "AGENTS.md").read_text(encoding="utf-8")
        if (baseline / "AGENTS.md").is_file()
        else ""
    )
    treatment_config = (treatment / "config.toml").read_text(encoding="utf-8")
    treatment_agents = (
        (treatment / "AGENTS.md").read_text(encoding="utf-8")
        if (treatment / "AGENTS.md").is_file()
        else ""
    )
    if CONFIG_START in baseline_config or AGENTS_START in baseline_agents:
        raise RuntimeError("baseline profile contains zvec-grep integration")
    if CONFIG_START not in treatment_config or AGENTS_START not in treatment_agents:
        raise RuntimeError("treatment profile is missing zvec-grep integration")

    manifest = {
        "stage": "profiles",
        "generated_at": utc_now(),
        "codex_bin": str(codex),
        "zg_bin": str(zg),
        "source_codex_home": str(source_home),
        "baseline_home": str(baseline.resolve()),
        "treatment_home": str(treatment.resolve()),
        "zvec_grep_home": environment["ZVEC_GREP_HOME"],
        "baseline": {"zvec_mcp": False, "zvec_guidance": False},
        "zvec-grep": {
            "zvec_mcp": True,
            "zvec_guidance": True,
            "install_command": "zg install --target codex --yes",
        },
        "install_stdout": install.stdout,
        "install_stderr": install.stderr,
    }
    output = artifacts / "state" / "profiles.json"
    write_json(output, manifest)
    return output


def ensure_server(artifacts: Path, *, zg_bin: str = "zg") -> None:
    executable = resolve_executable(zg_bin)
    if executable is None:
        raise RuntimeError(f"zvec-grep executable not found: {zg_bin}")
    environment = inherited_environment()
    environment["ZVEC_GREP_HOME"] = str((artifacts / "runtime" / "zvec-home").resolve())
    check = run_command(
        [executable, "server", "status", "--check-ready"],
        env=environment,
        timeout=30,
    )
    if check.ok:
        return
    start = run_command(
        [executable, "server", "on", "--mcp-toolset", "agent"],
        env=environment,
        timeout=60,
    )
    if not start.ok:
        raise RuntimeError(start.stderr.strip() or start.stdout.strip())
    check = run_command(
        [executable, "server", "status", "--check-ready"],
        env=environment,
        timeout=30,
    )
    if not check.ok:
        raise RuntimeError(check.stderr.strip() or check.stdout.strip())


def prepare_search_runtime(
    config: BenchmarkConfig, artifacts: Path, *, zg_bin: str = "zg"
) -> dict[str, object]:
    """Warm and verify the stable corpus root outside measured agent time."""
    executable = resolve_executable(zg_bin)
    if executable is None:
        raise RuntimeError(f"zvec-grep executable not found: {zg_bin}")
    started_at = utc_now()
    started = time.monotonic()
    print("Preparing the zvec-grep runtime and stable corpus root...", flush=True)
    ensure_server(artifacts, zg_bin=str(executable))
    root = (artifacts / "corpus").resolve()
    search_root = root / "documents"
    environment = inherited_environment()
    environment["ZVEC_GREP_HOME"] = str((artifacts / "runtime" / "zvec-home").resolve())
    status = run_command(
        [executable, "status", root, "--mode", "server", "--check-ready"],
        cwd=root,
        env=environment,
        timeout=120,
    )
    if not status.ok:
        raise RuntimeError(status.stderr.strip() or status.stdout.strip())
    warmup = run_command(
        [
            executable,
            "query",
            "benchmark runtime readiness",
            "--mode",
            "server",
            "--refresh",
            "wait",
            "--limit",
            "1",
            "--preview",
            "none",
        ],
        cwd=search_root,
        env=environment,
        timeout=max(900, config.zvec_grep.mcp_tool_timeout_seconds),
    )
    if not warmup.ok:
        raise RuntimeError(warmup.stderr.strip() or warmup.stdout.strip())
    wall_seconds = time.monotonic() - started
    result: dict[str, object] = {
        "started_at": started_at,
        "finished_at": utc_now(),
        "wall_seconds": wall_seconds,
        "root": str(root),
        "search_root": str(search_root),
        "status_stdout": status.stdout,
        "status_stderr": status.stderr,
        "warmup_stdout": warmup.stdout,
        "warmup_stderr": warmup.stderr,
    }
    write_json(artifacts / "state" / "runtime.json", result)
    print(
        f"zvec-grep runtime ready in {wall_seconds:.1f} seconds",
        flush=True,
    )
    return result
