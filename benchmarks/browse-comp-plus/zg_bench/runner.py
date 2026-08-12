from __future__ import annotations

import json
import platform
import random
import sys
from concurrent.futures import Future, ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from .artifacts import new_run_id, read_json, utc_now, write_json
from .codex import (
    cancellation_requested,
    reset_cancellation,
    run_attempt,
    terminate_active_processes,
    validate_model,
)
from .config import BenchmarkConfig, PROMPT_PATH, SUITES_DIR
from .corpus import prepared_corpus
from .dataset import load_queries
from .index import prepared_index
from .models import PROFILES, AttemptResult, Profile
from .process import resolve_executable, run_command
from .profiles import prepare_profiles, prepare_search_runtime, validate_profiles


def load_suite(name: str, queries: list[dict[str, Any]]) -> list[str]:
    path = Path(name)
    if path.suffix != ".txt":
        path = SUITES_DIR / f"{name}.txt"
    if not path.is_file():
        raise ValueError(f"suite not found: {path}")
    lines = [
        line.strip()
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    all_ids = [str(row["query_id"]) for row in queries]
    if lines == ["@all"]:
        return all_ids
    if len(lines) == 1 and lines[0].startswith("@first "):
        count = int(lines[0].split()[1])
        if count < 1 or count > len(all_ids):
            raise ValueError(f"invalid @first count: {count}")
        return all_ids[:count]
    missing = sorted(set(lines) - set(all_ids))
    if missing:
        raise ValueError(f"suite contains unknown query IDs: {', '.join(missing)}")
    if len(set(lines)) != len(lines):
        raise ValueError("suite contains duplicate query IDs")
    return lines


def _randomized_profile_orders(
    query_ids: list[str],
) -> dict[str, tuple[Profile, Profile]]:
    rng = random.SystemRandom()
    orders: list[tuple[Profile, Profile]] = [PROFILES] * (len(query_ids) // 2)
    orders.extend(
        [("zvec-grep", "baseline")] * (len(query_ids) // 2)
    )
    if len(query_ids) % 2:
        orders.append(rng.choice((PROFILES, ("zvec-grep", "baseline"))))
    rng.shuffle(orders)
    return dict(zip(query_ids, orders, strict=True))


def _prompt(query: str) -> str:
    template = PROMPT_PATH.read_text(encoding="utf-8")
    return template.replace("{query}", query).rstrip() + "\n"


def _result_path(run_root: Path, query_id: str, profile: Profile) -> Path:
    return run_root / "cases" / query_id / profile / "result.json"


def _needs_run(path: Path) -> bool:
    return not path.is_file() or read_json(path)["status"] != "completed"


def _run_profile(
    config: BenchmarkConfig,
    artifacts: Path,
    run_root: Path,
    query: dict[str, Any],
    profile: Profile,
    model: str,
    reasoning_effort: str,
    profiles_root: Path,
    codex_bin: str,
    zg_bin: str,
) -> AttemptResult:
    query_id = str(query["query_id"])
    selected_path = _result_path(run_root, query_id, profile)
    if selected_path.is_file():
        existing = read_json(selected_path)
        if existing["status"] == "completed":
            return _result_from_dict(existing)

    attempts_root = selected_path.parent / "attempts"
    attempts_root.mkdir(parents=True, exist_ok=True)
    existing_attempts = sorted(attempts_root.glob("attempt-*"))
    first_attempt = len(existing_attempts) + 1
    final: AttemptResult | None = None
    for offset in range(config.run.infrastructure_retries + 1):
        if cancellation_requested():
            raise InterruptedError("benchmark cancellation requested")
        number = first_attempt + offset
        output = attempts_root / f"attempt-{number:03d}"
        final = run_attempt(
            config,
            artifacts,
            query_id=query_id,
            prompt=_prompt(str(query["query"])),
            profile=profile,
            model=model,
            reasoning_effort=reasoning_effort,
            attempt=number,
            output_dir=output,
            profiles_root=profiles_root,
            codex_bin=codex_bin,
            zg_bin=zg_bin,
            idle_timeout_seconds=config.run.idle_timeout_seconds,
        )
        if (
            final.status == "completed"
            or not final.infrastructure_failure
            or cancellation_requested()
        ):
            break
    assert final is not None
    write_json(selected_path, final.to_dict())
    return final


def _result_from_dict(raw: dict[str, Any]) -> AttemptResult:
    from .models import ToolCall, TraceSummary, Usage

    trace = raw["trace"]
    raw_usage = trace["usage"]
    usage = Usage(
        input_tokens=int(raw_usage["input_tokens"]),
        cached_input_tokens=int(raw_usage["cached_input_tokens"]),
        output_tokens=int(raw_usage["output_tokens"]),
        reasoning_output_tokens=int(raw_usage["reasoning_output_tokens"]),
    )
    return AttemptResult(
        query_id=str(raw["query_id"]),
        profile=raw["profile"],
        status=str(raw["status"]),
        attempt=int(raw["attempt"]),
        started_at=str(raw["started_at"]),
        finished_at=str(raw["finished_at"]),
        wall_seconds=float(raw["wall_seconds"]),
        exit_code=int(raw["exit_code"]),
        infrastructure_failure=bool(raw["infrastructure_failure"]),
        interrupted_by=raw["interrupted_by"],
        trace=TraceSummary(
            thread_id=trace["thread_id"],
            final_response=str(trace["final_response"]),
            last_agent_message=str(trace["last_agent_message"]),
            turn_completed=bool(trace["turn_completed"]),
            usage=usage,
            tool_calls=tuple(ToolCall(**call) for call in trace["tool_calls"]),
            observed_docids=tuple(trace["observed_docids"]),
            errors=tuple(trace["errors"]),
        ),
        paths=dict(raw["paths"]),
    )


def _write_pair(run_root: Path, query_id: str) -> bool:
    results: dict[str, Any] = {}
    for profile in PROFILES:
        path = _result_path(run_root, query_id, profile)
        if not path.is_file():
            return False
        results[profile] = read_json(path)
    baseline = results["baseline"]
    treatment = results["zvec-grep"]

    def metrics(result: dict[str, Any]) -> dict[str, Any]:
        calls = result["trace"].get("tool_calls", [])
        counts: dict[str, int] = {}
        for call in calls:
            name = str(call.get("name", "unknown"))
            counts[name] = counts.get(name, 0) + 1
        return {
            "status": result["status"],
            "wall_seconds": result["wall_seconds"],
            "usage": result["trace"].get("usage"),
            "tool_calls": len(calls),
            "tool_call_counts": counts,
            "observed_docids": len(result["trace"].get("observed_docids", [])),
        }

    pair = {
        "query_id": query_id,
        "eligible": all(result["status"] == "completed" for result in results.values()),
        "baseline": metrics(baseline),
        "zvec-grep": metrics(treatment),
    }
    write_json(run_root / "cases" / query_id / "pair.json", pair)
    return True


def completed_pairs(run_root: Path, query_ids: list[str]) -> int:
    count = 0
    for query_id in query_ids:
        path = run_root / "cases" / query_id / "pair.json"
        if path.is_file() and read_json(path)["eligible"] is True:
            count += 1
    return count


def run_benchmark(
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    suite: str,
    model: str,
    reasoning_effort: str | None = None,
    concurrency: int | None = None,
    run_id: str | None = None,
    codex_bin: str = "codex",
    zg_bin: str = "zg",
) -> Path:
    query_path = artifacts / "source" / "browsecomp_plus_decrypted.jsonl"
    if not query_path.is_file():
        raise RuntimeError("dataset is missing; run 'zg-bench prepare' first")
    queries = load_queries(query_path)
    by_id = {str(row["query_id"]): row for row in queries}
    validate_model(artifacts, model)
    codex = resolve_executable(codex_bin)
    zg = resolve_executable(zg_bin)
    if codex is None or zg is None:
        raise RuntimeError("Codex and zvec-grep must be available before a run")
    codex_version = run_command([codex, "--version"], timeout=30)
    zg_version = run_command([zg, "version"], timeout=30)
    actual_zg_version = (
        zg_version.stdout.strip().splitlines()[0] if zg_version.stdout else ""
    )
    if not zg_version.ok or actual_zg_version != config.zvec_grep.version:
        raise RuntimeError(
            f"zvec-grep {config.zvec_grep.version} is required; found "
            f"{actual_zg_version or 'unknown'}"
        )
    missing_states = []
    if prepared_corpus(config, artifacts) is None:
        missing_states.append("corpus")
    if prepared_index(config, artifacts) is None:
        missing_states.append("index")
    if missing_states:
        raise RuntimeError(
            "benchmark preparation is incomplete: " + ", ".join(missing_states)
        )
    run_id = run_id or new_run_id()
    run_root = artifacts / "runs" / run_id
    metadata_path = run_root / "run.json"
    profiles_root = run_root / "profiles"
    profiles_manifest_path = profiles_root / "manifest.json"
    if metadata_path.is_file():
        metadata = read_json(metadata_path)
        query_ids = [str(value) for value in metadata["query_ids"]]
        profile_orders = {
            str(query_id): tuple(order)
            for query_id, order in metadata["profile_orders"].items()
        }
        if metadata["model"] != model:
            raise RuntimeError("cannot resume a run with a different model")
        reasoning_effort = str(metadata["reasoning_effort"])
        suite = str(metadata["suite"])
        if "profiles_manifest" not in metadata:
            raise RuntimeError(
                "run profile identity is missing; start a new run"
            )
        recorded_manifest = Path(metadata["profiles_manifest"])
        if recorded_manifest.resolve() != profiles_manifest_path.resolve():
            raise RuntimeError("run profile manifest path does not match its run")
        profile_manifest = validate_profiles(recorded_manifest, zg_bin=str(zg))
        if metadata.get("profiles_fingerprint") != profile_manifest["fingerprint"]:
            raise RuntimeError("run profile fingerprint does not match its manifest")
    else:
        required_states = {
            name: artifacts / "state" / f"{name}.json"
            for name in ("corpus", "index")
        }
        query_ids = load_suite(suite, queries)
        profile_orders = _randomized_profile_orders(query_ids)
        reasoning_effort = reasoning_effort or config.run.reasoning_effort
        prepare_profiles(
            config,
            artifacts,
            codex_bin=str(codex),
            zg_bin=str(zg),
            profiles_root=profiles_root,
            manifest_path=profiles_manifest_path,
        )
        profile_manifest = validate_profiles(profiles_manifest_path, zg_bin=str(zg))
        corpus_state = read_json(required_states["corpus"])
        index_state = read_json(required_states["index"])
        metadata = {
            "run_id": run_id,
            "created_at": utc_now(),
            "suite": suite,
            "query_ids": query_ids,
            "model": model,
            "reasoning_effort": reasoning_effort,
            "profiles": list(PROFILES),
            "profile_orders": {
                query_id: list(order)
                for query_id, order in profile_orders.items()
            },
            "profiles_manifest": str(profiles_manifest_path.resolve()),
            "profiles_fingerprint": profile_manifest["fingerprint"],
            "concurrency": concurrency or config.run.concurrency,
            "checkpoint_every": config.run.checkpoint_every,
            "configuration": str(config.path),
            "environment": {
                "platform": platform.platform(),
                "machine": platform.machine(),
                "python": sys.version.split()[0],
                "codex_bin": str(codex),
                "codex_version": codex_version.stdout.strip(),
                "zg_bin": str(zg),
                "zg_version": zg_version.stdout.strip(),
            },
            "corpus_fingerprint": corpus_state["fingerprint"],
            "index_fingerprint": index_state["fingerprint"],
            "runtime_setups": [],
        }
        write_json(metadata_path, metadata)

    missing = [query_id for query_id in query_ids if query_id not in by_id]
    if missing:
        raise RuntimeError(f"run references missing queries: {', '.join(missing)}")
    reasoning_effort = reasoning_effort or config.run.reasoning_effort
    workers = concurrency or int(metadata["concurrency"])
    tasks: list[tuple[dict[str, Any], Profile]] = []
    for query_id in query_ids:
        for profile in profile_orders[query_id]:
            if _needs_run(_result_path(run_root, query_id, profile)):
                tasks.append((by_id[query_id], profile))
    if any(profile == "zvec-grep" for _, profile in tasks):
        runtime_setup = prepare_search_runtime(
            config,
            artifacts,
            zg_bin=str(zg),
            restart_server=True,
        )
        metadata["runtime_setups"].append(runtime_setup)
        write_json(metadata_path, metadata)

    futures: dict[Future[AttemptResult], tuple[str, Profile]] = {}
    last_checkpoint = (
        completed_pairs(run_root, query_ids) // config.run.checkpoint_every
    )
    reset_cancellation()
    with ThreadPoolExecutor(max_workers=workers) as executor:
        try:
            for query, profile in tasks:
                query_id = str(query["query_id"])
                future = executor.submit(
                    _run_profile,
                    config,
                    artifacts,
                    run_root,
                    query,
                    profile,
                    model,
                    reasoning_effort,
                    profiles_root,
                    str(codex),
                    str(zg),
                )
                futures[future] = (query_id, profile)
            for future in as_completed(futures):
                query_id, profile = futures[future]
                result = future.result()
                _write_pair(run_root, query_id)
                done = completed_pairs(run_root, query_ids)
                print(
                    json.dumps(
                        {
                            "run_id": run_id,
                            "query_id": query_id,
                            "profile": profile,
                            "status": result.status,
                            "pairs": done,
                            "total_pairs": len(query_ids),
                            "input_tokens": (
                                result.trace.usage.input_tokens
                                if result.trace.usage
                                else None
                            ),
                            "output_tokens": (
                                result.trace.usage.output_tokens
                                if result.trace.usage
                                else None
                            ),
                            "wall_seconds": round(result.wall_seconds, 3),
                        }
                    ),
                    flush=True,
                )
                checkpoint = done // config.run.checkpoint_every
                if checkpoint > last_checkpoint:
                    from .report import write_checkpoint

                    write_checkpoint(run_root, through=done)
                    last_checkpoint = checkpoint
        except KeyboardInterrupt:
            terminate_active_processes()
            for future in futures:
                future.cancel()
            raise

    metadata["finished_at"] = utc_now()
    metadata["completed_pairs"] = completed_pairs(run_root, query_ids)
    write_json(metadata_path, metadata)
    from .report import generate_report

    generate_report(run_root)
    return run_root


def resume_benchmark(
    config: BenchmarkConfig,
    artifacts: Path,
    run_id: str,
    *,
    codex_bin: str = "codex",
    zg_bin: str = "zg",
) -> Path:
    metadata = read_json(artifacts / "runs" / run_id / "run.json")
    return run_benchmark(
        config,
        artifacts,
        suite=str(metadata["suite"]),
        model=str(metadata["model"]),
        reasoning_effort=str(metadata["reasoning_effort"]),
        concurrency=int(metadata["concurrency"]),
        run_id=run_id,
        codex_bin=codex_bin,
        zg_bin=zg_bin,
    )
