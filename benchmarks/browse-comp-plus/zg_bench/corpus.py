from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, Iterator

import pyarrow.parquet as parquet

from .artifacts import fingerprint, sha256_file, utc_now, write_json
from .config import BenchmarkConfig


def _rows(files: list[Path]) -> Iterator[dict[str, Any]]:
    for path in files:
        source = parquet.ParquetFile(path)
        for batch in source.iter_batches(
            batch_size=256, columns=["docid", "text", "url"]
        ):
            yield from batch.to_pylist()


def _safe_docid(value: Any) -> str:
    docid = str(value)
    if (
        not docid
        or docid in {".", ".."}
        or "/" in docid
        or "\\" in docid
        or "\0" in docid
    ):
        raise ValueError(f"unsafe corpus docid: {docid!r}")
    return docid


def materialize(config: BenchmarkConfig, artifacts: Path) -> Path:
    source = artifacts / "source" / "corpus"
    files = sorted(source.rglob("*.parquet"))
    if not files:
        raise RuntimeError("corpus source is missing; run 'zg-bench fetch' first")

    documents = artifacts / "corpus" / "documents"
    documents.mkdir(parents=True, exist_ok=True)
    manifest_path = artifacts / "state" / "corpus-manifest.jsonl"
    temporary_manifest = manifest_path.with_suffix(".jsonl.tmp")
    temporary_manifest.parent.mkdir(parents=True, exist_ok=True)

    count = 0
    total_bytes = 0
    maximum_bytes = 0
    aggregate_parts: list[str] = [config.dataset.corpus_revision]
    with temporary_manifest.open("w", encoding="utf-8") as manifest:
        for row in _rows(files):
            docid = _safe_docid(row["docid"])
            text = str(row["text"])
            encoded = text.encode("utf-8")
            digest = hashlib.sha256(encoded).hexdigest()
            shard = hashlib.sha256(docid.encode("utf-8")).hexdigest()[:2]
            relative = Path("documents") / shard / f"{docid}.md"
            output = artifacts / "corpus" / relative
            if not output.is_file() or sha256_file(output) != digest:
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text(text, encoding="utf-8")
            entry = {
                "docid": docid,
                "url": str(row["url"]),
                "path": relative.as_posix(),
                "bytes": len(encoded),
                "sha256": digest,
            }
            manifest.write(json.dumps(entry, ensure_ascii=False) + "\n")
            count += 1
            total_bytes += len(encoded)
            maximum_bytes = max(maximum_bytes, len(encoded))
            aggregate_parts.extend((docid, digest))

    temporary_manifest.replace(manifest_path)
    if count != config.dataset.expected_corpus_documents:
        raise RuntimeError(
            f"expected {config.dataset.expected_corpus_documents} corpus documents, "
            f"found {count}"
        )
    state = {
        "stage": "materialize",
        "generated_at": utc_now(),
        "source_revision": config.dataset.corpus_revision,
        "root": str((artifacts / "corpus").resolve()),
        "documents": str(documents.resolve()),
        "manifest": str(manifest_path.resolve()),
        "count": count,
        "total_bytes": total_bytes,
        "maximum_file_bytes": maximum_bytes,
        "fingerprint": fingerprint(aggregate_parts),
        "content_policy": "Parquet text fields are written byte-for-byte as UTF-8",
    }
    output = artifacts / "state" / "corpus.json"
    write_json(output, state)
    return output
