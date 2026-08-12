from __future__ import annotations

import csv
import io
import statistics
from collections import Counter
from pathlib import Path
from typing import Any

from .artifacts import atomic_write_text, read_json, utc_now, write_json
from .dataset import load_queries
from .models import PROFILES


def _median(values: list[float]) -> float | None:
    return statistics.median(values) if values else None


def _aggregate(rows: list[dict[str, Any]], profile: str) -> dict[str, Any]:
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
                int(row.get("observed_docids", 0)) for row in completed
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
    answer_quality = _manual_quality(run_root)
    retrieval_quality = _retrieval_quality(run_root, pairs)
    summary = {
        "generated_at": utc_now(),
        "run_id": metadata["run_id"],
        "suite": metadata["suite"],
        "model": metadata["model"],
        "reasoning_effort": metadata["reasoning_effort"],
        "environment": metadata["environment"],
        "planned_pairs": len(metadata["query_ids"]),
        "persisted_pairs": len(pairs),
        "completed_pairs": sum(bool(pair["eligible"]) for pair in pairs),
        "profiles": {profile: _aggregate(pairs, profile) for profile in PROFILES},
        "quality": {
            "answer": answer_quality,
            "retrieval": retrieval_quality,
        },
    }
    report_dir = run_root / "report"
    write_json(report_dir / "summary.json", summary)
    baseline = summary["profiles"]["baseline"]
    treatment = summary["profiles"]["zvec-grep"]
    quality_line = (
        f"Baseline {answer_quality['baseline_accuracy_percent']:.2f}% · "
        f"zvec-grep {answer_quality['treatment_accuracy_percent']:.2f}% · "
        f"{answer_quality['scored_pairs']} scored pairs"
        if answer_quality.get("status") == "scored"
        else "pending evaluation"
    )
    baseline_retrieval = retrieval_quality["baseline"]
    treatment_retrieval = retrieval_quality["zvec-grep"]
    markdown = f"""# BrowseComp-Plus paired report

- Run: `{summary["run_id"]}`
- Suite: `{summary["suite"]}`
- Model: `{summary["model"]}`
- Reasoning: `{summary["reasoning_effort"]}`
- Codex: `{summary["environment"]["codex_version"]}`
- zvec-grep: `{summary["environment"]["zg_version"]}`
- Completed pairs: {summary["completed_pairs"]} / {summary["planned_pairs"]}
- Quality: {quality_line}

| Metric | Baseline | zvec-grep |
|---|---:|---:|
| Completed | {baseline["completed"]} | {treatment["completed"]} |
| Input tokens | {baseline["tokens"]["input_total"]} | {treatment["tokens"]["input_total"]} |
| Cached input tokens | {baseline["tokens"]["cached_input_total"]} | {treatment["tokens"]["cached_input_total"]} |
| Output tokens | {baseline["tokens"]["output_total"]} | {treatment["tokens"]["output_total"]} |
| Median total tokens | {baseline["tokens"]["median_total"]} | {treatment["tokens"]["median_total"]} |
| Total wall seconds | {baseline["wall_seconds"]["total"]:.1f} | {treatment["wall_seconds"]["total"]:.1f} |
| Median wall seconds | {baseline["wall_seconds"]["median"]} | {treatment["wall_seconds"]["median"]} |
| Tool calls | {baseline["tools"]["total"]} | {treatment["tools"]["total"]} |
| Command calls | {baseline["tools"]["commands"]} | {treatment["tools"]["commands"]} |
| zvec-search calls | {baseline["tools"]["zvec_search"]} | {treatment["tools"]["zvec_search"]} |
| Observed document IDs | {baseline["tools"]["observed_docids"]} | {treatment["tools"]["observed_docids"]} |
| Evidence recall | {baseline_retrieval["mean_recall_percent"]:.2f}% | {treatment_retrieval["mean_recall_percent"]:.2f}% |
| Evidence hit rate | {baseline_retrieval["hit_rate_percent"]:.2f}% | {treatment_retrieval["hit_rate_percent"]:.2f}% |
"""
    atomic_write_text(report_dir / "summary.md", markdown)

    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow(
        [
            "query_id",
            "baseline_status",
            "treatment_status",
            "baseline_tokens",
            "treatment_tokens",
            "baseline_wall_seconds",
            "treatment_wall_seconds",
            "baseline_tool_calls",
            "treatment_tool_calls",
        ]
    )
    for pair in pairs:
        baseline_row = pair["baseline"]
        treatment_row = pair["zvec-grep"]
        baseline_usage = baseline_row.get("usage")
        treatment_usage = treatment_row.get("usage")
        writer.writerow(
            [
                pair["query_id"],
                baseline_row["status"],
                treatment_row["status"],
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
            ]
        )
    atomic_write_text(report_dir / "cases.csv", output.getvalue())
    return report_dir


def _manual_quality(run_root: Path) -> dict[str, Any]:
    path = run_root / "evaluation" / "manual-review.csv"
    if not path.is_file():
        return {"status": "pending"}
    scored: list[tuple[float, float]] = []
    with path.open(encoding="utf-8", newline="") as source:
        for line_number, row in enumerate(csv.DictReader(source), 2):
            baseline = str(row.get("baseline_score", "")).strip()
            treatment = str(row.get("treatment_score", "")).strip()
            if not baseline or not treatment:
                continue
            values = (float(baseline), float(treatment))
            if any(value < 0 or value > 1 for value in values):
                raise ValueError(
                    f"{path}:{line_number}: manual scores must be between 0 and 1"
                )
            scored.append(values)
    if not scored:
        return {"status": "pending", "source": str(path)}
    wins = sum(treatment > baseline for baseline, treatment in scored)
    losses = sum(treatment < baseline for baseline, treatment in scored)
    return {
        "status": "scored",
        "source": str(path),
        "scored_pairs": len(scored),
        "baseline_accuracy_percent": 100
        * sum(baseline for baseline, _ in scored)
        / len(scored),
        "treatment_accuracy_percent": 100
        * sum(treatment for _, treatment in scored)
        / len(scored),
        "treatment_wins": wins,
        "treatment_losses": losses,
        "ties": len(scored) - wins - losses,
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
    run_root: Path, pairs: list[dict[str, Any]]
) -> dict[str, dict[str, int | float]]:
    query_path = run_root.parent.parent / "source" / "browsecomp_plus_decrypted.jsonl"
    by_id = {str(query["query_id"]): query for query in load_queries(query_path)}
    recalls: dict[str, list[float]] = {profile: [] for profile in PROFILES}
    hits: Counter[str] = Counter()
    for pair in pairs:
        query_id = str(pair["query_id"])
        expected = _expected_docids(by_id[query_id]["gold_docs"])
        if not expected:
            continue
        for profile in PROFILES:
            result_path = run_root / "cases" / query_id / profile / "result.json"
            result = read_json(result_path)
            if result["status"] != "completed":
                continue
            observed = set(result["trace"].get("observed_docids", []))
            matched = expected & observed
            recalls[profile].append(len(matched) / len(expected))
            hits[profile] += bool(matched)
    return {
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


def write_checkpoint(run_root: Path, *, through: int) -> Path:
    report = generate_report(run_root)
    source = (report / "summary.md").read_text(encoding="utf-8")
    output = run_root / "checkpoints" / f"checkpoint-{through:03d}.md"
    atomic_write_text(output, source)
    return output
