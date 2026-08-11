from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from .config import DEFAULT_ARTIFACTS_DIR, DEFAULT_CONFIG_PATH, load_config
from .corpus import materialize
from .dataset import fetch
from .doctor import format_report, run_doctor
from .evaluate import export_manual, export_official
from .index import build_index
from .profiles import prepare_profiles
from .report import generate_report
from .runner import resume_benchmark, run_benchmark


def _common(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--config", type=Path, default=DEFAULT_CONFIG_PATH, help="benchmark TOML"
    )
    parser.add_argument(
        "--artifacts",
        type=Path,
        default=DEFAULT_ARTIFACTS_DIR,
        help="generated data, indexes, runs, and reports",
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="zg-bench",
        description="Native Codex A/B benchmark on BrowseComp-Plus.",
    )
    _common(parser)
    subparsers = parser.add_subparsers(dest="command", required=True)

    doctor = subparsers.add_parser("doctor", help="validate the local environment")
    doctor.add_argument("--codex-bin", default="codex")
    doctor.add_argument("--zg-bin", default="zg")
    doctor.add_argument("--minimum-free-gib", type=float, default=30.0)
    doctor.add_argument("--json", action="store_true")

    fetch_parser = subparsers.add_parser("fetch", help="download pinned official data")
    fetch_parser.add_argument("--hf-token")

    subparsers.add_parser(
        "materialize", help="write official corpus text fields as Markdown files"
    )

    index_parser = subparsers.add_parser("index", help="manage the reusable index")
    index_subparsers = index_parser.add_subparsers(dest="index_command", required=True)
    index_build = index_subparsers.add_parser("build", help="build or resume the index")
    index_build.add_argument("--zg-bin", default="zg")
    index_build.add_argument("--rebuild", action="store_true")

    profiles = subparsers.add_parser("profiles", help="manage isolated Codex profiles")
    profiles_subparsers = profiles.add_subparsers(
        dest="profiles_command", required=True
    )
    profiles_prepare = profiles_subparsers.add_parser(
        "prepare", help="prepare baseline and zvec-grep profiles"
    )
    profiles_prepare.add_argument("--codex-bin", default="codex")
    profiles_prepare.add_argument("--zg-bin", default="zg")
    profiles_prepare.add_argument("--source-codex-home", type=Path)

    prepare = subparsers.add_parser(
        "prepare", help="fetch, materialize, index, and prepare profiles"
    )
    prepare.add_argument("--hf-token")
    prepare.add_argument("--codex-bin", default="codex")
    prepare.add_argument("--zg-bin", default="zg")
    prepare.add_argument("--source-codex-home", type=Path)
    prepare.add_argument("--yes", action="store_true", help="confirm first index build")

    for name, help_text in (
        ("run", "run a paired suite"),
        ("smoke", "run the fixed one-query smoke suite"),
    ):
        run = subparsers.add_parser(name, help=help_text)
        if name == "run":
            run.add_argument(
                "--suite",
                default="study-80",
                help="suite name or .txt path (default: study-80)",
            )
        run.add_argument("--model", required=True)
        run.add_argument("--reasoning")
        run.add_argument("--concurrency", type=int)
        run.add_argument("--run-id")
        run.add_argument("--codex-bin", default="codex")
        run.add_argument("--zg-bin", default="zg")

    resume = subparsers.add_parser("resume", help="resume an interrupted run")
    resume.add_argument("run_id")
    resume.add_argument("--codex-bin", default="codex")
    resume.add_argument("--zg-bin", default="zg")

    status = subparsers.add_parser("status", help="show paired run progress")
    status.add_argument("run_id", nargs="?")

    inspect = subparsers.add_parser("inspect", help="print one persisted trial")
    inspect.add_argument("run_id")
    inspect.add_argument("--case", required=True)
    inspect.add_argument("--profile", choices=("baseline", "zvec-grep"), required=True)
    inspect.add_argument("--events", action="store_true")

    evaluate = subparsers.add_parser(
        "evaluate", help="export official input or a manual review sheet"
    )
    evaluate.add_argument("run_id")
    evaluate.add_argument(
        "--evaluator", choices=("official", "manual"), default="official"
    )

    report = subparsers.add_parser("report", help="regenerate a run report")
    report.add_argument("run_id")
    return parser


def _latest_run(artifacts: Path) -> Path | None:
    runs = artifacts / "runs"
    candidates = (
        [path for path in runs.iterdir() if path.is_dir()] if runs.is_dir() else []
    )
    return (
        max(candidates, key=lambda path: path.stat().st_mtime) if candidates else None
    )


def _status(artifacts: Path, run_id: str | None) -> int:
    root = artifacts / "runs" / run_id if run_id else _latest_run(artifacts)
    if root is None or not root.is_dir():
        raise SystemExit("error: no benchmark runs found")
    metadata = json.loads((root / "run.json").read_text(encoding="utf-8"))
    profiles = {"baseline": {}, "zvec-grep": {}}
    persisted_pairs = 0
    completed_pairs = 0
    for query_id in metadata["query_ids"]:
        case = root / "cases" / str(query_id)
        pair_path = case / "pair.json"
        if pair_path.is_file():
            persisted_pairs += 1
            pair = json.loads(pair_path.read_text(encoding="utf-8"))
            if pair["eligible"] is True:
                completed_pairs += 1
        for profile in profiles:
            path = case / profile / "result.json"
            if path.is_file():
                status = json.loads(path.read_text(encoding="utf-8"))["status"]
                profiles[profile][status] = profiles[profile].get(status, 0) + 1
    print(f"Run: {metadata['run_id']}")
    print(f"Completed pairs: {completed_pairs} / {len(metadata['query_ids'])}")
    if persisted_pairs != completed_pairs:
        print(f"Persisted pair records: {persisted_pairs}")
    for profile, counts in profiles.items():
        rendered = (
            ", ".join(f"{name}={count}" for name, count in sorted(counts.items()))
            or "none"
        )
        print(f"{profile}: {rendered}")
    checkpoints = (
        sorted((root / "checkpoints").glob("*.md"))
        if (root / "checkpoints").is_dir()
        else []
    )
    if checkpoints:
        print(f"Latest checkpoint: {checkpoints[-1]}")
    return 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    config = load_config(args.config)
    artifacts = args.artifacts.expanduser().resolve()

    if args.command == "doctor":
        report, output = run_doctor(
            config,
            artifacts,
            codex_bin=args.codex_bin,
            zg_bin=args.zg_bin,
            minimum_free_gib=args.minimum_free_gib,
        )
        print(
            json.dumps(report, indent=2) if args.json else format_report(report, output)
        )
        return 0 if report["ready"] else 1
    if args.command == "fetch":
        print(
            fetch(config, artifacts, token=args.hf_token or os.environ.get("HF_TOKEN"))
        )
        return 0
    if args.command == "materialize":
        print(materialize(config, artifacts))
        return 0
    if args.command == "index":
        print(build_index(config, artifacts, zg_bin=args.zg_bin, rebuild=args.rebuild))
        return 0
    if args.command == "profiles":
        print(
            prepare_profiles(
                config,
                artifacts,
                codex_bin=args.codex_bin,
                zg_bin=args.zg_bin,
                source_codex_home=args.source_codex_home,
            )
        )
        return 0
    if args.command == "prepare":
        fetch(config, artifacts, token=args.hf_token or os.environ.get("HF_TOKEN"))
        materialize(config, artifacts)
        index_state = artifacts / "state" / "index.json"
        if not index_state.is_file() and not args.yes:
            if not sys.stdin.isatty():
                raise SystemExit("error: first index build requires --yes")
            answer = (
                input("Build the reusable zvec-grep index now? [y/N] ").strip().lower()
            )
            if answer not in {"y", "yes"}:
                raise SystemExit("index build not confirmed")
        build_index(config, artifacts, zg_bin=args.zg_bin)
        print(
            prepare_profiles(
                config,
                artifacts,
                codex_bin=args.codex_bin,
                zg_bin=args.zg_bin,
                source_codex_home=args.source_codex_home,
            )
        )
        return 0
    if args.command in {"run", "smoke"}:
        root = run_benchmark(
            config,
            artifacts,
            suite="smoke" if args.command == "smoke" else args.suite,
            model=args.model,
            reasoning_effort=args.reasoning,
            concurrency=args.concurrency,
            run_id=args.run_id,
            codex_bin=args.codex_bin,
            zg_bin=args.zg_bin,
        )
        print(root)
        return 0
    if args.command == "resume":
        print(
            resume_benchmark(
                config,
                artifacts,
                args.run_id,
                codex_bin=args.codex_bin,
                zg_bin=args.zg_bin,
            )
        )
        return 0
    if args.command == "status":
        return _status(artifacts, args.run_id)
    if args.command == "inspect":
        result = (
            artifacts
            / "runs"
            / args.run_id
            / "cases"
            / args.case
            / args.profile
            / "result.json"
        )
        if not result.is_file():
            raise SystemExit(f"error: trial not found: {result}")
        print(result.read_text(encoding="utf-8"), end="")
        if args.events:
            raw = json.loads(result.read_text(encoding="utf-8"))
            print(Path(raw["paths"]["events"]).read_text(encoding="utf-8"), end="")
        return 0
    if args.command == "evaluate":
        run_root = artifacts / "runs" / args.run_id
        print(
            export_official(artifacts, run_root)
            if args.evaluator == "official"
            else export_manual(artifacts, run_root)
        )
        return 0
    if args.command == "report":
        print(generate_report(artifacts / "runs" / args.run_id))
        return 0
    raise AssertionError(f"unhandled command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
