from __future__ import annotations

import os
import signal
import shutil
import subprocess
import tempfile
import threading
import time
from datetime import UTC, datetime
from difflib import get_close_matches
from pathlib import Path
from typing import BinaryIO

from .artifacts import atomic_write_text, read_json, write_json
from .config import BenchmarkConfig
from .corpus import workspace_root
from .models import AttemptResult, Profile, TraceSummary
from .process import resolve_executable
from .profiles import ensure_server
from .trace import parse_trace


INFRASTRUCTURE_PATTERN = (
    "stream disconnected",
    "failed to connect",
    "connection reset",
    "dns",
    "timed out",
    "timeout",
    "rate limit",
    "service unavailable",
)

_ACTIVE_PROCESSES: set[subprocess.Popen[bytes]] = set()
_ACTIVE_PROCESSES_LOCK = threading.Lock()
_CANCEL_REQUESTED = threading.Event()


def _iso_now() -> str:
    return datetime.now(UTC).isoformat()


def _runtime_profile(template: Path, destination: Path) -> None:
    destination.mkdir(parents=True)
    for name in ("config.toml", "AGENTS.md"):
        source = template / name
        if source.is_file():
            shutil.copy2(source, destination / name)
    authentication = template / "auth.json"
    if authentication.exists():
        (destination / "auth.json").symlink_to(authentication.resolve())


def _copy_stream(
    source: BinaryIO, destination: BinaryIO, activity: list[float]
) -> None:
    # BufferedReader.read(n) may wait for all n bytes, hiding short JSONL events
    # and making an active process appear idle. Pipes expose read1(), which
    # returns currently available bytes instead.
    read_available = getattr(source, "read1", source.read)
    while chunk := read_available(64 * 1024):
        destination.write(chunk)
        destination.flush()
        activity[0] = time.monotonic()


def _terminate_group(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def terminate_active_processes() -> None:
    """Stop Codex process groups so an interrupted run can exit and resume."""
    _CANCEL_REQUESTED.set()
    with _ACTIVE_PROCESSES_LOCK:
        processes = tuple(_ACTIVE_PROCESSES)
    for process in processes:
        _terminate_group(process)


def cancellation_requested() -> bool:
    return _CANCEL_REQUESTED.is_set()


def reset_cancellation() -> None:
    _CANCEL_REQUESTED.clear()


def _corpus_access_denied(trace: TraceSummary, stderr: str) -> bool:
    denial = (
        "operation_not_permitted" in stderr.lower()
        or "sandbox violation" in stderr.lower()
    )
    message = trace.last_agent_message.lower()
    inaccessible = any(
        marker in message
        for marker in (
            "cannot access the corpus",
            "can't access the corpus",
            "shell cannot access the corpus",
            "local shell is unavailable under this corpus sandbox",
        )
    )
    return denial and inaccessible


def _classify_attempt(
    *,
    exit_code: int,
    idle_killed: bool,
    interrupted: bool,
    trace: TraceSummary,
    stderr: str,
) -> tuple[str, bool]:
    completed = exit_code == 0 and trace.turn_completed and bool(
        trace.final_response.strip()
    )
    corpus_access_denied = _corpus_access_denied(trace, stderr)
    combined_errors = "\n".join((*trace.errors, stderr)).lower()
    infrastructure_failure = not interrupted and (
        corpus_access_denied
        or (
            not completed
            and (
                idle_killed
                or exit_code < 0
                or any(
                    marker in combined_errors for marker in INFRASTRUCTURE_PATTERN
                )
            )
        )
    )
    if interrupted:
        return "interrupted", False
    if completed and not corpus_access_denied:
        return "completed", False
    if infrastructure_failure:
        return "infrastructure_failed", True
    return "failed", False


def validate_model(artifacts: Path, model: str) -> None:
    """Reject model IDs absent from the authenticated Codex model cache."""
    source_home = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
    cache = source_home / "models_cache.json"
    if not cache.is_file():
        return
    raw_models = read_json(cache).get("models", [])
    models = {
        str(item["slug"]): item
        for item in raw_models
        if isinstance(item, dict) and item.get("slug")
    }
    if not models or model in models:
        return
    visible = sorted(
        slug for slug, item in models.items() if item.get("visibility") == "list"
    )
    suggestions = get_close_matches(model, visible, n=3, cutoff=0.45)
    if model.startswith("gpt-5.6"):
        suggestions = [slug for slug in visible if slug.startswith("gpt-5.6")]
    rendered = ", ".join(suggestions or visible)
    raise RuntimeError(
        f"Codex model {model!r} is not available for the authenticated account. "
        f"Available model IDs include: {rendered}"
    )


def run_attempt(
    config: BenchmarkConfig,
    artifacts: Path,
    *,
    query_id: str,
    prompt: str,
    profile: Profile,
    model: str,
    reasoning_effort: str,
    attempt: int,
    output_dir: Path,
    profiles_root: Path,
    codex_bin: str = "codex",
    zg_bin: str = "zg",
    idle_timeout_seconds: int | None = None,
) -> AttemptResult:
    codex = resolve_executable(codex_bin)
    zg = resolve_executable(zg_bin)
    if codex is None:
        raise RuntimeError(f"Codex executable not found: {codex_bin}")
    if zg is None:
        raise RuntimeError(f"zvec-grep executable not found: {zg_bin}")
    profile_root = profiles_root / profile
    codex_home = profile_root / "codex-home"
    if not codex_home.is_dir():
        raise RuntimeError("run-local Codex profiles are missing")
    if profile == "zvec-grep":
        ensure_server(artifacts, zg_bin=str(zg))

    workspace = workspace_root(artifacts, profile)
    index_dir = workspace_root(artifacts, "zvec-grep") / ".zvec-grep"
    if profile == "zvec-grep" and not index_dir.is_dir():
        raise RuntimeError("zvec-grep index is missing; run 'zg-bench prepare'")

    output_dir.mkdir(parents=True, exist_ok=True)
    events_path = output_dir / "events.jsonl"
    stderr_path = output_dir / "stderr.log"
    final_path = output_dir / "response.md"
    atomic_write_text(output_dir / "prompt.md", prompt)
    server_log = artifacts / "runtime" / "zvec-home" / "daemon" / "logs" / "server.log"
    server_log_offset = (
        server_log.stat().st_size
        if profile == "zvec-grep" and server_log.is_file()
        else 0
    )

    with tempfile.TemporaryDirectory(prefix="zg-bench-") as temporary_name:
        temporary_root = Path(temporary_name)
        runtime_home = temporary_root / "codex-home"
        runtime_final_path = temporary_root / "response.md"
        _runtime_profile(codex_home, runtime_home)

        command = [
            str(codex),
            "exec",
            "--json",
            "--ephemeral",
            "--model",
            model,
            "-c",
            f'model_reasoning_effort="{reasoning_effort}"',
            "-c",
            'web_search="disabled"',
            "-c",
            "allow_login_shell=false",
            "--sandbox",
            "read-only",
            "--ignore-rules",
            "--skip-git-repo-check",
            "-C",
            str(workspace),
            "-o",
            str(runtime_final_path),
            "-",
        ]
        environment = dict(os.environ)
        environment.pop("ZVEC_GREP_HOME", None)
        environment.update(
            {
                "CODEX_HOME": str(runtime_home),
                "HOME": str(runtime_home),
                "NO_COLOR": "1",
                "CODEX_CI": "1",
            }
        )
        if profile == "zvec-grep":
            environment["ZVEC_GREP_HOME"] = str(
                (artifacts / "runtime" / "zvec-home").resolve()
            )
        command_record = {
            "args": command,
            "cwd": str(workspace),
            "profile": profile,
            "model": model,
            "reasoning_effort": reasoning_effort,
            "environment": {
                "CODEX_HOME": environment["CODEX_HOME"],
                "PATH": environment.get("PATH", ""),
                **(
                    {"ZVEC_GREP_HOME": environment["ZVEC_GREP_HOME"]}
                    if profile == "zvec-grep"
                    else {}
                ),
            },
        }
        write_json(output_dir / "command.json", command_record)

        started_at = _iso_now()
        started = time.monotonic()
        idle_killed = False
        activity = [started]
        with events_path.open("wb") as events, stderr_path.open("wb") as errors:
            process = subprocess.Popen(
                command,
                cwd=workspace,
                env=environment,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,
            )
            with _ACTIVE_PROCESSES_LOCK:
                _ACTIVE_PROCESSES.add(process)
            assert process.stdin and process.stdout and process.stderr
            stdout_thread = threading.Thread(
                target=_copy_stream,
                args=(process.stdout, events, activity),
                daemon=True,
            )
            stderr_thread = threading.Thread(
                target=_copy_stream,
                args=(process.stderr, errors, activity),
                daemon=True,
            )
            stdout_thread.start()
            stderr_thread.start()
            try:
                process.stdin.write(prompt.encode("utf-8"))
                process.stdin.close()
                while process.poll() is None:
                    if (
                        idle_timeout_seconds
                        and time.monotonic() - activity[0] > idle_timeout_seconds
                    ):
                        idle_killed = True
                        _terminate_group(process)
                        break
                    time.sleep(1)
                exit_code = process.wait()
                stdout_thread.join(timeout=5)
                stderr_thread.join(timeout=5)
            except BaseException:
                _terminate_group(process)
                raise
            finally:
                with _ACTIVE_PROCESSES_LOCK:
                    _ACTIVE_PROCESSES.discard(process)
        if runtime_final_path.is_file():
            atomic_write_text(
                final_path,
                runtime_final_path.read_text(encoding="utf-8", errors="replace"),
            )

    finished_at = _iso_now()
    wall_seconds = time.monotonic() - started
    trace = parse_trace(events_path, final_path)
    stderr = stderr_path.read_text(encoding="utf-8", errors="replace")
    interrupted = cancellation_requested() and exit_code < 0
    status, infrastructure_failure = _classify_attempt(
        exit_code=exit_code,
        idle_killed=idle_killed,
        interrupted=interrupted,
        trace=trace,
        stderr=stderr,
    )

    server_trace_path = output_dir / "zvec-server.jsonl"
    if profile == "zvec-grep" and server_log.is_file():
        with server_log.open("rb") as source:
            source.seek(min(server_log_offset, server_log.stat().st_size))
            server_trace = source.read().decode("utf-8", errors="replace")
        atomic_write_text(server_trace_path, server_trace)
    result = AttemptResult(
        query_id=query_id,
        profile=profile,
        status=status,
        attempt=attempt,
        started_at=started_at,
        finished_at=finished_at,
        wall_seconds=wall_seconds,
        exit_code=exit_code,
        infrastructure_failure=infrastructure_failure,
        trace=trace,
        interrupted_by="user" if interrupted else None,
        paths={
            "events": str(events_path.resolve()),
            "stderr": str(stderr_path.resolve()),
            "response": str(final_path.resolve()),
            **(
                {"zvec_server": str(server_trace_path.resolve())}
                if profile == "zvec-grep"
                else {}
            ),
        },
    )
    write_json(output_dir / "result.json", result.to_dict())
    write_json(
        output_dir / "usage.json", trace.usage.to_dict() if trace.usage else None
    )
    write_json(output_dir / "tools.json", [call.to_dict() for call in trace.tool_calls])
    write_json(output_dir / "documents.json", list(trace.observed_docids))
    return result
