from __future__ import annotations

import platform
import shutil
import subprocess
import sys
from dataclasses import dataclass


@dataclass(frozen=True)
class Check:
    name: str
    ok: bool
    detail: str
    required: bool = True


def _run_version(command: list[str]) -> tuple[bool, str]:
    try:
        completed = subprocess.run(
            command,
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


def collect_checks() -> list[Check]:
    version = sys.version_info
    python_ok = (3, 12) <= version[:2] < (3, 14)
    checks = [
        Check(
            "Python",
            python_ok,
            f"{version.major}.{version.minor}.{version.micro}",
        )
    ]

    harbor = shutil.which("harbor")
    if harbor is None:
        checks.append(Check("Harbor", False, "not found on PATH"))
    else:
        ok, detail = _run_version([harbor, "--version"])
        checks.append(Check("Harbor", ok, detail or harbor))

    docker = shutil.which("docker")
    if docker is None:
        checks.append(Check("Docker", False, "not found on PATH"))
    else:
        ok, detail = _run_version(
            [docker, "version", "--format", "{{.Server.Version}}"]
        )
        checks.append(
            Check(
                "Docker",
                ok,
                f"server {detail}" if ok else f"daemon unavailable: {detail}",
            )
        )

    machine = platform.machine() or "unknown"
    system = platform.system() or "unknown"
    note = f"{system} {machine}"
    native_linux_x86 = system == "Linux" and machine in {"x86_64", "amd64"}
    if system == "Darwin" and machine in {"arm64", "aarch64"}:
        note += "; some benchmark images may run through emulation"
    checks.append(Check("Platform", native_linux_x86, note, required=False))
    return checks


def print_report(checks: list[Check]) -> int:
    for check in checks:
        marker = "OK" if check.ok else ("FAIL" if check.required else "WARN")
        print(f"[{marker}] {check.name}: {check.detail}")
    return 0 if all(check.ok for check in checks if check.required) else 1


def run_doctor() -> int:
    return print_report(collect_checks())
