from __future__ import annotations

import csv
import io
import json
import statistics
from collections import Counter
from pathlib import Path
from typing import Any

from .artifacts import atomic_write_text, read_json, utc_now, write_json
from .dataset import load_queries
from .models import PROFILES
from .trace import extract_docids, mcp_result_text, parse_trace


def _median(values: list[float]) -> float | None:
    return statistics.median(values) if values else None


def _display_number(value: int | float | None, *, digits: int = 2) -> str:
    return "-" if value is None else f"{value:.{digits}f}"


def _markdown_cell(value: Any) -> str:
    if value is None:
        return "-"
    return str(value).replace("|", "\\|").replace("\n", " ")


def _aggregate(
    rows: list[dict[str, Any]], profile: str, run_root: Path
) -> dict[str, Any]:
    selected = [row[profile] for row in rows]
    completed = [row for row in selected if row["status"] == "completed"]
    measured = [row for row in completed if isinstance(row.get("usage"), dict)]
    return {
        "attempts": len(selected),
        "completed": len(completed),
        "status_counts": dict(Counter(row["status"] for row in selected)),
        "tokens": {
            "input_total": sum(row["usage"].get("input_tokens", 0) for row in measured),
            "cached_input_total": sum(
                row["usage"].get("cached_input_tokens", 0) for row in measured
            ),
            "output_total": sum(
                row["usage"].get("output_tokens", 0) for row in measured
            ),
            "reasoning_output_total": sum(
                row["usage"].get("reasoning_output_tokens", 0) for row in measured
            ),
            "available": len(measured),
            "unavailable": len(completed) - len(measured),
            "median_total": _median(
                [
                    row["usage"].get("input_tokens", 0)
                    + row["usage"].get("output_tokens", 0)
                    for row in measured
                ]
            ),
        },
        "wall_seconds": {
            "total": sum(float(row["wall_seconds"]) for row in completed),
            "median": _median([float(row["wall_seconds"]) for row in completed]),
        },
        "tools": {
            "total": sum(int(row.get("tool_calls", 0)) for row in completed),
            "commands": sum(
                int(row.get("tool_call_counts", {}).get("command_execution", 0))
                for row in completed
            ),
            "zvec_search": sum(
                int(row.get("tool_call_counts", {}).get("zvec_grep_search", 0))
                for row in completed
            ),
            "observed_docids": sum(
                len(
                    _observed_docids(
                        Path(
                            read_json(
                                run_root
                                / "cases"
                                / str(pair["query_id"])
                                / profile
                                / "result.json"
                            )["paths"]["events"]
                        )
                    )
                )
                for pair in rows
                if pair[profile]["status"] == "completed"
            ),
        },
    }


def _pairs(run_root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in sorted((run_root / "cases").glob("*/pair.json")):
        rows.append(read_json(path))
    return rows


def generate_report(run_root: Path) -> Path:
    metadata = read_json(run_root / "run.json")
    pairs = _pairs(run_root)
    eligible_pairs = [pair for pair in pairs if pair["eligible"]]
    query_path = run_root.parent.parent / "source" / "browsecomp_plus_decrypted.jsonl"
    queries = {str(query["query_id"]): query for query in load_queries(query_path)}
    eligible_ids = {str(pair["query_id"]) for pair in eligible_pairs}
    answer_quality = _judge_quality(run_root, eligible_ids)
    usage_quality = _zvec_grep_usage_quality(
        run_root, eligible_ids, str(metadata["suite"])
    )
    retrieval_quality = _retrieval_quality(run_root, eligible_pairs, queries)
    interaction_quality = _tool_interaction_quality(
        run_root, eligible_pairs, queries
    )
    evaluation_summary = run_root / "evaluation" / "summary.json"
    evaluator = (
        read_json(evaluation_summary) if evaluation_summary.is_file() else None
    )
    runtime_setups = metadata["runtime_setups"]
    runtime_preparation = {
        "sessions": len(runtime_setups),
        "total_wall_seconds": sum(
            float(setup["total_wall_seconds"]) for setup in runtime_setups
        ),
        "server_start_wall_seconds": sum(
            float(setup["server_start_wall_seconds"]) for setup in runtime_setups
        ),
        "profile_preparation_wall_seconds": sum(
            float(setup["profile_preparation_wall_seconds"])
            for setup in runtime_setups
        ),
        "profile_install_wall_seconds": sum(
            float(setup["profile_install_wall_seconds"])
            for setup in runtime_setups
        ),
        "warmup_wall_seconds": sum(
            float(setup["warmup_wall_seconds"]) for setup in runtime_setups
        ),
    }
    summary = {
        "generated_at": utc_now(),
        "run_id": metadata["run_id"],
        "suite": metadata["suite"],
        "model": metadata["model"],
        "reasoning_effort": metadata["reasoning_effort"],
        "environment": metadata["environment"],
        "index_build_wall_seconds": metadata["index_build_wall_seconds"],
        "runtime_preparation": runtime_preparation,
        "planned_pairs": len(metadata["query_ids"]),
        "persisted_pairs": len(pairs),
        "completed_pairs": sum(bool(pair["eligible"]) for pair in pairs),
        "profiles": {
            profile: _aggregate(eligible_pairs, profile, run_root)
            for profile in PROFILES
        },
        "quality": {
            "answer": answer_quality,
            "retrieval": retrieval_quality,
            "zvec_grep_usage": usage_quality,
        },
        "evaluator": evaluator,
        "tool_interaction_batches": interaction_quality,
    }
    report_dir = run_root / "report"
    write_json(report_dir / "summary.json", summary)
    baseline = summary["profiles"]["baseline"]
    treatment = summary["profiles"]["zvec-grep"]
    quality_line = "pending evaluation"
    if answer_quality.get("status") in {"scored", "partial"}:
        pair_label = "pair" if answer_quality["scored_pairs"] == 1 else "pairs"
        quality_line = (
            f"Baseline {answer_quality['baseline_accuracy_percent']:.2f}% · "
            f"zvec-grep {answer_quality['treatment_accuracy_percent']:.2f}% · "
            f"{answer_quality['scored_pairs']} scored {pair_label}"
        )
        if answer_quality["status"] == "partial":
            quality_line += " (partial)"
    evaluator_section = ""
    if evaluator:
        evaluator_cost = evaluator["cost"]
        evaluator_rows = "\n".join(
            "| {label} | {completed_calls} | {usage_records} | {input_tokens} | "
            "{cached_input_tokens} | {output_tokens} | "
            "{reasoning_output_tokens} | {wall_seconds:.1f} |".format(
                label=label,
                usage_records=(
                    f"{evaluator_cost[key]['usage_available']} / "
                    f"{evaluator_cost[key]['completed_calls']}"
                ),
                **evaluator_cost[key],
            )
            for label, key in (
                ("Answer judgements", "answer_judgements"),
                ("zvec-grep usage audits", "zvec_grep_usage_audits"),
                ("Total evaluator cost", "total"),
            )
        )
        evaluator_section = f"""

## Evaluator cost

- Model: `{evaluator["model"]}`
- Reasoning: `{evaluator["reasoning_effort"]}`
- Scope: evaluator-only cost; excluded from baseline and zvec-grep run metrics

| Workload | Completed calls | Usage records | Input tokens | Cached input tokens | Output tokens | Reasoning output tokens | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
{evaluator_rows}
"""
    usage_audit_line = ""
    usage_audit_section = ""
    if usage_quality["status"] in {"scored", "partial"}:
        usage_audit_line = (
            f"\n- zvec-grep usage audit: {usage_quality['correct_cases']} / "
            f"{usage_quality['evaluated_cases']} correct"
        )
        rows = "\n".join(
            "| {query_id} | {used} | {correct} | {calls} | {reasoning} |".format(
                query_id=row["query_id"],
                used="Yes" if row["used_zvec_grep"] else "No",
                correct="Yes" if row["correct_usage"] else "No",
                calls=row["observed_zvec_grep_search_calls"],
                reasoning=str(row["reasoning"])
                .replace("|", "\\|")
                .replace("\n", " "),
            )
            for row in usage_quality["cases"]
        )
        usage_audit_section = f"""

## zvec-grep usage audit

| Case | Used zvec-grep | Correct usage | Search calls | Reasoning |
| --- | --- | --- | ---: | --- |
{rows}
"""
    baseline_evidence = retrieval_quality["evidence"]["baseline"]
    treatment_evidence = retrieval_quality["evidence"]["zvec-grep"]
    baseline_gold = retrieval_quality["gold"]["baseline"]
    treatment_gold = retrieval_quality["gold"]["zvec-grep"]
    baseline_interactions = interaction_quality["baseline"]
    treatment_interactions = interaction_quality["zvec-grep"]
    runtime_preparation = summary["runtime_preparation"]
    environment = summary["environment"]
    available_cpu_count = environment["available_cpu_count"]
    logical_cpu_count = environment["logical_cpu_count"]
    cpu_counts = (
        f"{available_cpu_count if available_cpu_count is not None else '-'} "
        "available / "
        f"{logical_cpu_count if logical_cpu_count is not None else '-'} logical"
    )
    environment_rows = "\n".join(
        f"| {label} | {_markdown_cell(value)} |"
        for label, value in (
            ("Operating system", environment["operating_system"]),
            ("Kernel / platform", environment["platform"]),
            ("Architecture", environment["machine"]),
            ("CPU", environment["cpu_model"]),
            ("CPU count", cpu_counts),
            ("Python", environment["python"]),
            ("Codex", environment["codex_version"]),
            ("Codex executable", environment["codex_bin"]),
            ("Codex sandbox", environment["codex_sandbox"]),
            ("Web search", environment["web_search"]),
            ("History persistence", environment["history_persistence"]),
            ("zvec-grep", environment["zg_version"]),
            ("Embedding", environment["embedding"]),
            (
                "Index embedding concurrency",
                environment["embedding_concurrency"],
            ),
            ("Configured embedding device", environment["embedding_device"]),
            ("Maximum indexed file size", environment["max_filesize"]),
            ("MCP transport", environment["mcp_transport"]),
            (
                "MCP tool timeout",
                f"{environment['mcp_tool_timeout_seconds']} seconds",
            ),
            ("zvec-grep server", environment["zg_server_url"]),
        )
    )
    markdown = f"""# BrowseComp-Plus paired report

- Run: `{summary["run_id"]}`
- Suite: `{summary["suite"]}`
- Model: `{summary["model"]}`
- Reasoning: `{summary["reasoning_effort"]}`
- Index build wall time: {summary["index_build_wall_seconds"]:,.1f} seconds (one-time preparation; excluded from measured run wall time)
- Runtime preparation wall time: {runtime_preparation["total_wall_seconds"]:,.1f} seconds across {runtime_preparation["sessions"]} session(s) (excluded from measured run wall time)
- Completed pairs: {summary["completed_pairs"]} / {summary["planned_pairs"]}
- Quality: {quality_line}{usage_audit_line}

## Environment

| Setting | Value |
| --- | --- |
{environment_rows}

## Runtime preparation

| Phase | Wall seconds |
| --- | ---: |
| End-to-end preparation | {runtime_preparation["total_wall_seconds"]:.1f} |
| Server startup | {runtime_preparation["server_start_wall_seconds"]:.1f} |
| Profile preparation | {runtime_preparation["profile_preparation_wall_seconds"]:.1f} |
| `zg install` (included in profile preparation) | {runtime_preparation["profile_install_wall_seconds"]:.1f} |
| Runtime verification and index warmup | {runtime_preparation["warmup_wall_seconds"]:.1f} |

## Paired results

| Metric | Baseline | zvec-grep |
| --- | ---: | ---: |
| Completed | {baseline["completed"]} | {treatment["completed"]} |
| Input tokens | {baseline["tokens"]["input_total"]} | {treatment["tokens"]["input_total"]} |
| Cached input tokens | {baseline["tokens"]["cached_input_total"]} | {treatment["tokens"]["cached_input_total"]} |
| Output tokens | {baseline["tokens"]["output_total"]} | {treatment["tokens"]["output_total"]} |
| Median total tokens | {baseline["tokens"]["median_total"]} | {treatment["tokens"]["median_total"]} |
| Total wall seconds | {baseline["wall_seconds"]["total"]:.1f} | {treatment["wall_seconds"]["total"]:.1f} |
| Median wall seconds | {_display_number(baseline["wall_seconds"]["median"])} | {_display_number(treatment["wall_seconds"]["median"])} |
| Tool calls | {baseline["tools"]["total"]} | {treatment["tools"]["total"]} |
| Command calls | {baseline["tools"]["commands"]} | {treatment["tools"]["commands"]} |
| zvec-search calls | {baseline["tools"]["zvec_search"]} | {treatment["tools"]["zvec_search"]} |
| Document ID mentions | {baseline["tools"]["observed_docids"]} | {treatment["tools"]["observed_docids"]} |
| Evidence recall | {baseline_evidence["mean_recall_percent"]:.2f}% | {treatment_evidence["mean_recall_percent"]:.2f}% |
| Evidence hit rate | {baseline_evidence["hit_rate_percent"]:.2f}% | {treatment_evidence["hit_rate_percent"]:.2f}% |
| Gold recall | {baseline_gold["mean_recall_percent"]:.2f}% | {treatment_gold["mean_recall_percent"]:.2f}% |
| Gold hit rate | {baseline_gold["hit_rate_percent"]:.2f}% | {treatment_gold["hit_rate_percent"]:.2f}% |
| Tool interaction batches | {baseline_interactions["total"]} | {treatment_interactions["total"]} |
| Median tool interaction batches | {_display_number(baseline_interactions["median"])} | {_display_number(treatment_interactions["median"])} |
| Evidence found cases | {baseline_interactions["first_evidence_hits"]} | {treatment_interactions["first_evidence_hits"]} |
| Mean batch to first evidence (hits only) | {_display_number(baseline_interactions["first_evidence_mean"])} | {_display_number(treatment_interactions["first_evidence_mean"])} |
| Gold found cases | {baseline_interactions["first_gold_hits"]} | {treatment_interactions["first_gold_hits"]} |
| Mean batch to first gold (hits only) | {_display_number(baseline_interactions["first_gold_mean"])} | {_display_number(treatment_interactions["first_gold_mean"])} |
{evaluator_section}{usage_audit_section}"""
    atomic_write_text(report_dir / "summary.md", markdown)

    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow(
        [
            "query_id",
            "baseline_status",
            "treatment_status",
            "baseline_correct",
            "treatment_correct",
            "baseline_tokens",
            "treatment_tokens",
            "baseline_wall_seconds",
            "treatment_wall_seconds",
            "baseline_tool_calls",
            "treatment_tool_calls",
            "zvec_grep_usage_correct",
        ]
    )
    for pair in pairs:
        query_id = str(pair["query_id"])
        baseline_row = pair["baseline"]
        treatment_row = pair["zvec-grep"]
        baseline_usage = baseline_row.get("usage")
        treatment_usage = treatment_row.get("usage")
        usage_audit_path = (
            run_root
            / "evaluation"
            / "usage-audit"
            / "results"
            / f"{query_id}.json"
        )
        usage_audit = (
            read_json(usage_audit_path) if usage_audit_path.is_file() else {}
        )
        correctness: dict[str, bool | str] = {}
        for profile in PROFILES:
            path = (
                run_root
                / "evaluation"
                / "results"
                / profile
                / f"{query_id}.json"
            )
            result = read_json(path) if path.is_file() else {}
            correctness[profile] = (
                bool(result["correct"])
                if result.get("status") == "completed"
                else ""
            )
        writer.writerow(
            [
                query_id,
                baseline_row["status"],
                treatment_row["status"],
                correctness["baseline"],
                correctness["zvec-grep"],
                baseline_usage.get("total_tokens", "")
                if isinstance(baseline_usage, dict)
                else "",
                treatment_usage.get("total_tokens", "")
                if isinstance(treatment_usage, dict)
                else "",
                baseline_row["wall_seconds"],
                treatment_row["wall_seconds"],
                baseline_row.get("tool_calls", 0),
                treatment_row.get("tool_calls", 0),
                usage_audit.get("correct_usage", "")
                if usage_audit.get("status") == "completed"
                else "",
            ]
        )
    atomic_write_text(report_dir / "cases.csv", output.getvalue())
    return report_dir


def _judge_quality(run_root: Path, eligible_ids: set[str]) -> dict[str, Any]:
    scored: list[tuple[bool, bool]] = []
    for query_id in sorted(eligible_ids):
        paths = {
            profile: run_root
            / "evaluation"
            / "results"
            / profile
            / f"{query_id}.json"
            for profile in PROFILES
        }
        if not all(path.is_file() for path in paths.values()):
            continue
        results = {profile: read_json(path) for profile, path in paths.items()}
        if not all(result.get("status") == "completed" for result in results.values()):
            continue
        scored.append(
            (
                bool(results["baseline"]["correct"]),
                bool(results["zvec-grep"]["correct"]),
            )
        )
    if not scored:
        return {"status": "pending"}
    wins = sum(treatment > baseline for baseline, treatment in scored)
    losses = sum(treatment < baseline for baseline, treatment in scored)
    return {
        "status": "scored" if len(scored) == len(eligible_ids) else "partial",
        "source": str((run_root / "evaluation" / "results").resolve()),
        "scored_pairs": len(scored),
        "baseline_accuracy_percent": 100
        * sum(int(baseline) for baseline, _ in scored)
        / len(scored),
        "treatment_accuracy_percent": 100
        * sum(int(treatment) for _, treatment in scored)
        / len(scored),
        "treatment_wins": wins,
        "treatment_losses": losses,
        "ties": len(scored) - wins - losses,
    }


def _zvec_grep_usage_quality(
    run_root: Path, eligible_ids: set[str], suite: str
) -> dict[str, Any]:
    if suite != "smoke":
        return {"status": "not_applicable"}
    cases: list[dict[str, Any]] = []
    for query_id in sorted(eligible_ids):
        path = (
            run_root
            / "evaluation"
            / "usage-audit"
            / "results"
            / f"{query_id}.json"
        )
        if not path.is_file():
            continue
        result = read_json(path)
        if result.get("status") != "completed":
            continue
        cases.append(
            {
                "query_id": query_id,
                "used_zvec_grep": bool(result["used_zvec_grep"]),
                "correct_usage": bool(result["correct_usage"]),
                "observed_zvec_grep_search_calls": int(
                    result["observed_zvec_grep_search_calls"]
                ),
                "reasoning": str(result["reasoning"]),
            }
        )
    if not cases:
        return {"status": "pending", "expected_cases": len(eligible_ids)}
    return {
        "status": "scored" if len(cases) == len(eligible_ids) else "partial",
        "expected_cases": len(eligible_ids),
        "evaluated_cases": len(cases),
        "correct_cases": sum(case["correct_usage"] for case in cases),
        "cases": cases,
    }


def _expected_docids(value: Any) -> set[str]:
    result: set[str] = set()
    if not isinstance(value, list):
        return result
    for item in value:
        if isinstance(item, (str, int)):
            result.add(str(item))
        elif isinstance(item, dict) and item.get("docid") is not None:
            result.add(str(item["docid"]))
    return result


def _retrieval_quality(
    run_root: Path,
    pairs: list[dict[str, Any]],
    queries: dict[str, dict[str, Any]],
) -> dict[str, dict[str, dict[str, int | float]]]:
    output: dict[str, dict[str, dict[str, int | float]]] = {}
    for label, field in (("evidence", "evidence_docs"), ("gold", "gold_docs")):
        recalls: dict[str, list[float]] = {profile: [] for profile in PROFILES}
        hits: Counter[str] = Counter()
        for pair in pairs:
            query_id = str(pair["query_id"])
            expected = _expected_docids(queries[query_id][field])
            if not expected:
                continue
            for profile in PROFILES:
                result_path = run_root / "cases" / query_id / profile / "result.json"
                result = read_json(result_path)
                observed = _observed_docids(Path(result["paths"]["events"]))
                matched = expected & observed
                recalls[profile].append(len(matched) / len(expected))
                hits[profile] += bool(matched)
        output[label] = {
            profile: {
                "eligible_cases": len(recalls[profile]),
                "mean_recall_percent": 100 * statistics.fmean(recalls[profile])
                if recalls[profile]
                else 0.0,
                "hit_rate_percent": 100 * hits[profile] / len(recalls[profile])
                if recalls[profile]
                else 0.0,
            }
            for profile in PROFILES
        }
    return output


def _observed_docids(events_path: Path) -> set[str]:
    return set(parse_trace(events_path).observed_docids)


def _tool_interaction_batches(
    events_path: Path,
    *,
    evidence: set[str],
    gold: set[str],
) -> dict[str, int | None]:
    rounds: list[dict[str, set[str]]] = []
    active: set[str] = set()
    round_by_item: dict[str, int] = {}
    with events_path.open(encoding="utf-8", errors="replace") as source:
        for line in source:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            item = event.get("item") or {}
            item_type = item.get("type")
            if event.get("type") == "item.completed" and item_type == "agent_message":
                for item_id in active:
                    round_by_item.pop(item_id, None)
                active.clear()
                continue
            if item_type not in {"command_execution", "mcp_tool_call"}:
                continue
            if event.get("type") not in {"item.started", "item.completed"}:
                continue
            item_id = str(item.get("id", ""))
            if event.get("type") == "item.started":
                if not active:
                    rounds.append({"evidence": set(), "gold": set()})
                active.add(item_id)
                round_by_item[item_id] = len(rounds) - 1
                continue
            round_index = round_by_item.pop(item_id, None)
            if round_index is None:
                if not active:
                    rounds.append({"evidence": set(), "gold": set()})
                round_index = len(rounds) - 1
            if item_type == "command_execution":
                output = str(item.get("aggregated_output", ""))
                observed = extract_docids(item.get("command"), output)
            else:
                output = mcp_result_text(item.get("result"))
                observed = extract_docids(output)
            current = rounds[round_index]
            current["evidence"].update(observed & evidence)
            current["gold"].update(observed & gold)
            active.discard(item_id)

    def first_hit(kind: str) -> int | None:
        return next(
            (number for number, value in enumerate(rounds, 1) if value[kind]),
            None,
        )

    evidence_round = first_hit("evidence")
    gold_round = first_hit("gold")
    return {
        "total": len(rounds),
        "first_evidence": evidence_round,
        "after_first_evidence": (
            len(rounds) - evidence_round if evidence_round is not None else None
        ),
        "first_gold": gold_round,
        "after_first_gold": (
            len(rounds) - gold_round if gold_round is not None else None
        ),
    }


def _tool_interaction_quality(
    run_root: Path,
    pairs: list[dict[str, Any]],
    queries: dict[str, dict[str, Any]],
) -> dict[str, dict[str, int | float | None]]:
    values: dict[str, list[dict[str, int | None]]] = {
        profile: [] for profile in PROFILES
    }
    for pair in pairs:
        query_id = str(pair["query_id"])
        evidence = _expected_docids(queries[query_id]["evidence_docs"])
        gold = _expected_docids(queries[query_id]["gold_docs"])
        for profile in PROFILES:
            result = read_json(
                run_root / "cases" / query_id / profile / "result.json"
            )
            events = Path(result["paths"]["events"])
            values[profile].append(
                _tool_interaction_batches(events, evidence=evidence, gold=gold)
            )

    def aggregate(rows: list[dict[str, int | None]]) -> dict[str, int | float | None]:
        def measured(key: str) -> list[int]:
            return [int(row[key]) for row in rows if row[key] is not None]

        totals = measured("total")
        return {
            "eligible_cases": len(rows),
            "total": sum(totals),
            "mean": statistics.fmean(totals) if totals else None,
            "median": _median(totals),
            "first_evidence_hits": len(measured("first_evidence")),
            "first_evidence_mean": (
                statistics.fmean(measured("first_evidence"))
                if measured("first_evidence")
                else None
            ),
            "first_evidence_median": _median(measured("first_evidence")),
            "after_first_evidence_mean": (
                statistics.fmean(measured("after_first_evidence"))
                if measured("after_first_evidence")
                else None
            ),
            "after_first_evidence_median": _median(
                measured("after_first_evidence")
            ),
            "first_gold_hits": len(measured("first_gold")),
            "first_gold_mean": (
                statistics.fmean(measured("first_gold"))
                if measured("first_gold")
                else None
            ),
            "first_gold_median": _median(measured("first_gold")),
            "after_first_gold_mean": (
                statistics.fmean(measured("after_first_gold"))
                if measured("after_first_gold")
                else None
            ),
            "after_first_gold_median": _median(measured("after_first_gold")),
        }

    return {profile: aggregate(values[profile]) for profile in PROFILES}
