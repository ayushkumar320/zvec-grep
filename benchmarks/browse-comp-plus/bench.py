from __future__ import annotations

import argparse
import json
from pathlib import Path

from doctor import DEFAULT_MINIMUM_FREE_GIB, DoctorConfig, format_report, run_doctor

BENCHMARK_ROOT = Path(__file__).resolve().parent
DEFAULT_WORK_DIR = BENCHMARK_ROOT / "work"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="zg-bench",
        description="Run the BrowseComp-Plus A/B benchmark for zvec-grep.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    doctor = subparsers.add_parser(
        "doctor",
        help="validate local tools, authentication, disk, and configuration",
    )
    doctor.add_argument(
        "--work-dir",
        type=Path,
        default=DEFAULT_WORK_DIR,
        help=f"generated artifact directory (default: {DEFAULT_WORK_DIR})",
    )
    doctor.add_argument(
        "--zg-bin",
        default="zg",
        help="zvec-grep executable name or path (default: zg)",
    )
    doctor.add_argument(
        "--codex-bin",
        default="codex",
        help="Codex executable name or path (default: codex)",
    )
    doctor.add_argument(
        "--embedding-key-env",
        default="DASHSCOPE_API_KEY",
        help="environment variable containing the embedding credential",
    )
    doctor.add_argument(
        "--minimum-free-gib",
        type=float,
        default=DEFAULT_MINIMUM_FREE_GIB,
        help=f"required free disk space (default: {DEFAULT_MINIMUM_FREE_GIB:g} GiB)",
    )
    doctor.add_argument(
        "--json",
        action="store_true",
        help="print the machine-readable report instead of the human summary",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.minimum_free_gib <= 0:
        raise SystemExit("error: --minimum-free-gib must be positive")

    report, output = run_doctor(
        DoctorConfig(
            work_dir=args.work_dir.expanduser(),
            zg_bin=args.zg_bin,
            codex_bin=args.codex_bin,
            embedding_key_env=args.embedding_key_env,
            minimum_free_gib=args.minimum_free_gib,
        )
    )
    if args.json:
        print(json.dumps(report.to_dict(), indent=2, sort_keys=True))
    else:
        print(format_report(report, output))
    return 0 if report.ready else 1


if __name__ == "__main__":
    raise SystemExit(main())
