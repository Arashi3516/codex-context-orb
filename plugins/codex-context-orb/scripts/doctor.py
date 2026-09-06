#!/usr/bin/env python3
"""Inspect one explicit Orb session without writing files or attesting host integration."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

# Imports must not create __pycache__ in an installed plugin directory.
sys.dont_write_bytecode = True
import assessment_store
import emit_hook_event
import evidence_review

MAX_OUTPUT_BYTES = 8_192
MAX_ROOT_BYTES = 2_048
LAYERS = ("hooks", "assessment_v1", "evidence_v2")
UNVERIFIED = {"host_dispatch": "NOT_VERIFIED", "native_ipc": "NOT_VERIFIED", "native_ui": "NOT_VERIFIED"}


def _layer(status: str) -> dict:
    return {"status": status, "count": 0 if status == "ABSENT" else None,
            "report_id": None, "file_sha256": None}


def _identity(argument: str | None, environment: dict) -> dict:
    runtime = environment.get("CODEX_THREAD_ID")
    selected = argument if argument is not None else runtime
    source = "argument" if argument is not None else "CODEX_THREAD_ID" if runtime is not None else "none"
    valid = evidence_review.identifier(selected)
    return {"source": source, "status": "VALID" if valid else "ABSENT" if selected is None else "INVALID",
            "session_id": selected if valid else None,
            "runtime_match": ("MATCH" if selected == runtime else "MISMATCH")
            if valid and evidence_review.identifier(runtime) else "NOT_VERIFIED"}


def _root(argument: str | None, environment: dict) -> tuple[Path | None, str]:
    source = "argument" if argument is not None else "ORB_DATA_DIR" if "ORB_DATA_DIR" in environment else "default"
    value = argument if argument is not None else environment.get("ORB_DATA_DIR")
    path = Path(value) if value is not None else Path.home() / ".codex-context-orb"
    text = str(path)
    private = {".codex", ".ssh", ".aws", ".azure", ".gnupg", "keychains"}
    if (not path.is_absolute() or any(ord(char) < 32 or 127 <= ord(char) <= 159 or 0xD800 <= ord(char) <= 0xDFFF for char in text)
            or len(text.encode("utf-8")) > MAX_ROOT_BYTES
            or evidence_review._forbidden(path.parts) or any(part.lower() in private for part in path.parts)):
        return None, source
    return path, source


def _directory_status(path: Path) -> str:
    try:
        return "PRESENT" if evidence_review._plain_directory(path) else "ABSENT"
    except evidence_review.EvidenceError:
        return "INVALID"
    except OSError:
        return "UNREADABLE"


def _root_status(root: Path) -> str:
    # Refuse linked ancestors as well as a linked root before reading any snapshot.
    for path in reversed((root, *root.parents)):
        status = _directory_status(path)
        if status != "PRESENT":
            return status
    return "PRESENT"


def _read_layer(root: Path, session_id: str, name: str, now_ms: int) -> dict:
    directory_name, maximum = {"hooks": ("events", emit_hook_event.MAX_SNAPSHOT_BYTES),
                               "assessment_v1": ("assessments", assessment_store.MAX_BYTES),
                               "evidence_v2": ("evidence", evidence_review.MAX_BYTES)}[name]
    directory = root / directory_name
    status = _directory_status(directory)
    if status != "PRESENT":
        return _layer(status)
    path = directory / emit_hook_event.session_filename(session_id)
    try:
        raw = evidence_review._read_plain(path, maximum)
        if name == "hooks":
            value = evidence_review.parse_json(raw)
            if not emit_hook_event.valid_snapshot(value) or value["observed_at_ms"] > now_ms + 60_000:
                raise ValueError("Invalid hook snapshot")
        elif name == "assessment_v1":
            value = assessment_store.parse_assessment(raw, now_ms)
        else:
            value = evidence_review.parse_report(raw, now_ms)
        if value["session_id"] != session_id:
            raise ValueError("Session mismatch")
    except FileNotFoundError:
        return _layer("ABSENT")
    except OSError:
        return _layer("UNREADABLE")
    except (ValueError, TypeError, RecursionError):
        return _layer("INVALID")
    result = {"status": "PRESENT", "count": 1, "report_id": value.get("report_id"),
              "file_sha256": hashlib.sha256(raw).hexdigest()}
    if name == "evidence_v2":
        result["checks"] = {state: sum(probe["result"] == state for probe in value["probes"])
                            for state in ("pass", "fail", "unknown")}
    return result


def diagnose(session_id: str | None = None, data_dir: str | None = None,
             *, environment: dict | None = None, now_ms: int | None = None) -> dict:
    environment = os.environ if environment is None else environment
    identity = _identity(session_id, environment)
    root, root_source = _root(data_dir, environment)
    root_status = _root_status(root) if root is not None else "INVALID"
    result = {"schema_version": 1, "root": str(root) if root_status in ("PRESENT", "ABSENT") else None,
              "root_source": root_source, "root_status": root_status, "identity": identity,
              "layers": {name: _layer("NOT_CHECKED") for name in LAYERS}, "verification": dict(UNVERIFIED)}
    if identity["status"] != "VALID":
        return result
    if root_status == "ABSENT":
        result["layers"] = {name: _layer("ABSENT") for name in LAYERS}
    elif root_status == "PRESENT":
        now_ms = time.time_ns() // 1_000_000 if now_ms is None else now_ms
        result["layers"] = {name: _read_layer(root, identity["session_id"], name, now_ms) for name in LAYERS}
    return result


class _Parser(argparse.ArgumentParser):
    def error(self, _message):
        raise ValueError("Invalid doctor arguments")


def main() -> int:
    parser = _Parser(description=__doc__)
    parser.add_argument("--session-id", help="Exact session ID; defaults only to CODEX_THREAD_ID")
    parser.add_argument("--data-dir", help="Absolute Orb data root; defaults to ORB_DATA_DIR or the normal home directory")
    try:
        args = parser.parse_args()
        result = diagnose(args.session_id, args.data_dir)
        code = 0 if result["identity"]["status"] == "VALID" and result["root_status"] in ("PRESENT", "ABSENT") else 2
    except (OSError, ValueError, TypeError, RecursionError):
        result, code = {"schema_version": 1, "error": "INVALID_ARGUMENTS_OR_ROOT", "verification": dict(UNVERIFIED)}, 2
    raw = json.dumps(result, ensure_ascii=False, separators=(",", ":")).encode("utf-8") + b"\n"
    if len(raw) > MAX_OUTPUT_BYTES:
        raw, code = b'{"schema_version":1,"error":"OUTPUT_LIMIT_EXCEEDED"}\n', 2
    sys.stdout.buffer.write(raw)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
