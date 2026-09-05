#!/usr/bin/env python3
"""Read only Orb's snapshots, without selecting a foreground Codex session."""

from __future__ import annotations

import argparse
import itertools
import json
from pathlib import Path
import re
import sys

sys.dont_write_bytecode = True
from emit_hook_event import MAX_SNAPSHOT_BYTES, data_root, identifier, session_filename, valid_snapshot

MAX_DIRECTORY_ENTRIES = 512
MAX_SNAPSHOTS = 128
FILENAME = re.compile(r"[0-9a-f]{64}\.json\Z")


def read_snapshot(path: Path) -> dict | None:
    try:
        if path.is_symlink() or not path.is_file():
            return None
        with path.open("rb") as handle:
            raw = handle.read(MAX_SNAPSHOT_BYTES + 1)
        if len(raw) > MAX_SNAPSHOT_BYTES:
            return None
        value = json.loads(raw)
        if valid_snapshot(value) and path.name == session_filename(value["session_id"]):
            return value
    except (OSError, ValueError, TypeError, RecursionError):
        pass
    return None


def inspect(root: Path, session_id: str | None = None) -> dict:
    result = {"schema_version": 1, "binding": "unbound", "context_status": "unknown",
              "semantic_assessment": "not_implemented", "events": [], "coverage": "complete",
              "invalid_files": 0}
    events = root / "events"
    if events.is_symlink():
        result["coverage"] = "unavailable"
        return result
    try:
        if session_id is not None:
            paths = [events / session_filename(session_id)]
        else:
            paths = list(itertools.islice(events.iterdir(), MAX_DIRECTORY_ENTRIES + 1))
            if len(paths) > MAX_DIRECTORY_ENTRIES:
                result["coverage"] = "partial"
                paths = paths[:MAX_DIRECTORY_ENTRIES]
        candidates = [path for path in paths if FILENAME.fullmatch(path.name)]
        if len(candidates) > MAX_SNAPSHOTS:
            result["coverage"] = "partial"
        for path in candidates[:MAX_SNAPSHOTS]:
            snapshot = read_snapshot(path)
            if snapshot is not None:
                result["events"].append(snapshot)
            elif path.exists():
                result["invalid_files"] += 1
    except FileNotFoundError:
        pass
    except OSError:
        result["coverage"] = "unavailable"
    result["events"].sort(key=lambda value: value["observed_at_ms"], reverse=True)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--session", help="An explicitly selected session id; never inferred from latest activity.")
    args = parser.parse_args()
    if args.session is not None and identifier(args.session) is None:
        parser.error("--session must be a bounded Codex session identifier")
    try:
        result = inspect(data_root(), args.session)
    except ValueError as error:
        parser.error(str(error))
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
