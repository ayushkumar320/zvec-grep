from __future__ import annotations

import argparse
import shlex
from pathlib import Path

from .doctor import run_doctor
from .runner import (
    DEFAULT_RUNS_DIR,
    SuiteConfigError,
    available_suites,
    build_harbor_command,
    execute,
    load_suite,
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="zg-bench",
        description="Run the zvec-grep benchmark suites through Harbor.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser(
        "doctor", help="check that the local benchmark dependencies are ready"
    )

    run = subparsers.add_parser("run", help="run a baseline smoke benchmark")
    run.add_argument("suite", choices=available_suites())
    run.add_argument("--agent", required=True, help="Harbor agent name")
    run.add_argument("--model", required=True, help="model identifier for the agent")
    run.add_argument(
        "--jobs-dir",
        type=Path,
        default=DEFAULT_RUNS_DIR,
        help="directory for Harbor job output (default: benchmarks/runs)",
    )
    run.add_argument("--job-name", help="override the generated Harbor job name")
    run.add_argument(
        "--dry-run",
        action="store_true",
        help="print the Harbor command without running it",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.command == "doctor":
        return run_doctor()

    try:
        suite = load_suite(args.suite)
        command = build_harbor_command(
            suite,
            agent=args.agent,
            model=args.model,
            jobs_dir=args.jobs_dir,
            job_name=args.job_name,
        )
    except (SuiteConfigError, ValueError) as error:
        raise SystemExit(f"error: {error}") from error

    print(f"Suite:   {suite.name}")
    print("Tier:    smoke")
    print("Profile: baseline")
    print(f"Task:    {suite.task}")
    if args.dry_run:
        print(shlex.join(command))
        return 0
    try:
        return execute(command, jobs_dir=args.jobs_dir)
    except FileNotFoundError as error:
        raise SystemExit(
            "error: Harbor was not found; run 'zg-bench doctor' to check the setup"
        ) from error


if __name__ == "__main__":
    raise SystemExit(main())
