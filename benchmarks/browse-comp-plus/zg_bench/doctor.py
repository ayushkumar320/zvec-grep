from __future__ import annotations

import os
import platform
import re
import shutil
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from .artifacts import read_json, utc_now, write_json
from .config import BenchmarkConfig
from .process import resolve_executable, run_command


@dataclass(frozen=True)
class Check:
    name: str
    ok: bool
    detail: str
    required: bool = True

    @property
    def status(self) -> str:
        return "pass" if self.ok else "fail" if self.required else "warn"

    def to_dict(self) -> dict[str, Any]:
        return {**asdict(self), "status": self.status}


def _command(name: str, executable: str, args: list[str]) -> Check:
    resolved = resolve_executable(executable)
    if resolved is None:
        return Check(name, False, f"{executable!r} was not found")
    result = run_command([resolved, *args], timeout=30)
    output = (result.stdout or result.stderr).strip().splitlines()
    detail = output[0] if output else f"exit {result.returncode}"
    return Check(name, result.ok, f"{detail} ({resolved})")


def _disk(artifacts: Path, minimum_gib: float) -> Check:
    artifacts.mkdir(parents=True, exist_ok=True)
    free = shutil.disk_usage(artifacts).free / (1024**3)
    return Check(
        "Free disk",
        free >= minimum_gib,
        f"{free:.1f} GiB available; {minimum_gib:.1f} GiB required",
    )


def _authentication(codex_bin: str) -> Check:
    executable = resolve_executable(codex_bin)
    if executable is None:
        return Check("Codex authentication", False, "Codex was not found")
    result = run_command([executable, "login", "status"], timeout=30)
    return Check(
        "Codex authentication",
        result.ok,
        "authenticated" if result.ok else "run 'codex login' before the benchmark",
    )


def _zvec_version(config: BenchmarkConfig, zg_bin: str) -> Check:
    executable = resolve_executable(zg_bin)
    if executable is None:
        return Check("zvec-grep", False, f"{zg_bin!r} was not found")
    result = run_command([executable, "version"], timeout=30)
    value = result.stdout.strip()
    match = re.match(r"^(\d+)\.(\d+)\.(\d+)", value)
    actual = tuple(int(part) for part in match.groups()) if match else ()
    expected = tuple(int(part) for part in config.zvec_grep.version.split("."))
    ok = result.ok and bool(actual) and actual == expected
    return Check(
        "zvec-grep",
        ok,
        f"{value or 'unknown'}; pinned version is {config.zvec_grep.version} ({executable})",
    )


def _prepared_checks(
    config: BenchmarkConfig, artifacts: Path, zg_bin: str, codex_bin: str
) -> list[Check]:
    checks: list[Check] = []
    for stage in ("dataset", "corpus", "index", "profiles"):
        path = artifacts / "state" / f"{stage}.json"
        checks.append(
            Check(
                f"Prepared {stage}",
                path.is_file(),
                str(path) if path.is_file() else "not prepared",
                required=False,
            )
        )
    dataset_path = artifacts / "state" / "dataset.json"
    if dataset_path.is_file():
        dataset = read_json(dataset_path)
        query_state = dataset.get("queries", {})
        corpus_state = dataset.get("corpus", {})
        declared_files = [
            *query_state.get("parquet_files", []),
            *corpus_state.get("parquet_files", []),
        ]
        files_ok = all(
            (artifacts / "source" / item["path"]).is_file()
            and (artifacts / "source" / item["path"]).stat().st_size == item["bytes"]
            for item in declared_files
        )
        dataset_ok = (
            query_state.get("revision") == config.dataset.queries_revision
            and corpus_state.get("revision") == config.dataset.corpus_revision
            and query_state.get("count") == config.dataset.expected_queries
            and bool(declared_files)
            and files_ok
        )
        checks.append(
            Check(
                "Dataset integrity",
                dataset_ok,
                f"{query_state.get('count', 0)} queries; "
                f"{len(declared_files)} pinned Parquet files present"
                if dataset_ok
                else "dataset revision, count, or file sizes do not match",
            )
        )
    corpus_path = artifacts / "state" / "corpus.json"
    if corpus_path.is_file():
        corpus = read_json(corpus_path)
        root = Path(corpus.get("root", ""))
        documents = Path(corpus.get("documents", ""))
        manifest = Path(corpus.get("manifest", ""))
        corpus_ok = (
            corpus.get("source_revision") == config.dataset.corpus_revision
            and corpus.get("count") == config.dataset.expected_corpus_documents
            and documents.is_dir()
            and manifest.is_file()
        )
        checks.append(
            Check(
                "Corpus integrity",
                corpus_ok,
                f"{corpus.get('count', 0)} materialized documents"
                if corpus_ok
                else "corpus revision, count, or paths do not match",
            )
        )
        stable_root = (
            root.is_dir()
            and not root.is_symlink()
            and documents.is_dir()
            and not documents.is_symlink()
            and documents.parent.resolve() == root.resolve()
        )
        checks.append(
            Check(
                "Stable corpus root",
                stable_root,
                str(root.resolve())
                if stable_root
                else "corpus root and documents must be physical, stable directories",
            )
        )
    profiles_path = artifacts / "state" / "profiles.json"
    if profiles_path.is_file():
        profiles = read_json(profiles_path)
        baseline = Path(profiles["baseline_home"])
        treatment = Path(profiles["treatment_home"])
        baseline_text = (baseline / "config.toml").read_text(encoding="utf-8")
        baseline_agents = (
            (baseline / "AGENTS.md").read_text(encoding="utf-8")
            if (baseline / "AGENTS.md").is_file()
            else ""
        )
        treatment_text = (treatment / "config.toml").read_text(encoding="utf-8")
        treatment_agents = (
            (treatment / "AGENTS.md").read_text(encoding="utf-8")
            if (treatment / "AGENTS.md").is_file()
            else ""
        )
        checks.extend(
            (
                Check(
                    "Baseline isolation",
                    "ZVEC_GREP_START" not in baseline_text
                    and "ZVEC_GREP_START" not in baseline_agents,
                    "zvec-grep MCP and guidance absent"
                    if "ZVEC_GREP_START" not in baseline_text
                    and "ZVEC_GREP_START" not in baseline_agents
                    else "zvec-grep integration leaked into baseline",
                ),
                Check(
                    "Treatment integration",
                    "ZVEC_GREP_START" in treatment_text
                    and "ZVEC_GREP_START" in treatment_agents,
                    "zvec-grep MCP and guidance configured"
                    if "ZVEC_GREP_START" in treatment_text
                    and "ZVEC_GREP_START" in treatment_agents
                    else "zvec-grep MCP or guidance missing",
                ),
            )
        )
        codex = resolve_executable(codex_bin)
        if codex:
            environment = dict(os.environ)
            environment["CODEX_HOME"] = str(baseline)
            environment["HOME"] = str(baseline)
            authentication = run_command(
                [codex, "login", "status"], env=environment, timeout=30
            )
            checks.append(
                Check(
                    "Isolated Codex authentication",
                    authentication.ok,
                    "available in isolated profiles"
                    if authentication.ok
                    else "isolated CODEX_HOME cannot access Codex authentication",
                )
            )
            baseline_mcp = run_command(
                [codex, "mcp", "list"], env=environment, timeout=30
            )
            treatment_environment = dict(environment)
            treatment_environment["CODEX_HOME"] = str(treatment)
            treatment_environment["HOME"] = str(treatment)
            treatment_mcp = run_command(
                [codex, "mcp", "list"], env=treatment_environment, timeout=30
            )
            checks.extend(
                (
                    Check(
                        "Baseline MCP surface",
                        baseline_mcp.ok and "zvec_grep" not in baseline_mcp.stdout,
                        "zvec-grep absent from Codex MCP list"
                        if "zvec_grep" not in baseline_mcp.stdout
                        else "zvec-grep visible in baseline MCP list",
                    ),
                    Check(
                        "Treatment MCP surface",
                        treatment_mcp.ok and "zvec_grep" in treatment_mcp.stdout,
                        "zvec-grep enabled in Codex MCP list"
                        if "zvec_grep" in treatment_mcp.stdout
                        else "zvec-grep absent from treatment MCP list",
                    ),
                )
            )
    index_path = artifacts / "state" / "index.json"
    corpus_path = artifacts / "state" / "corpus.json"
    if index_path.is_file() and corpus_path.is_file():
        executable = resolve_executable(zg_bin)
        if executable:
            root = Path(read_json(corpus_path)["root"])
            result = run_command(
                [executable, "status", root, "--mode", "direct", "--check-ready"],
                timeout=120,
            )
            checks.append(
                Check(
                    "Index readiness",
                    result.ok,
                    (result.stdout or result.stderr).strip() or "no status output",
                )
            )
    return checks


def run_doctor(
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    codex_bin: str = "codex",
    zg_bin: str = "zg",
    minimum_free_gib: float = 30.0,
) -> tuple[dict[str, Any], Path]:
    version = sys.version_info
    system = platform.system()
    checks = [
        Check(
            "Python",
            (3, 12) <= version[:2] < (3, 14),
            f"{version.major}.{version.minor}.{version.micro}; requires >=3.12,<3.14",
        ),
        Check(
            "Platform",
            system in {"Darwin", "Linux"},
            f"{system} {platform.machine()}; native macOS and Linux are supported",
        ),
        _command("Git", "git", ["--version"]),
        _command("ripgrep", "rg", ["--version"]),
        _zvec_version(config, zg_bin),
        _command("Codex", codex_bin, ["--version"]),
        _authentication(codex_bin),
        _disk(artifacts, minimum_free_gib),
    ]
    if not config.zvec_grep.embedding.startswith("local/"):
        credential = any(
            os.environ.get(name)
            for name in ("ZVEC_GREP_API_KEY", "DASHSCOPE_API_KEY", "QWEN_API_KEY")
        )
        checks.append(
            Check(
                "Embedding credential",
                credential,
                "configured"
                if credential
                else "set ZVEC_GREP_API_KEY or provider credential",
            )
        )
    checks.extend(_prepared_checks(config, artifacts, zg_bin, codex_bin))
    report = {
        "stage": "doctor",
        "generated_at": utc_now(),
        "ready": all(check.ok for check in checks if check.required),
        "artifacts": str(artifacts.resolve()),
        "configuration": str(config.path),
        "checks": [check.to_dict() for check in checks],
    }
    output = artifacts / "state" / "doctor.json"
    write_json(output, report)
    return report, output


def format_report(report: dict[str, Any], output: Path) -> str:
    lines = ["BrowseComp-Plus doctor", ""]
    for check in report["checks"]:
        lines.append(f"[{check['status'].upper()}] {check['name']}: {check['detail']}")
    lines.extend(
        (
            "",
            f"Artifact: {output}",
            f"Result: {'ready' if report['ready'] else 'not ready'}",
        )
    )
    return "\n".join(lines)
