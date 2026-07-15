from __future__ import annotations

import subprocess
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Sequence

import yaml

BENCHMARKS_DIR = Path(__file__).resolve().parents[1]
SUITES_DIR = BENCHMARKS_DIR / "suites"
DEFAULT_RUNS_DIR = BENCHMARKS_DIR / "runs"


class SuiteConfigError(ValueError):
    """Raised when a benchmark suite definition is invalid."""


@dataclass(frozen=True)
class SmokeSuite:
    name: str
    dataset: str
    task: str


def available_suites() -> list[str]:
    return sorted(path.stem for path in SUITES_DIR.glob("*.yaml"))


def _require_mapping(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise SuiteConfigError(f"{label} must be a mapping")
    return value


def _require_nonempty_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise SuiteConfigError(f"{label} must be a non-empty string")
    return value


def load_suite(name_or_path: str | Path) -> SmokeSuite:
    candidate = Path(name_or_path)
    if candidate.suffix in {".yaml", ".yml"}:
        path = candidate
    else:
        if candidate.name != str(candidate):
            raise SuiteConfigError(f"invalid suite name: {name_or_path}")
        path = SUITES_DIR / f"{candidate.name}.yaml"

    if not path.is_file():
        raise SuiteConfigError(f"suite definition not found: {path}")

    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    except yaml.YAMLError as error:
        raise SuiteConfigError(f"invalid YAML in {path}: {error}") from error

    root = _require_mapping(raw, "suite definition")
    name = _require_nonempty_string(root.get("name"), "name")
    dataset = _require_nonempty_string(root.get("dataset"), "dataset")
    if "@" not in dataset or dataset.endswith("@latest"):
        raise SuiteConfigError("dataset must use a pinned Harbor revision")

    tiers = _require_mapping(root.get("tiers"), "tiers")
    smoke = _require_mapping(tiers.get("smoke"), "tiers.smoke")
    tasks = smoke.get("tasks")
    if not isinstance(tasks, list) or len(tasks) != 1:
        raise SuiteConfigError("the smoke tier must contain exactly one task")
    task = _require_nonempty_string(tasks[0], "tiers.smoke.tasks[0]")

    if path.parent == SUITES_DIR and name != path.stem:
        raise SuiteConfigError(f"suite name {name!r} must match filename {path.stem!r}")

    return SmokeSuite(name=name, dataset=dataset, task=task)


def default_job_name(suite: SmokeSuite) -> str:
    timestamp = datetime.now(UTC).strftime("%Y%m%d-%H%M%S")
    return f"{timestamp}-{suite.name}-smoke-baseline"


def build_harbor_command(
    suite: SmokeSuite,
    *,
    agent: str,
    model: str,
    jobs_dir: Path = DEFAULT_RUNS_DIR,
    job_name: str | None = None,
    harbor_executable: str = "harbor",
) -> list[str]:
    if not agent.strip():
        raise ValueError("agent must not be empty")
    if not model.strip():
        raise ValueError("model must not be empty")

    return [
        harbor_executable,
        "run",
        "--dataset",
        suite.dataset,
        "--include-task-name",
        suite.task,
        "--agent",
        agent,
        "--model",
        model,
        "--env",
        "docker",
        "--n-attempts",
        "1",
        "--n-concurrent",
        "1",
        "--jobs-dir",
        str(jobs_dir.resolve()),
        "--job-name",
        job_name or default_job_name(suite),
    ]


def execute(command: Sequence[str], *, jobs_dir: Path) -> int:
    jobs_dir.mkdir(parents=True, exist_ok=True)
    completed = subprocess.run(command, cwd=BENCHMARKS_DIR, check=False)
    return completed.returncode
