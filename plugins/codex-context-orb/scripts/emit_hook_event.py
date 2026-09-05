#!/usr/bin/env python3
"""Project Codex hook input onto a small, local, metadata-only snapshot."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time

MAX_INPUT_BYTES = 65_536
MAX_SNAPSHOT_BYTES = 4_096
EVENT_NAMES = frozenset({
    "SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "Stop", "Interrupt"
})
IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,127}\Z")
MODEL = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,95}\Z")
SNAPSHOT_KEYS = frozenset({
    "schema_version", "source", "session_id", "turn_id", "observed_at_ms",
    "last_event_name", "model", "trigger", "context_used_tokens",
    "context_window_tokens", "binding",
})


def identifier(value: object) -> str | None:
    return value if isinstance(value, str) and IDENTIFIER.fullmatch(value) else None


def data_root() -> Path:
    override = os.environ.get("ORB_DATA_DIR")
    root = Path(override) if override else Path.home() / ".codex-context-orb"
    if not root.is_absolute():
        raise ValueError("ORB_DATA_DIR must be an absolute path")
    return root


def session_filename(session_id: str) -> str:
    return hashlib.sha256(session_id.encode("utf-8")).hexdigest() + ".json"


def project_event(payload: object, observed_at_ms: int) -> dict | None:
    if not isinstance(payload, dict):
        return None
    session_id = identifier(payload.get("session_id"))
    event_name = payload.get("hook_event_name")
    if session_id is None or not isinstance(event_name, str) or event_name not in EVENT_NAMES:
        return None
    model = payload.get("model")
    trigger = payload.get("trigger")
    return {
        "schema_version": 1,
        "source": "codex-hook",
        "session_id": session_id,
        "turn_id": identifier(payload.get("turn_id")),
        "observed_at_ms": observed_at_ms,
        "last_event_name": event_name,
        "model": model if isinstance(model, str) and MODEL.fullmatch(model) else None,
        "trigger": trigger if event_name in {"PreCompact", "PostCompact"} and trigger in ("manual", "auto") else None,
        "context_used_tokens": None,
        "context_window_tokens": None,
        "binding": "unbound",
    }


def valid_snapshot(value: object) -> bool:
    if not isinstance(value, dict) or set(value) != SNAPSHOT_KEYS:
        return False
    observed_at = value.get("observed_at_ms")
    if type(observed_at) is not int or not 0 <= observed_at <= 9_007_199_254_740_991:
        return False
    projected = project_event({
        "session_id": value.get("session_id"),
        "turn_id": value.get("turn_id"),
        "hook_event_name": value.get("last_event_name"),
        "model": value.get("model"),
        "trigger": value.get("trigger"),
    }, observed_at)
    return type(value.get("schema_version")) is int and projected == value


def write_snapshot(snapshot: dict, root: Path) -> Path:
    events = root / "events"
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    events.mkdir(mode=0o700, exist_ok=True)
    if events.is_symlink():
        raise ValueError("The events directory must not be a symbolic link")
    content = (json.dumps(snapshot, ensure_ascii=True, separators=(",", ":")) + "\n").encode("utf-8")
    if len(content) > MAX_SNAPSHOT_BYTES or not valid_snapshot(snapshot):
        raise ValueError("Invalid metadata snapshot")
    destination = events / session_filename(snapshot["session_id"])
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", prefix=".orb-", suffix=".tmp", dir=events, delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(content)
        os.replace(temporary, destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return destination


def main() -> None:
    observed_at_ms = time.time_ns() // 1_000_000
    try:
        raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
        if len(raw) <= MAX_INPUT_BYTES:
            payload = json.loads(raw)
            snapshot = project_event(payload, observed_at_ms)
            if snapshot is not None:
                write_snapshot(snapshot, data_root())
    except (OSError, ValueError, TypeError, RecursionError):
        # Never echo incoming data or a traceback into the user's conversation.
        pass
    # No systemMessage/additionalContext/continue field: the hook is advisory.
    sys.stdout.write("{}\n")


if __name__ == "__main__":
    main()
