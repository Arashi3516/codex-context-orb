#!/usr/bin/env python3
"""Validate and store explicitly requested local reviews; never read a transcript."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile
import time

if os.name == "nt":
    import msvcrt
else:
    import fcntl

MAX_BYTES = 32_768
MAX_SAFE_INTEGER = 9_007_199_254_740_991
FUTURE_TOLERANCE_MS = 60_000
LOCK_TIMEOUT_SECONDS = 1.0
IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,127}\Z")
ROOT_KEYS = frozenset({"schema_version", "source", "session_id", "turn_id", "reviewed_at_ms",
                       "compactions_observed", "coverage", "current_goal", "next_step", "review_note", "signals"})
SIGNAL_KEYS = frozenset({"id", "kind", "summary", "status", "after_compaction", "affects_next_step",
                         "recurrence", "confidence", "evidence"})
EVIDENCE_KEYS = frozenset({"ref", "note"})
SIGNAL_KINDS = frozenset({"goal_drift", "constraint_loss", "decision_conflict", "stale_fact", "repeated_work"})


class AssessmentError(ValueError):
    """A safe, content-free error suitable for explicit CLI feedback."""


def identifier(value: object) -> bool:
    return isinstance(value, str) and IDENTIFIER.fullmatch(value) is not None


def _object(value: object, keys: frozenset, label: str) -> None:
    if not isinstance(value, dict) or set(value) != keys:
        raise AssessmentError(f"{label} has missing or unsupported fields")


def _text(value: object, minimum: int, maximum: int, label: str, multiline: bool = False) -> None:
    if not isinstance(value, str) or not minimum <= len(value) <= maximum:
        raise AssessmentError(f"{label} has an invalid text length")
    if any((ord(char) < 32 or 127 <= ord(char) <= 159 or 0xD800 <= ord(char) <= 0xDFFF)
           and not (multiline and char == "\n") for char in value):
        raise AssessmentError(f"{label} contains unsupported control characters")


def _enum(value: object, choices: object, label: str) -> None:
    if not isinstance(value, str) or value not in choices:
        raise AssessmentError(f"{label} has an unsupported value")


def validate_assessment(value: object, now_ms: int | None = None) -> dict:
    _object(value, ROOT_KEYS, "Assessment")
    if type(value["schema_version"]) is not int or value["schema_version"] != 1 or value["source"] != "codex-skill-review":
        raise AssessmentError("Unsupported assessment version or source")
    if not identifier(value["session_id"]) or (value["turn_id"] is not None and not identifier(value["turn_id"])):
        raise AssessmentError("Assessment has an invalid session or turn identifier")
    reviewed_at = value["reviewed_at_ms"]
    now_ms = time.time_ns() // 1_000_000 if now_ms is None else now_ms
    if type(reviewed_at) is not int or not 0 <= reviewed_at <= MAX_SAFE_INTEGER or reviewed_at > now_ms + FUTURE_TOLERANCE_MS:
        raise AssessmentError("Assessment review time is invalid or too far in the future")
    compactions = value["compactions_observed"]
    if compactions is not None and (type(compactions) is not int or not 0 <= compactions <= 10_000):
        raise AssessmentError("Assessment compaction observation is invalid")
    _enum(value["coverage"], ("sufficient", "partial"), "Coverage")
    _text(value["current_goal"], 1, 500, "Current goal")
    _text(value["next_step"], 1, 500, "Next step")
    _text(value["review_note"], 0, 1000, "Review note", multiline=True)
    signals = value["signals"]
    if not isinstance(signals, list) or len(signals) > 8:
        raise AssessmentError("Assessment must contain at most eight signals")
    for signal in signals:
        _object(signal, SIGNAL_KEYS, "Signal")
        if not identifier(signal["id"]):
            raise AssessmentError("Signal identifier is invalid")
        _enum(signal["kind"], SIGNAL_KINDS, "Signal kind")
        _text(signal["summary"], 1, 240, "Signal summary")
        _enum(signal["status"], ("open", "resolved"), "Signal status")
        if type(signal["after_compaction"]) is not bool or type(signal["affects_next_step"]) is not bool:
            raise AssessmentError("Signal impact flags must be booleans")
        _enum(signal["recurrence"], ("once", "after_correction"), "Signal recurrence")
        _enum(signal["confidence"], ("low", "medium", "high"), "Signal confidence")
        evidence = signal["evidence"]
        if not isinstance(evidence, list) or not 1 <= len(evidence) <= 4:
            raise AssessmentError("Signal must contain one to four evidence references")
        for item in evidence:
            _object(item, EVIDENCE_KEYS, "Evidence")
            _text(item["ref"], 1, 160, "Evidence reference")
            _text(item["note"], 1, 500, "Evidence note")
    return value


def _unique_object(pairs: list) -> dict:
    value = {}
    for key, item in pairs:
        if key in value:
            raise AssessmentError("JSON contains duplicate fields")
        value[key] = item
    return value


def _invalid_constant(_value: str) -> None:
    raise AssessmentError("JSON contains a non-finite number")


def parse_assessment(raw: bytes, now_ms: int | None = None) -> dict:
    if len(raw) > MAX_BYTES:
        raise AssessmentError("Assessment exceeds the 32 KiB limit")
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_unique_object, parse_constant=_invalid_constant)
    except AssessmentError:
        raise
    except (UnicodeError, ValueError, RecursionError) as error:
        raise AssessmentError("Assessment is not valid UTF-8 JSON") from error
    return validate_assessment(value, now_ms)


def data_root() -> Path:
    override = os.environ.get("ORB_DATA_DIR")
    root = Path(override) if override is not None else Path.home() / ".codex-context-orb"
    if not root.is_absolute():
        raise AssessmentError("ORB_DATA_DIR must be an absolute path")
    return root


def assessment_filename(session_id: str) -> str:
    if not identifier(session_id):
        raise AssessmentError("An exact bounded session identifier is required")
    return hashlib.sha256(session_id.encode("utf-8")).hexdigest() + ".json"


def _plain_directory(path: Path, create: bool = False) -> bool:
    if create:
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return False
    if not stat.S_ISDIR(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode):
        raise AssessmentError("Assessment data directory must be a regular directory, not a link")
    return True


def _directory(root: Path, create: bool = False) -> Path | None:
    if not root.is_absolute():
        raise AssessmentError("Assessment data root must be absolute")
    if not _plain_directory(root, create):
        return None
    directory = root / "assessments"
    return directory if _plain_directory(directory, create) else None


def _open_plain(path: Path, flags: int, mode: int = 0o600) -> int:
    try:
        before = path.lstat()
    except FileNotFoundError:
        before = None
    if before is not None and not stat.S_ISREG(before.st_mode):
        raise AssessmentError("Assessment file must be regular, not a link or device")
    descriptor = os.open(path, flags | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0), mode)
    after = os.fstat(descriptor)
    if not stat.S_ISREG(after.st_mode) or (before is not None and (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino)):
        os.close(descriptor)
        raise AssessmentError("Assessment file changed while opening")
    return descriptor


def read_assessment(root: Path, session_id: str, now_ms: int | None = None) -> dict | None:
    filename = assessment_filename(session_id)
    directory = _directory(root)
    if directory is None:
        return None
    path = directory / filename
    try:
        descriptor = _open_plain(path, os.O_RDONLY)
    except FileNotFoundError:
        return None
    with os.fdopen(descriptor, "rb") as handle:
        raw = handle.read(MAX_BYTES + 1)
    value = parse_assessment(raw, now_ms)
    if value["session_id"] != session_id:
        raise AssessmentError("Assessment file does not match the requested session")
    return value


@contextmanager
def _session_lock(directory: Path, session_id: str, timeout: float):
    locks = directory / ".locks"
    _plain_directory(locks, create=True)
    path = locks / (assessment_filename(session_id) + ".lock")
    descriptor = _open_plain(path, os.O_RDWR | os.O_CREAT)
    locked = False
    try:
        deadline = time.monotonic() + max(0.0, timeout)
        while True:
            try:
                if os.name == "nt":
                    # Windows byte-range locks may extend beyond EOF; no initialization write is needed.
                    os.lseek(descriptor, 0, os.SEEK_SET)
                    msvcrt.locking(descriptor, msvcrt.LK_NBLCK, 1)
                else:
                    fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                locked = True
                break
            except OSError as error:
                if error.errno not in (errno.EACCES, errno.EAGAIN, errno.EDEADLK):
                    raise
                if time.monotonic() >= deadline:
                    raise AssessmentError("Assessment is busy; no report was written") from error
                time.sleep(0.01)
        yield
    finally:
        if locked:
            if os.name == "nt":
                os.lseek(descriptor, 0, os.SEEK_SET)
                msvcrt.locking(descriptor, msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(descriptor, fcntl.LOCK_UN)
        os.close(descriptor)
        # Keep the lock inode stable. OS locks release automatically on process exit.


def write_assessment(value: object, root: Path, session_id: str, now_ms: int | None = None,
                     lock_timeout: float = LOCK_TIMEOUT_SECONDS) -> Path:
    filename = assessment_filename(session_id)
    report = validate_assessment(value, now_ms)
    if report["session_id"] != session_id:
        raise AssessmentError("Assessment does not match the explicitly requested session")
    content = (json.dumps(report, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")
    if len(content) > MAX_BYTES:
        raise AssessmentError("Assessment exceeds the 32 KiB limit")
    directory = _directory(root, create=True)
    destination = directory / filename
    with _session_lock(directory, session_id, lock_timeout):
        previous = read_assessment(root, session_id, now_ms)
        if previous is not None:
            if previous["reviewed_at_ms"] > report["reviewed_at_ms"]:
                raise AssessmentError("A newer assessment already exists; no report was written")
            if previous["reviewed_at_ms"] == report["reviewed_at_ms"]:
                if previous == report:
                    return destination
                raise AssessmentError("A different assessment already uses this review time")
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode="wb", prefix=".assessment-", suffix=".tmp", dir=directory, delete=False) as handle:
                temporary = Path(handle.name)
                handle.write(content)
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, destination)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("write", "read"))
    parser.add_argument("--session", required=True, help="Explicit exact session identifier")
    args = parser.parse_args()
    try:
        if args.action == "write":
            value = parse_assessment(sys.stdin.buffer.read(MAX_BYTES + 1))
            path = write_assessment(value, data_root(), args.session)
            print(json.dumps({"session_id": args.session, "report_path": str(path)}))
        else:
            # A redirected Windows stdout can use a legacy code page. The CLI
            # contract is UTF-8 regardless of the caller's console encoding.
            output = json.dumps(read_assessment(data_root(), args.session), ensure_ascii=False) + "\n"
            sys.stdout.buffer.write(output.encode("utf-8"))
        return 0
    except (AssessmentError, OSError, RecursionError) as error:
        # Keep report prose, filesystem details and tracebacks out of error output.
        message = str(error) if isinstance(error, AssessmentError) else "Local assessment storage is unavailable"
        print(f"Assessment store: {message}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
