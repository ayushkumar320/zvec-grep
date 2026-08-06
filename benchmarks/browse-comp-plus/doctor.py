from __future__ import annotations

import json
import os
import platform
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Mapping, Sequence

SCHEMA_VERSION = 1
DEFAULT_MINIMUM_FREE_GIB = 30.0


@dataclass(frozen=True)
class Check:
    name: str
    ok: bool
    detail: str
    required: bool = True

    @property
    def status(self) -> str:
        if self.ok:
            return "pass"
        return "fail" if self.required else "warn"

    def to_dict(self) -> dict[str, object]:
        return {**asdict(self), "status": self.status}


@dataclass(frozen=True)
class DoctorConfig:
    work_dir: Path
    zg_bin: str = "zg"
    codex_bin: str = "codex"
    embedding_key_env: str = "DASHSCOPE_API_KEY"
    minimum_free_gib: float = DEFAULT_MINIMUM_FREE_GIB


@dataclass(frozen=True)
class DoctorReport:
    generated_at: str
    ready: bool
    work_dir: str
    configuration: dict[str, object]
    system: dict[str, str]
    checks: tuple[Check, ...]

    def to_dict(self) -> dict[str, object]:
        return {
            "schema_version": SCHEMA_VERSION,
            "stage": "doctor",
            "generated_at": self.generated_at,
            "ready": self.ready,
            "work_dir": self.work_dir,
            "configuration": self.configuration,
            "system": self.system,
            "checks": [check.to_dict() for check in self.checks],
        }


def _resolve_executable(command: str) -> str | None:
    candidate = Path(command).expanduser()
    if candidate.is_absolute() or candidate.parent != Path("."):
        return str(candidate.resolve()) if candidate.is_file() else None
    return shutil.which(command)


def _run(command: Sequence[str]) -> tuple[bool, str]:
    try:
        completed = subprocess.run(
            list(command),
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return False, str(error)

    output = (completed.stdout or completed.stderr).strip()
    if completed.returncode != 0:
        return False, output or f"exited with status {completed.returncode}"
    return True, output


def _command_check(name: str, command: str, version_args: Sequence[str]) -> Check:
    executable = _resolve_executable(command)
    if executable is None:
        return Check(name, False, f"{command!r} was not found")

    ok, output = _run([executable, *version_args])
    first_line = output.splitlines()[0] if output else "version unavailable"
    return Check(name, ok, f"{first_line} ({executable})")


def _codex_auth_check(codex_bin: str) -> Check:
    executable = _resolve_executable(codex_bin)
    if executable is None:
        return Check("Codex authentication", False, "Codex was not found")

    ok, _ = _run([executable, "login", "status"])
    return Check(
        "Codex authentication",
        ok,
        "authenticated" if ok else "run 'codex login' before the benchmark",
    )


def _credential_check(name: str, environment: Mapping[str, str]) -> Check:
    value = environment.get(name, "").strip()
    return Check(
        "Embedding credential",
        bool(value),
        f"{name} is set" if value else f"{name} is not set",
    )


def _disk_check(work_dir: Path, minimum_free_gib: float) -> Check:
    free = shutil.disk_usage(work_dir).free
    free_gib = free / (1024**3)
    return Check(
        "Free disk",
        free_gib >= minimum_free_gib,
        f"{free_gib:.1f} GiB available; {minimum_free_gib:.1f} GiB required",
    )


def collect_checks(
    config: DoctorConfig,
    *,
    environment: Mapping[str, str] | None = None,
) -> list[Check]:
    environment = os.environ if environment is None else environment
    version = sys.version_info
    python_ok = (3, 12) <= version[:2] < (3, 14)
    system = platform.system() or "unknown"
    machine = platform.machine() or "unknown"

    return [
        Check(
            "Python",
            python_ok,
            f"{version.major}.{version.minor}.{version.micro}; requires >=3.12,<3.14",
        ),
        Check(
            "Platform",
            system in {"Darwin", "Linux"},
            f"{system} {machine}; supported platforms are macOS and Linux",
        ),
        _command_check("Git", "git", ["--version"]),
        _command_check("ripgrep", "rg", ["--version"]),
        _command_check("zvec-grep", config.zg_bin, ["version"]),
        _command_check("Codex", config.codex_bin, ["--version"]),
        _codex_auth_check(config.codex_bin),
        _credential_check(config.embedding_key_env, environment),
        _disk_check(config.work_dir, config.minimum_free_gib),
    ]


def build_report(config: DoctorConfig, checks: Sequence[Check]) -> DoctorReport:
    return DoctorReport(
        generated_at=datetime.now(timezone.utc).isoformat(),
        ready=all(check.ok for check in checks if check.required),
        work_dir=str(config.work_dir.resolve()),
        configuration={
            "zg_bin": config.zg_bin,
            "codex_bin": config.codex_bin,
            "embedding_key_env": config.embedding_key_env,
            "minimum_free_gib": config.minimum_free_gib,
        },
        system={
            "python": platform.python_version(),
            "platform": platform.platform(),
        },
        checks=tuple(checks),
    )


def write_report(report: DoctorReport, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(f"{output.suffix}.tmp")
    temporary.write_text(
        json.dumps(report.to_dict(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(output)


def run_doctor(config: DoctorConfig) -> tuple[DoctorReport, Path]:
    config.work_dir.mkdir(parents=True, exist_ok=True)
    report = build_report(config, collect_checks(config))
    output = config.work_dir / "state" / "doctor.json"
    write_report(report, output)
    return report, output


def format_report(report: DoctorReport, output: Path) -> str:
    lines = ["BrowseComp-Plus doctor", ""]
    for check in report.checks:
        marker = "PASS" if check.ok else ("FAIL" if check.required else "WARN")
        lines.append(f"[{marker}] {check.name}: {check.detail}")
    lines.extend(
        [
            "",
            f"Artifact: {output}",
            f"Result: {'ready' if report.ready else 'not ready'}",
        ]
    )
    return "\n".join(lines)
