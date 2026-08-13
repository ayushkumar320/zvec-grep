from __future__ import annotations

import json
import platform
import random
import sys
from pathlib import Path
from typing import Any

from .artifacts import (
    fingerprint,
    new_run_id,
    read_json,
    sha256_file,
    utc_now,
    write_json,
)
from .codex import (
    cancellation_requested,
    reset_cancellation,
    run_attempt,
    terminate_active_processes,
    validate_model,
)
from .config import BENCHMARK_ROOT, BenchmarkConfig, PROMPT_PATH, SUITES_DIR
from .corpus import prepared_corpus
from .dataset import load_queries, prepared_dataset
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


def _attempt_results(selected_path: Path) -> list[dict[str, Any]]:
    attempts_root = selected_path.parent / "attempts"
    return [
        read_json(path)
        for path in sorted(attempts_root.glob("attempt-*/result.json"))
    ]


def _remaining_attempts(config: BenchmarkConfig, selected_path: Path) -> int:
    failures = sum(
        result.get("infrastructure_failure") is True
        for result in _attempt_results(selected_path)
    )
    return max(0, config.run.infrastructure_retries + 1 - failures)


def _needs_run(config: BenchmarkConfig, path: Path) -> bool:
    if not path.is_file():
        return True
    result = read_json(path)
    if result["status"] in {"completed", "failed"}:
        return False
    if result["status"] not in {"infrastructure_failed", "interrupted"}:
        raise RuntimeError(f"unknown trial status in {path}: {result['status']}")
    return _remaining_attempts(config, path) > 0


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
    persisted_attempts = _attempt_results(selected_path)
    if selected_path.is_file():
        existing = read_json(selected_path)
        if not _needs_run(config, selected_path):
            return _result_from_dict(existing)
    elif persisted_attempts:
        latest = persisted_attempts[-1]
        if (
            latest["status"] in {"completed", "failed"}
            or _remaining_attempts(config, selected_path) == 0
        ):
            write_json(selected_path, latest)
            return _result_from_dict(latest)

    attempts_root = selected_path.parent / "attempts"
    attempts_root.mkdir(parents=True, exist_ok=True)
    existing_attempts = sorted(attempts_root.glob("attempt-*"))
    first_attempt = len(existing_attempts) + 1
    final: AttemptResult | None = None
    for offset in range(_remaining_attempts(config, selected_path)):
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


def _execution_source_fingerprint() -> str:
    paths = [
        BENCHMARK_ROOT / "zg_bench" / name
        for name in (
            "codex.py",
            "config.py",
            "dataset.py",
            "models.py",
            "profiles.py",
            "runner.py",
            "trace.py",
        )
    ]
    return fingerprint(
        value
        for path in paths
        for value in (str(path.relative_to(BENCHMARK_ROOT)), sha256_file(path))
    )


def _run_protocol(
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    suite: str,
    query_ids: list[str],
    queries: dict[str, dict[str, Any]],
    profile_orders: dict[str, tuple[Profile, Profile]],
    model: str,
    reasoning_effort: str,
    codex: Path,
    codex_version: str,
    profiles_fingerprint: str,
) -> dict[str, Any]:
    dataset_state = read_json(artifacts / "state" / "dataset.json")
    corpus_state = read_json(artifacts / "state" / "corpus.json")
    index_state = read_json(artifacts / "state" / "index.json")
    return {
        "execution": "sequential",
        "execution_source_sha256": _execution_source_fingerprint(),
        "runner_path": str(BENCHMARK_ROOT),
        "task_prompt_sha256": sha256_file(PROMPT_PATH),
        "query_set_sha256": dataset_state["queries"]["sha256"],
        "suite": suite,
        "query_ids_sha256": fingerprint(query_ids),
        "selected_queries_sha256": fingerprint(
            json.dumps(queries[query_id], sort_keys=True, ensure_ascii=False)
            for query_id in query_ids
        ),
        "profile_orders_sha256": fingerprint(
            f"{query_id}:{','.join(profile_orders[query_id])}"
            for query_id in query_ids
        ),
        "corpus_fingerprint": corpus_state["fingerprint"],
        "index_fingerprint": index_state["fingerprint"],
        "profiles_fingerprint": profiles_fingerprint,
        "codex_sha256": sha256_file(codex),
        "codex_path": str(codex),
        "codex_version": codex_version,
        "platform": platform.platform(),
        "machine": platform.machine(),
        "python": sys.version.split()[0],
        "model": model,
        "reasoning_effort": reasoning_effort,
        "sandbox": "workspace-write",
        "web_search": "disabled",
        "history_persistence": "none",
        "git_ceiling": "workspace-parent",
        "checkpoint_every": config.run.checkpoint_every,
        "infrastructure_retries": config.run.infrastructure_retries,
        "idle_timeout_seconds": config.run.idle_timeout_seconds,
        "mcp_tool_timeout_seconds": config.zvec_grep.mcp_tool_timeout_seconds,
        "configuration_sha256": sha256_file(config.path),
    }


def _validate_run_protocol(recorded: dict[str, Any], actual: dict[str, Any]) -> None:
    if recorded == actual:
        return
    changed = sorted(
        key
        for key in set(recorded) | set(actual)
        if recorded.get(key) != actual.get(key)
    )
    raise RuntimeError(
        "run protocol changed after this run was created: "
        + ", ".join(changed)
        + "; restore the recorded setup or start a new run"
    )


def _protocol_fingerprint(protocol: dict[str, Any]) -> str:
    return fingerprint([json.dumps(protocol, sort_keys=True, separators=(",", ":"))])


def _result_from_dict(raw: dict[str, Any]) -> AttemptResult:
    from .models import ToolCall, TraceSummary, Usage

    trace = raw["trace"]
    raw_usage = trace["usage"]
    usage = (
        Usage(
            input_tokens=int(raw_usage["input_tokens"]),
            cached_input_tokens=int(raw_usage["cached_input_tokens"]),
            output_tokens=int(raw_usage["output_tokens"]),
            reasoning_output_tokens=int(raw_usage["reasoning_output_tokens"]),
        )
        if isinstance(raw_usage, dict)
        else None
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
    run_id: str | None = None,
    codex_bin: str = "codex",
    zg_bin: str = "zg",
) -> Path:
    model = config.run.model
    reasoning_effort = config.run.reasoning_effort
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
    if not codex_version.ok or not codex_version.stdout.strip():
        raise RuntimeError("could not determine the installed Codex version")
    zg_version = run_command([zg, "version"], timeout=30)
    actual_zg_version = (
        zg_version.stdout.strip().splitlines()[0] if zg_version.stdout else ""
    )
    if not zg_version.ok or not actual_zg_version:
        raise RuntimeError("could not determine the installed zvec-grep version")
    missing_states = []
    if prepared_dataset(config, artifacts) is None:
        missing_states.append("dataset")
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
            raise RuntimeError("cannot resume a run after changing [run].model")
        if metadata["reasoning_effort"] != reasoning_effort:
            raise RuntimeError(
                "cannot resume a run after changing [run].reasoning_effort"
            )
        suite = str(metadata["suite"])
        recorded_manifest = Path(metadata["profiles_manifest"])
        if recorded_manifest.resolve() != profiles_manifest_path.resolve():
            raise RuntimeError("run profile manifest path does not match its run")
        profile_manifest = validate_profiles(recorded_manifest, zg_bin=str(zg))
        if metadata["profiles_fingerprint"] != profile_manifest["fingerprint"]:
            raise RuntimeError("run profile fingerprint does not match its manifest")
        recorded_protocol = metadata["protocol"]
        actual_protocol = _run_protocol(
            config,
            artifacts,
            suite=suite,
            query_ids=query_ids,
            queries=by_id,
            profile_orders=profile_orders,
            model=model,
            reasoning_effort=reasoning_effort,
            codex=codex,
            codex_version=codex_version.stdout.strip(),
            profiles_fingerprint=profile_manifest["fingerprint"],
        )
        if metadata["protocol_fingerprint"] != _protocol_fingerprint(
            recorded_protocol
        ):
            raise RuntimeError("recorded run protocol fingerprint is invalid")
        _validate_run_protocol(recorded_protocol, actual_protocol)
    else:
        required_states = {
            name: artifacts / "state" / f"{name}.json"
            for name in ("corpus", "index")
        }
        query_ids = load_suite(suite, queries)
        profile_orders = _randomized_profile_orders(query_ids)
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
        protocol = _run_protocol(
            config,
            artifacts,
            suite=suite,
            query_ids=query_ids,
            queries=by_id,
            profile_orders=profile_orders,
            model=model,
            reasoning_effort=reasoning_effort,
            codex=codex,
            codex_version=codex_version.stdout.strip(),
            profiles_fingerprint=profile_manifest["fingerprint"],
        )
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
            "checkpoint_every": config.run.checkpoint_every,
            "protocol": protocol,
            "protocol_fingerprint": _protocol_fingerprint(protocol),
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
    for query_id in query_ids:
        _write_pair(run_root, query_id)
    tasks: list[tuple[dict[str, Any], Profile]] = []
    for query_id in query_ids:
        for profile in profile_orders[query_id]:
            if _needs_run(config, _result_path(run_root, query_id, profile)):
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

    last_checkpoint = (
        completed_pairs(run_root, query_ids) // config.run.checkpoint_every
    )
    reset_cancellation()
    try:
        for query, profile in tasks:
            query_id = str(query["query_id"])
            result = _run_profile(
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
        run_id=run_id,
        codex_bin=codex_bin,
        zg_bin=zg_bin,
    )
