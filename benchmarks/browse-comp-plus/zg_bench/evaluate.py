from __future__ import annotations

import csv
import io
from pathlib import Path

from .artifacts import atomic_write_text, read_json, write_json
from .dataset import load_queries
from .models import PROFILES


def export_official(artifacts: Path, run_root: Path) -> Path:
    output_root = run_root / "evaluation" / "official-input"
    metadata = read_json(run_root / "run.json")
    for profile in PROFILES:
        profile_root = output_root / profile
        profile_root.mkdir(parents=True, exist_ok=True)
        for query_id in metadata["query_ids"]:
            result_path = run_root / "cases" / str(query_id) / profile / "result.json"
            if not result_path.is_file():
                continue
            result = read_json(result_path)
            if result["status"] != "completed":
                continue
            calls = result["trace"].get("tool_calls", [])
            counts: dict[str, int] = {}
            for call in calls:
                name = str(call.get("name", "unknown"))
                counts[name] = counts.get(name, 0) + 1
            row = {
                "query_id": str(query_id),
                "tool_call_counts": counts,
                "status": "completed"
                if result["status"] == "completed"
                else result["status"],
                "retrieved_docids": result["trace"].get("observed_docids", []),
                "result": [
                    {
                        "type": "output_text",
                        "output": result["trace"].get("final_response", ""),
                    }
                ],
            }
            write_json(profile_root / f"{query_id}.json", row)
    return output_root


def export_manual(artifacts: Path, run_root: Path) -> Path:
    path = run_root / "evaluation" / "manual-review.csv"
    if path.is_file():
        return path
    queries = load_queries(artifacts / "source" / "browsecomp_plus_decrypted.jsonl")
    by_id = {str(row["query_id"]): row for row in queries}
    metadata = read_json(run_root / "run.json")
    output = io.StringIO()
    fields = [
        "query_id",
        "question",
        "expected_answer",
        "baseline_response",
        "baseline_score",
        "treatment_response",
        "treatment_score",
        "notes",
    ]
    writer = csv.DictWriter(output, fieldnames=fields)
    writer.writeheader()
    for query_id in metadata["query_ids"]:
        query = by_id[str(query_id)]
        responses: dict[str, str] = {}
        for profile in PROFILES:
            path = run_root / "cases" / str(query_id) / profile / "result.json"
            result = read_json(path) if path.is_file() else None
            responses[profile] = (
                str(result["trace"].get("final_response", ""))
                if result and result.get("status") == "completed"
                else ""
            )
        writer.writerow(
            {
                "query_id": query_id,
                "question": query["query"],
                "expected_answer": query.get("answer", ""),
                "baseline_response": responses["baseline"],
                "baseline_score": "",
                "treatment_response": responses["zvec-grep"],
                "treatment_score": "",
                "notes": "",
            }
        )
    atomic_write_text(path, output.getvalue())
    return path
