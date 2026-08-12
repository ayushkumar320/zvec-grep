from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Any

from .artifacts import (
    atomic_write_text,
    fingerprint,
    read_json,
    sha256_file,
    utc_now,
    write_json,
)
from .config import BenchmarkConfig
from .corpus import workspace_root
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
                'history.persistence = "none"',
                "",
            )
        ),
    )


def _authentication_status(codex: Path, home: Path) -> str:
    environment = inherited_environment()
    environment.update(
        {
            "CODEX_HOME": str(home),
            "HOME": str(home),
            "NO_COLOR": "1",
        }
    )
    result = run_command(
        [codex, "login", "status"],
        env=environment,
        timeout=30,
    )
    if not result.ok:
        detail = result.stderr.strip() or result.stdout.strip()
        raise RuntimeError(
            f"Codex authentication is unavailable in profile {home}: "
            f"{detail or 'login status failed'}"
        )
    return next(
        (
            line.strip()
            for line in (*result.stdout.splitlines(), *result.stderr.splitlines())
            if line.lower().startswith("logged in")
        ),
        "authenticated",
    )


def prepare_profiles(
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    codex_bin: str = "codex",
    zg_bin: str = "zg",
    source_codex_home: Path | None = None,
    profiles_root: Path,
    manifest_path: Path,
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
    root = profiles_root
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
        cwd=workspace_root(artifacts, "zvec-grep"),
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

    authentication = {
        "baseline": _authentication_status(codex, baseline),
        "zvec-grep": _authentication_status(codex, treatment),
    }
    baseline_config_path = baseline / "config.toml"
    treatment_config_path = treatment / "config.toml"
    treatment_agents_path = treatment / "AGENTS.md"
    build = zvec_grep_build_identity(zg)
    files: dict[str, str | None] = {
        "baseline_config_sha256": sha256_file(baseline_config_path),
        "baseline_agents_sha256": None,
        "treatment_config_sha256": sha256_file(treatment_config_path),
        "treatment_agents_sha256": sha256_file(treatment_agents_path),
    }
    profile_fingerprint = fingerprint(
        [
            build["fingerprint"],
            *(files[key] or "<absent>" for key in sorted(files)),
        ]
    )
    manifest = {
        "stage": "profiles",
        "generated_at": utc_now(),
        "codex_bin": str(codex),
        "zg_bin": str(zg),
        "source_codex_home": str(source_home),
        "baseline_home": str(baseline.resolve()),
        "treatment_home": str(treatment.resolve()),
        "zvec_grep_home": environment["ZVEC_GREP_HOME"],
        "zvec_grep_build": build,
        "authentication": authentication,
        "files": files,
        "fingerprint": profile_fingerprint,
        "baseline": {"zvec_mcp": False, "zvec_guidance": False},
        "zvec-grep": {
            "zvec_mcp": True,
            "zvec_guidance": True,
            "install_command": "zg install --target codex --yes",
        },
        "install_stdout": install.stdout,
        "install_stderr": install.stderr,
    }
    write_json(manifest_path, manifest)
    return manifest_path


def zvec_grep_build_identity(executable: Path) -> dict[str, str]:
    resolved = executable.resolve()
    package_root = _package_root(resolved)
    build_root = package_root / "dist" if package_root else resolved.parent
    files = sorted(path for path in build_root.rglob("*") if path.is_file())
    if not files:
        files = [resolved]
    build_fingerprint = fingerprint(
        value
        for path in files
        for value in (
            str(path.relative_to(build_root)),
            sha256_file(path),
        )
    )
    return {
        "executable": str(resolved),
        "build_root": str(build_root.resolve()),
        "fingerprint": build_fingerprint,
    }


def validate_profiles(manifest_path: Path, *, zg_bin: str = "zg") -> dict[str, Any]:
    executable = resolve_executable(zg_bin)
    if executable is None:
        raise RuntimeError(f"zvec-grep executable not found: {zg_bin}")
    manifest = read_json(manifest_path)
    expected_build = manifest.get("zvec_grep_build", {})
    actual_build = zvec_grep_build_identity(executable)
    if expected_build.get("fingerprint") != actual_build["fingerprint"]:
        raise RuntimeError(
            "zvec-grep build changed after this run was created; "
            "resume with the original executable or start a new run"
        )
    baseline = Path(manifest["baseline_home"])
    treatment = Path(manifest["treatment_home"])
    baseline_agents = baseline / "AGENTS.md"
    actual_files: dict[str, str | None] = {
        "baseline_config_sha256": sha256_file(baseline / "config.toml"),
        "baseline_agents_sha256": (
            sha256_file(baseline_agents) if baseline_agents.is_file() else None
        ),
        "treatment_config_sha256": sha256_file(treatment / "config.toml"),
        "treatment_agents_sha256": sha256_file(treatment / "AGENTS.md"),
    }
    if manifest.get("files") != actual_files:
        raise RuntimeError(
            "benchmark profile files changed after this run was created; "
            "restore the run artifacts or start a new run"
        )
    expected_fingerprint = fingerprint(
        [
            actual_build["fingerprint"],
            *(actual_files[key] or "<absent>" for key in sorted(actual_files)),
        ]
    )
    if manifest.get("fingerprint") != expected_fingerprint:
        raise RuntimeError("benchmark profile fingerprint is invalid")
    return manifest


def _package_root(executable: Path) -> Path | None:
    for parent in executable.parents:
        if (parent / "package.json").is_file() and (parent / "dist").is_dir():
            return parent
    return None


def ensure_server(
    artifacts: Path, *, zg_bin: str = "zg", restart: bool = False
) -> None:
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
    if check.ok and not restart:
        return
    if check.ok:
        stop = run_command(
            [executable, "server", "off"],
            env=environment,
            timeout=60,
        )
        if not stop.ok:
            raise RuntimeError(stop.stderr.strip() or stop.stdout.strip())
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
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    zg_bin: str = "zg",
    restart_server: bool = False,
) -> dict[str, object]:
    """Verify the daemon and warm the existing index outside measured agent time."""
    executable = resolve_executable(zg_bin)
    if executable is None:
        raise RuntimeError(f"zvec-grep executable not found: {zg_bin}")
    started_at = utc_now()
    started = time.monotonic()
    print("Preparing the zvec-grep daemon and existing index...", flush=True)
    ensure_server(
        artifacts,
        zg_bin=str(executable),
        restart=restart_server,
    )
    root = workspace_root(artifacts, "zvec-grep")
    environment = inherited_environment()
    environment["ZVEC_GREP_HOME"] = str((artifacts / "runtime" / "zvec-home").resolve())
    warmup = run_command(
        [
            executable,
            "query",
            "benchmark runtime readiness",
            "--mode",
            "server",
            "--refresh",
            "off",
            "--limit",
            "1",
            "--preview",
            "none",
        ],
        cwd=root,
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
        "warmup_stdout": warmup.stdout,
        "warmup_stderr": warmup.stderr,
    }
    write_json(artifacts / "state" / "runtime.json", result)
    print(
        f"zvec-grep runtime ready in {wall_seconds:.1f} seconds",
        flush=True,
    )
    return result
