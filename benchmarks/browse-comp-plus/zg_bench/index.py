from __future__ import annotations

import difflib
import platform
import re
import time
from pathlib import Path

from .artifacts import read_json, utc_now, write_json
from .config import BenchmarkConfig
from .process import (
    inherited_environment,
    resolve_executable,
    run_command,
    run_streaming_command,
)


def build_index(
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    zg_bin: str = "zg",
) -> Path:
    executable = resolve_executable(zg_bin)
    if executable is None:
        raise RuntimeError(f"zvec-grep executable not found: {zg_bin}")
    version = run_command([executable, "version"], timeout=30)
    actual_version = version.stdout.strip().splitlines()[0] if version.stdout else ""
    if not version.ok or actual_version != config.zvec_grep.version:
        raise RuntimeError(
            f"zvec-grep {config.zvec_grep.version} is required; found "
            f"{actual_version or 'unknown'}"
        )
    corpus_state_path = artifacts / "state" / "corpus.json"
    if not corpus_state_path.is_file():
        raise RuntimeError("corpus is not prepared; run 'zg-bench prepare'")
    corpus_state = read_json(corpus_state_path)
    root = Path(corpus_state["root"])
    index_dir = root / ".zvec-grep"

    existing = artifacts / "state" / "index.json"
    if existing.is_file() and index_dir.is_dir():
        state = read_json(existing)
        expected = _index_fingerprint(config, corpus_state)
        if state["fingerprint"] == expected:
            check = run_command(
                [executable, "status", root, "--mode", "direct", "--check-ready"],
                timeout=120,
            )
            if check.ok:
                return existing
        raise RuntimeError(
            "the existing index does not match this benchmark or is not ready; "
            "start with an empty artifacts directory"
        )

    command: list[str | Path] = [
        executable,
        "index",
        root,
        "--mode",
        "direct",
        "--embedding",
        config.zvec_grep.embedding,
        "--embedding-concurrency",
        str(config.zvec_grep.embedding_concurrency),
        "--max-filesize",
        config.zvec_grep.max_filesize,
        "--device",
        config.zvec_grep.device,
        "--glob",
        "documents/**/*.md",
        "--debug",
    ]
    environment = inherited_environment()
    environment["ZVEC_GREP_HOME"] = str((artifacts / "runtime" / "zvec-home").resolve())
    stdout_log = artifacts / "logs" / "index.stdout.log"
    stderr_log = artifacts / "logs" / "index.stderr.log"
    started_at = utc_now()
    started = time.monotonic()
    result = run_streaming_command(
        command,
        cwd=root,
        env=environment,
        stdout_log=stdout_log,
        stderr_log=stderr_log,
    )
    build_wall_seconds = time.monotonic() - started
    if not result.ok:
        raise RuntimeError(
            _index_failure(config, result.stderr, result.stdout, stderr_log)
        )
    status = run_command(
        [executable, "status", root, "--mode", "direct", "--check-ready"],
        cwd=root,
        env=environment,
        timeout=120,
    )
    if not status.ok:
        raise RuntimeError(status.stderr.strip() or status.stdout.strip())

    state = {
        "stage": "index",
        "started_at": started_at,
        "finished_at": utc_now(),
        "root": str(root.resolve()),
        "index": str(index_dir.resolve()),
        "fingerprint": _index_fingerprint(config, corpus_state),
        "platform": {"system": platform.system(), "machine": platform.machine()},
        "command": [str(part) for part in command],
        "build_wall_seconds": build_wall_seconds,
        "index_bytes": _directory_bytes(index_dir),
        "runtime_bytes": _directory_bytes(artifacts / "runtime" / "zvec-home"),
        "status_output": status.stdout,
        "statistics": _parse_status(status.stdout),
        "build_stdout_log": str(stdout_log.resolve()),
        "build_stderr_log": str(stderr_log.resolve()),
    }
    write_json(existing, state)
    return existing


def _index_failure(
    config: BenchmarkConfig,
    stderr: str,
    stdout: str,
    log: Path,
) -> str:
    output = f"{stderr}\n{stdout}"
    reason = re.search(r"^Error:\s*(.+)$", output, re.MULTILINE)
    models = sorted(
        set(
            re.findall(
                r"^\s{2}((?:local|qwen)/\S+?)(?:\s|$)",
                output,
                re.MULTILINE,
            )
        )
    )
    embedding = config.zvec_grep.embedding
    suggestion = difflib.get_close_matches(embedding, models, n=1, cutoff=0.6)
    details = [
        "zvec-grep index build failed",
        f"Reason: {reason.group(1) if reason else 'zvec-grep exited with an error'}",
        f"Config: {config.path}",
        "Setting: [zvec_grep].embedding",
        f"Value: {embedding}",
    ]
    if suggestion:
        details.append(f"Did you mean: {suggestion[0]}")
    details.append(f"Log: {log.resolve()}")
    return "\n".join(details)


def _directory_bytes(root: Path) -> int:
    if not root.is_dir():
        return 0
    return sum(
        path.stat().st_size
        for path in root.rglob("*")
        if path.is_file() and not path.is_symlink()
    )


def _index_fingerprint(
    config: BenchmarkConfig, corpus_state: dict[str, object]
) -> dict[str, object]:
    return {
        "corpus_fingerprint": corpus_state["fingerprint"],
        "embedding": config.zvec_grep.embedding,
        "zvec_grep_version": config.zvec_grep.version,
        "max_filesize": config.zvec_grep.max_filesize,
        "device": config.zvec_grep.device,
        "system": platform.system(),
        "machine": platform.machine(),
    }


def _parse_status(output: str) -> dict[str, int | float | None]:
    coverage = re.search(r"Coverage\s+.*?([\d,]+)\s*/\s*([\d,]+)\s+files", output)
    entities = re.search(r"Entities\s+([\d,]+)", output)
    truncated = re.search(r"Truncated\s+([\d,]+)\s+fragments", output)
    return {
        "indexed_files": int(coverage.group(1).replace(",", "")) if coverage else None,
        "corpus_files": int(coverage.group(2).replace(",", "")) if coverage else None,
        "entities": int(entities.group(1).replace(",", "")) if entities else None,
        "truncated_fragments": int(truncated.group(1).replace(",", ""))
        if truncated
        else None,
    }
