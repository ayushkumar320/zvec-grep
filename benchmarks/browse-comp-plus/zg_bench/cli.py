from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from .console import Console
from .config import DEFAULT_ARTIFACTS_DIR, load_config
from .corpus import materialize, prepared_corpus, workspace_root
from .dataset import fetch, prepared_dataset
from .doctor import format_report, run_doctor
from .evaluate import export_manual, export_official
from .index import build_index, prepared_index
from .report import generate_report
from .runner import resume_benchmark, run_benchmark


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="zg-bench",
        description="Native Codex A/B benchmark on BrowseComp-Plus.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    doctor = subparsers.add_parser("doctor", help="validate the local environment")
    doctor.add_argument("--codex-bin", default="codex")
    doctor.add_argument("--zg-bin", default="zg")
    doctor.add_argument("--json", action="store_true")

    prepare = subparsers.add_parser(
        "prepare", help="prepare benchmark data and the reusable index"
    )
    prepare.add_argument("--hf-token")
    prepare.add_argument("--zg-bin", default="zg")
    prepare.add_argument(
        "--yes", action="store_true", help="confirm index build or rebuild"
    )

    run = subparsers.add_parser("run", help="run a paired suite")
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
    config = load_config()

    if args.command == "doctor":
        report = run_doctor(
            config,
            codex_bin=args.codex_bin,
            zg_bin=args.zg_bin,
        )
        color = (
            sys.stdout.isatty()
            and "NO_COLOR" not in os.environ
            and os.environ.get("TERM") != "dumb"
        )
        print(
            json.dumps(report, indent=2)
            if args.json
            else format_report(report, color=color)
        )
        return 0 if report["ready"] else 1
    artifacts = DEFAULT_ARTIFACTS_DIR.resolve()
    if args.command == "prepare":
        console = Console()
        console.heading("BrowseComp-Plus prepare")
        console.detail("Artifacts", artifacts)

        console.step(1, 3, "Prepare official data")
        try:
            dataset_state = prepared_dataset(config, artifacts)
        except RuntimeError as error:
            console.error(str(error))
            return 1
        if dataset_state:
            console.success("Reused prepared official data")
            console.detail("State", dataset_state)
        else:
            console.activity("Downloading pinned Hugging Face snapshots")
            try:
                fetch(
                    config,
                    artifacts,
                    token=args.hf_token or os.environ.get("HF_TOKEN"),
                    status=console.activity,
                    progress=console.progress,
                )
            except RuntimeError as error:
                console.error(str(error))
                return 1
            console.success("Official data downloaded and verified")

        console.step(2, 3, "Prepare workspaces")
        try:
            corpus_state = prepared_corpus(config, artifacts)
        except RuntimeError as error:
            console.error(str(error))
            return 1
        if corpus_state:
            console.success(
                "Reused two workspaces with "
                f"{config.dataset.expected_corpus_documents:,} documents each"
            )
            console.detail("State", corpus_state)
        else:
            console.activity("Writing two independent corpus copies")
            try:
                materialize(
                    config,
                    artifacts,
                    progress=console.progress,
                )
            except RuntimeError as error:
                console.error(str(error))
                return 1
            finally:
                console.finish_progress()
            console.success(
                "Prepared two workspaces with "
                f"{config.dataset.expected_corpus_documents:,} documents each"
            )

        index_dir = workspace_root(artifacts, "zvec-grep") / ".zvec-grep"
        reuse_index = prepared_index(config, artifacts) is not None
        rebuild_index = index_dir.is_dir() and not reuse_index
        if not reuse_index and not args.yes:
            if not sys.stdin.isatty():
                action = "rebuild" if rebuild_index else "build"
                raise SystemExit(f"error: index {action} requires --yes")
            prompt = (
                "Existing index cannot be reused and will be rebuilt. "
                "Continue? [y/N] "
                if rebuild_index
                else "Build the reusable zvec-grep index now? [y/N] "
            )
            answer = console.prompt(
                prompt
            ).strip().lower()
            if answer not in {"y", "yes"}:
                raise SystemExit("index preparation not confirmed")

        console.step(3, 3, "Prepare zvec-grep index")
        console.detail("Embedding", config.zvec_grep.embedding)
        console.detail("Concurrency", str(config.zvec_grep.embedding_concurrency))
        console.detail("Logs", artifacts / "logs")
        activity = (
            "Checking existing index"
            if reuse_index
            else "Rebuilding existing index"
            if rebuild_index
            else "Building reusable index"
        )
        console.activity(activity)
        try:
            output = build_index(
                config,
                artifacts,
                zg_bin=args.zg_bin,
                rebuild=rebuild_index,
            )
        except RuntimeError as error:
            console.error(str(error))
            return 1
        result = (
            "Reused existing index"
            if reuse_index
            else "Index rebuilt"
            if rebuild_index
            else "Index is ready"
        )
        console.success(result)
        console.detail("State", output)
        console.blank()
        console.success("Preparation complete")
        return 0
    if args.command == "run":
        root = run_benchmark(
            config,
            artifacts,
            suite=args.suite,
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
