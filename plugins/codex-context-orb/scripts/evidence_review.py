#!/usr/bin/env python3
"""Collect explicit local artifacts into bounded as-of receipts, without model or network calls."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import copy
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

MAX_BYTES = 65_536
MAX_ARTIFACT_BYTES = 1_048_576
MAX_SAFE_INTEGER = 9_007_199_254_740_991
FUTURE_TOLERANCE_MS = 60_000
MAX_HISTORY = 8
MAX_HISTORY_ENTRIES = 64
MAX_HISTORY_OUTPUT = MAX_HISTORY * MAX_BYTES + 1024
LOCK_TIMEOUT_SECONDS = 1.0
RENAME_TIMEOUT_SECONDS = 0.25
IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,127}\Z")
DIGEST = re.compile(r"[0-9a-f]{64}\Z")
MANIFEST_KEYS = frozenset({"schema_version", "session_id", "turn_id", "scope", "sources", "ledger", "probes", "observations"})
REPORT_KEYS = (MANIFEST_KEYS - {"schema_version"}) | {"schema_version", "source", "report_id", "reviewed_at_ms"}
SCOPE_KEYS = frozenset({"mode", "origin", "action_id", "goal_id", "next_step", "coverage", "unknowns"})
SOURCE_KEYS = frozenset({"id", "kind", "ref", "note"})
LEDGER_KEYS = frozenset({"id", "kind", "text", "source_ids", "status", "supersedes", "critical"})
PROBE_KEYS = frozenset({"id", "item_id", "source_id", "rule", "expected"})
OBSERVATION_KEYS = frozenset({"id", "item_id", "kind", "summary", "status", "recurrence", "source_ids"})
OBSERVATION_KINDS = ("goal_drift", "constraint_loss", "decision_conflict", "stale_fact", "repeated_work")


class EvidenceError(ValueError):
    """Content-free failure for the explicitly invoked CLI."""


def identifier(value: object) -> bool:
    return isinstance(value, str) and IDENTIFIER.fullmatch(value) is not None


def digest(value: object) -> bool:
    return isinstance(value, str) and DIGEST.fullmatch(value) is not None


def _object(value: object, keys: object, label: str) -> None:
    if not isinstance(value, dict) or set(value) != set(keys):
        raise EvidenceError(f"{label} has missing or unsupported fields")


def _text(value: object, minimum: int, maximum: int, label: str) -> None:
    if not isinstance(value, str) or not minimum <= len(value) <= maximum:
        raise EvidenceError(f"{label} has an invalid text length")
    if any(ord(char) < 32 or 127 <= ord(char) <= 159 or 0xD800 <= ord(char) <= 0xDFFF for char in value):
        raise EvidenceError(f"{label} contains unsupported control characters")


def _enum(value: object, choices: object, label: str) -> None:
    if not isinstance(value, str) or value not in choices:
        raise EvidenceError(f"{label} has an unsupported value")


def _array(value: object, minimum: int, maximum: int, label: str) -> list:
    if not isinstance(value, list) or not minimum <= len(value) <= maximum:
        raise EvidenceError(f"{label} has an invalid item count")
    return value


def _ids(value: object, minimum: int, maximum: int, label: str) -> list:
    result = _array(value, minimum, maximum, label)
    if any(not identifier(item) for item in result) or len(set(result)) != len(result):
        raise EvidenceError(f"{label} has invalid or duplicate identifiers")
    return result


def _index(values: object, maximum: int, keys: object, label: str) -> dict:
    result = {}
    for value in _array(values, 0, maximum, label):
        _object(value, keys, label)
        if not identifier(value["id"]) or value["id"] in result:
            raise EvidenceError(f"{label} has invalid or duplicate identifiers")
        result[value["id"]] = value
    return result


def artifact_parts(value: object) -> list[str]:
    _text(value, 1, 240, "Artifact reference")
    # A portable relative path: no drives, UNC, ADS, alternate separators, or dot segments.
    parts = value.split("/")
    if "\\" in value or ":" in value or any(part in ("", ".", "..") for part in parts):
        raise EvidenceError("Artifact reference must be a normalized workspace-relative path")
    # Windows ignores trailing spaces/dots and interprets device names specially.
    for part in parts:
        stem = part.split(".", 1)[0].upper()
        if part.endswith((" ", ".")) or stem in {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"} or re.fullmatch(r"(?:COM|LPT)[1-9]", stem):
            raise EvidenceError("Artifact reference contains a nonportable path component")
    return parts


def _core(value: dict, report: bool) -> None:
    if not identifier(value["session_id"]) or (value["turn_id"] is not None and not identifier(value["turn_id"])):
        raise EvidenceError("Invalid session or turn identifier")
    scope = value["scope"]
    _object(scope, SCOPE_KEYS, "Scope")
    if scope["mode"] != "as_of":
        raise EvidenceError("Only as-of evidence is supported")
    _enum(scope["origin"], ("main", "unknown"), "Declared origin")
    _enum(scope["coverage"], ("declared", "partial"), "Declared coverage")
    if not identifier(scope["action_id"]) or not identifier(scope["goal_id"]):
        raise EvidenceError("Invalid action or goal identifier")
    _text(scope["next_step"], 1, 500, "Next step")
    for unknown in _array(scope["unknowns"], 0, 8, "Scope unknowns"):
        _text(unknown, 1, 240, "Scope unknown")
    sources = _index(value["sources"], 16, SOURCE_KEYS | ({"status", "sha256"} if report else set()), "Sources")
    for source in sources.values():
        _enum(source["kind"], ("statement", "artifact"), "Source kind")
        _text(source["ref"], 1, 240, "Source reference")
        _text(source["note"], 1, 240, "Source note")
        if source["kind"] == "artifact":
            artifact_parts(source["ref"])
        if report:
            if source["kind"] == "statement":
                if source["status"] != "attested" or source["sha256"] is not None:
                    raise EvidenceError("Statements must be attested without a file hash")
            elif not ((source["status"] == "captured" and digest(source["sha256"]))
                      or (source["status"] == "unavailable" and source["sha256"] is None)):
                raise EvidenceError("Artifact receipt status and hash do not agree")
            elif source["status"] == "captured" and _forbidden(artifact_parts(source["ref"])):
                raise EvidenceError("Prohibited artifact paths cannot carry captured receipts")
    ledger = _index(value["ledger"], 24, LEDGER_KEYS, "Ledger")
    for item in ledger.values():
        _enum(item["kind"], ("goal", "constraint", "fact", "decision", "progress"), "Ledger kind")
        _enum(item["status"], ("active", "superseded", "hypothesis"), "Ledger status")
        _text(item["text"], 1, 500, "Ledger text")
        if type(item["critical"]) is not bool:
            raise EvidenceError("Critical flag must be a boolean")
        if any(ref not in sources for ref in _ids(item["source_ids"], 1, 4, "Ledger source references")):
            raise EvidenceError("Ledger refers to an unknown source")
        for target in _ids(item["supersedes"], 0, 4, "Supersession references"):
            if target not in ledger or ledger[target]["status"] != "superseded":
                raise EvidenceError("Supersession target must be a retained superseded entry")
    visited, visiting = set(), set()

    def visit(item_id: str) -> None:
        if item_id in visiting:
            raise EvidenceError("Supersession graph contains a cycle")
        if item_id not in visited:
            visiting.add(item_id)
            for target in ledger[item_id]["supersedes"]:
                visit(target)
            visiting.remove(item_id)
            visited.add(item_id)

    for item_id in ledger:
        visit(item_id)
    goal = ledger.get(scope["goal_id"])
    if goal is None or goal["kind"] != "goal" or goal["status"] != "active":
        raise EvidenceError("Scope goal must reference an active goal entry")
    probes = _index(value["probes"], 32, PROBE_KEYS | ({"result", "detail"} if report else set()), "Probes")
    for probe in probes.values():
        item = ledger.get(probe["item_id"]) if identifier(probe["item_id"]) else None
        if item is None or item["status"] != "active":
            raise EvidenceError("Probe must refer to an active ledger entry")
        _enum(probe["rule"], ("contains", "not_contains", "sha256", "manual"), "Probe rule")
        if probe["rule"] == "manual":
            if probe["source_id"] is not None or probe["expected"] != "":
                raise EvidenceError("Manual probes require null source and empty expected text")
        else:
            source = sources.get(probe["source_id"]) if identifier(probe["source_id"]) else None
            if source is None or source["kind"] != "artifact":
                raise EvidenceError("Non-manual probe must reference an artifact source")
            if probe["rule"] == "sha256":
                if not digest(probe["expected"]):
                    raise EvidenceError("SHA-256 check requires a lower-case digest")
            else:
                _text(probe["expected"], 1, 240, "Literal expectation")
        if report:
            _enum(probe["result"], ("pass", "fail", "unknown"), "Probe result")
            _text(probe["detail"], 1, 240, "Probe explanation")
            if probe["rule"] == "manual" or sources[probe["source_id"]]["status"] == "unavailable":
                if probe["result"] != "unknown":
                    raise EvidenceError("Manual or unavailable checks must remain unknown")
            elif probe["result"] not in ("pass", "fail"):
                raise EvidenceError("Captured artifact checks must have a pass or fail receipt")
            elif probe["rule"] == "sha256":
                expected_result = "pass" if probe["expected"] == sources[probe["source_id"]]["sha256"] else "fail"
                if probe["result"] != expected_result:
                    raise EvidenceError("SHA-256 result contradicts the source receipt")
    observations = _index(value["observations"], 8, OBSERVATION_KEYS, "Observations")
    for observation in observations.values():
        if not identifier(observation["item_id"]) or observation["item_id"] not in ledger:
            raise EvidenceError("Observation refers to an unknown ledger entry")
        _enum(observation["kind"], OBSERVATION_KINDS, "Observation kind")
        _enum(observation["status"], ("open", "resolved"), "Observation status")
        _enum(observation["recurrence"], ("once", "after_correction"), "Observation recurrence")
        _text(observation["summary"], 1, 240, "Observation summary")
        if any(ref not in sources for ref in _ids(observation["source_ids"], 1, 4, "Observation source references")):
            raise EvidenceError("Observation refers to an unknown source")


def canonical_bytes(value: object) -> bytes:
    try:
        return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")
    except (ValueError, UnicodeError, RecursionError) as error:
        raise EvidenceError("Evidence is not canonical UTF-8 JSON") from error


def report_hash(value: dict) -> str:
    return hashlib.sha256(canonical_bytes({key: item for key, item in value.items() if key != "report_id"})).hexdigest()


def validate_manifest(value: object) -> dict:
    _object(value, MANIFEST_KEYS, "Manifest")
    if type(value["schema_version"]) is not int or value["schema_version"] != 1:
        raise EvidenceError("Unsupported manifest version")
    _core(value, report=False)
    if len(canonical_bytes(value)) > MAX_BYTES:
        raise EvidenceError("Manifest exceeds the 64 KiB limit")
    return value


def validate_report(value: object, now_ms: int | None = None) -> dict:
    _object(value, REPORT_KEYS, "Report")
    if type(value["schema_version"]) is not int or value["schema_version"] != 2 or value["source"] != "codex-evidence-review":
        raise EvidenceError("Unsupported evidence report version or source")
    reviewed = value["reviewed_at_ms"]
    now_ms = time.time_ns() // 1_000_000 if now_ms is None else now_ms
    if type(reviewed) is not int or not 0 <= reviewed <= MAX_SAFE_INTEGER or reviewed > now_ms + FUTURE_TOLERANCE_MS:
        raise EvidenceError("Evidence review time is invalid or too far in the future")
    _core(value, report=True)
    if not digest(value["report_id"]) or report_hash(value) != value["report_id"]:
        raise EvidenceError("Evidence report hash does not match its contents")
    if len(canonical_bytes(value)) > MAX_BYTES:
        raise EvidenceError("Evidence report exceeds the 64 KiB limit")
    return value


def _unique_object(pairs: list) -> dict:
    value = {}
    for key, item in pairs:
        if key in value:
            raise EvidenceError("JSON contains duplicate fields")
        value[key] = item
    return value


def _invalid_number(_value: str) -> None:
    raise EvidenceError("JSON must not contain floats or non-finite numbers")


def parse_json(raw: bytes) -> object:
    if len(raw) > MAX_BYTES:
        raise EvidenceError("Input exceeds the 64 KiB limit")
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=_unique_object,
                          parse_float=_invalid_number, parse_constant=_invalid_number)
    except EvidenceError:
        raise
    except (ValueError, UnicodeError, RecursionError) as error:
        raise EvidenceError("Input is not valid UTF-8 JSON") from error


def parse_report(raw: bytes, now_ms: int | None = None) -> dict:
    return validate_report(parse_json(raw), now_ms)


def _is_link(metadata: os.stat_result) -> bool:
    return stat.S_ISLNK(metadata.st_mode) or bool(getattr(metadata, "st_file_attributes", 0) & 0x400)


def _plain_directory(path: Path, create: bool = False) -> bool:
    if create:
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return False
    if not stat.S_ISDIR(metadata.st_mode) or _is_link(metadata):
        raise EvidenceError("Evidence directory must be regular, not a link or reparse point")
    return True


def _signature(metadata: os.stat_result) -> tuple:
    return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns)


def _open_plain(path: Path, flags: int, mode: int = 0o600) -> int:
    try:
        before = path.lstat()
    except FileNotFoundError:
        before = None
    if before is not None and (not stat.S_ISREG(before.st_mode) or _is_link(before)):
        raise EvidenceError("Evidence file must be regular, not a link or device")
    descriptor = os.open(path, flags | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_BINARY", 0), mode)
    after = os.fstat(descriptor)
    if not stat.S_ISREG(after.st_mode) or _is_link(after) or (before is not None and (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino)):
        os.close(descriptor)
        raise EvidenceError("Evidence file changed while opening")
    return descriptor


def _read_plain(path: Path, maximum: int) -> bytes:
    descriptor = _open_plain(path, os.O_RDONLY)
    with os.fdopen(descriptor, "rb") as handle:
        before = os.fstat(handle.fileno())
        if before.st_size > maximum:
            raise EvidenceError("File exceeds its byte limit")
        raw = handle.read(maximum + 1)
        after = os.fstat(handle.fileno())
        current = path.lstat()
        if len(raw) > maximum or _is_link(current) or _signature(before) != _signature(after) or _signature(after) != _signature(current):
            raise EvidenceError("File changed during the bounded read")
    return raw


def _forbidden(parts: object) -> bool:
    return any(part.lower() == ".codex" or part.lower() == ".env" or part.lower().startswith(".env.") for part in parts)


def _workspace(path: Path) -> None:
    if not path.is_absolute() or _forbidden(path.parts):
        raise EvidenceError("An explicit non-private absolute workspace is required")
    for component in reversed((path, *path.parents)):
        if not _plain_directory(component):
            raise EvidenceError("Workspace directory is unavailable")


def _capture(workspace: Path, parts: list[str]) -> bytes | None:
    if _forbidden(parts):
        return None
    try:
        if os.name != "nt" and os.open in os.supports_dir_fd:
            # Anchor every lookup to an open directory. A renamed parent cannot redirect reads.
            descriptors = [os.open(workspace.anchor, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)]
            directory_parts = [*workspace.parts[1:], *parts[:-1]]
            try:
                for part in directory_parts:
                    descriptors.append(os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=descriptors[-1]))
                before = os.stat(parts[-1], dir_fd=descriptors[-1], follow_symlinks=False)
                if _is_link(before) or not stat.S_ISREG(before.st_mode) or before.st_size > MAX_ARTIFACT_BYTES:
                    return None
                descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=descriptors[-1])
                with os.fdopen(descriptor, "rb") as handle:
                    opened = os.fstat(handle.fileno())
                    if _signature(before) != _signature(opened) or not stat.S_ISREG(opened.st_mode):
                        return None
                    raw = handle.read(MAX_ARTIFACT_BYTES + 1)
                    after = os.fstat(handle.fileno())
                    current = os.stat(parts[-1], dir_fd=descriptors[-1], follow_symlinks=False)
                    if len(raw) > MAX_ARTIFACT_BYTES or _signature(opened) != _signature(after) or _signature(after) != _signature(current) or _is_link(current):
                        return None
                    for index, part in enumerate(directory_parts):
                        current_directory = os.stat(part, dir_fd=descriptors[index], follow_symlinks=False)
                        opened_directory = os.fstat(descriptors[index + 1])
                        if _is_link(current_directory) or (current_directory.st_dev, current_directory.st_ino) != (opened_directory.st_dev, opened_directory.st_ino):
                            return None
            finally:
                for descriptor in reversed(descriptors):
                    os.close(descriptor)
        else:
            # Reparse-point and identity checks also cover Windows junctions.
            current = workspace
            parents = []
            for part in parts[:-1]:
                current /= part
                if not _plain_directory(current):
                    return None
                parents.append((current, _signature(current.lstat())))
            raw = _read_plain(workspace.joinpath(*parts), MAX_ARTIFACT_BYTES)
            if any(_is_link(path.lstat()) or _signature(path.lstat()) != original for path, original in parents):
                return None
        raw.decode("utf-8")
        return raw
    except (OSError, EvidenceError, UnicodeError):
        return None


def collect(manifest: object, workspace: Path, now_ms: int | None = None) -> dict:
    validate_manifest(manifest)
    _workspace(workspace)
    report = copy.deepcopy(manifest)
    captures = {}
    for source in report["sources"]:
        if source["kind"] == "statement":
            source.update(status="attested", sha256=None)
        else:
            raw = _capture(workspace, artifact_parts(source["ref"]))
            captures[source["id"]] = raw
            source.update(status="unavailable" if raw is None else "captured",
                          sha256=None if raw is None else hashlib.sha256(raw).hexdigest())
    for probe in report["probes"]:
        raw = captures.get(probe["source_id"])
        if probe["rule"] == "manual":
            result, detail = "unknown", "Manual declaration; no artifact check was performed."
        elif raw is None:
            result, detail = "unknown", "Artifact is unavailable, prohibited, non-UTF-8, oversized, or changed during capture."
        elif probe["rule"] == "sha256":
            matches = hashlib.sha256(raw).hexdigest() == probe["expected"]
            result, detail = ("pass" if matches else "fail"), "Compared SHA-256 with the exact captured bytes."
        else:
            present = probe["expected"] in raw.decode("utf-8")
            matches = present if probe["rule"] == "contains" else not present
            result = "pass" if matches else "fail"
            detail = "Literal substring is present in the captured UTF-8 bytes." if present else "Literal substring is absent from the captured UTF-8 bytes."
        probe.update(result=result, detail=detail)
    report.update(schema_version=2, source="codex-evidence-review", reviewed_at_ms=time.time_ns() // 1_000_000 if now_ms is None else now_ms)
    report["report_id"] = report_hash(report)
    return validate_report(report, now_ms)


def data_root() -> Path:
    override = os.environ.get("ORB_DATA_DIR")
    root = Path(override) if override is not None else Path.home() / ".codex-context-orb"
    if not root.is_absolute():
        raise EvidenceError("ORB_DATA_DIR must be an absolute path")
    return root


def session_hash(session_id: str) -> str:
    if not identifier(session_id):
        raise EvidenceError("An exact bounded session identifier is required")
    return hashlib.sha256(session_id.encode("utf-8")).hexdigest()


def _directory(root: Path, create: bool = False) -> Path | None:
    if not root.is_absolute():
        raise EvidenceError("Evidence data root must be absolute")
    if not _plain_directory(root, create):
        return None
    directory = root / "evidence"
    return directory if _plain_directory(directory, create) else None


def read_report(root: Path, session_id: str, now_ms: int | None = None) -> dict | None:
    filename = session_hash(session_id) + ".json"
    directory = _directory(root)
    if directory is None:
        return None
    try:
        raw = _read_plain(directory / filename, MAX_BYTES)
    except FileNotFoundError:
        return None
    report = parse_report(raw, now_ms)
    if report["session_id"] != session_id:
        raise EvidenceError("Evidence does not match the requested session")
    return report


def _history_directory(directory: Path, session_id: str, create: bool = False) -> Path | None:
    parent = directory / "history"
    if not _plain_directory(parent, create):
        return None
    path = parent / session_hash(session_id)
    return path if _plain_directory(path, create) else None


def _history_reports(directory: Path, session_id: str, now_ms: int | None) -> list[dict]:
    reports = []
    with os.scandir(directory) as entries:
        for index, entry in enumerate(entries):
            if index >= MAX_HISTORY_ENTRIES:
                raise EvidenceError("Evidence history exceeds the bounded inventory; explicit cleanup is required")
            name = entry.name.removesuffix(".json")
            if not entry.name.endswith(".json") or not digest(name):
                continue
            report = parse_report(_read_plain(Path(entry.path), MAX_BYTES), now_ms)
            if report["session_id"] != session_id or report["report_id"] != name:
                raise EvidenceError("History filename, hash or session does not match")
            reports.append(report)
    return reports


def read_history(root: Path, session_id: str, now_ms: int | None = None) -> list[dict]:
    session_hash(session_id)
    directory = _directory(root)
    if directory is None:
        return []
    latest = read_report(root, session_id, now_ms)
    history = _history_directory(directory, session_id)
    reports = [] if history is None else _history_reports(history, session_id, now_ms)
    if latest is not None:
        reports.append(latest)
    unique = {report["report_id"]: report for report in reports}
    return sorted(unique.values(), key=lambda report: (report["reviewed_at_ms"], report["report_id"]), reverse=True)[:MAX_HISTORY]


@contextmanager
def _session_lock(directory: Path, session_id: str, timeout: float):
    locks = directory / ".locks"
    _plain_directory(locks, create=True)
    descriptor = _open_plain(locks / (session_hash(session_id) + ".lock"), os.O_RDWR | os.O_CREAT)
    locked = False
    try:
        deadline = time.monotonic() + max(0.0, timeout)
        while True:
            try:
                if os.name == "nt":
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
                    raise EvidenceError("Evidence is busy; no report was written") from error
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


def _retry_share(operation, *args):
    deadline = time.monotonic() + RENAME_TIMEOUT_SECONDS
    while True:
        try:
            return operation(*args)
        except OSError as error:
            if getattr(error, "winerror", None) not in (5, 32, 33) or time.monotonic() >= deadline:
                raise
            time.sleep(0.01)


def _atomic(path: Path, raw: bytes, immutable: bool = False) -> None:
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", prefix=".evidence-", suffix=".tmp", dir=path.parent, delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
        if immutable:
            if os.name == "nt":
                _retry_share(os.rename, temporary, path)  # Windows rename never overwrites an existing path.
            else:
                os.link(temporary, path)  # Atomic no-clobber publication of the complete file.
        else:
            _retry_share(os.replace, temporary, path)
    finally:
        if temporary is not None:
            try:
                _retry_share(os.unlink, temporary)
            except FileNotFoundError:
                pass


def _archive(directory: Path, report: dict, now_ms: int | None) -> None:
    path = directory / (report["report_id"] + ".json")
    try:
        existing = parse_report(_read_plain(path, MAX_BYTES), now_ms)
    except FileNotFoundError:
        _atomic(path, canonical_bytes(report), immutable=True)
        return
    if existing != report:
        raise EvidenceError("Immutable history conflicts with the report; nothing was overwritten")


def write_report(report: object, root: Path, now_ms: int | None = None,
                 lock_timeout: float = LOCK_TIMEOUT_SECONDS) -> Path:
    validate_report(report, now_ms)
    session_id = report["session_id"]
    directory = _directory(root, create=True)
    destination = directory / (session_hash(session_id) + ".json")
    with _session_lock(directory, session_id, lock_timeout):
        previous = read_report(root, session_id, now_ms)
        if previous is not None:
            if previous["reviewed_at_ms"] > report["reviewed_at_ms"]:
                raise EvidenceError("A newer evidence report already exists")
            if previous["reviewed_at_ms"] == report["reviewed_at_ms"] and previous != report:
                raise EvidenceError("A different report already uses this review time")
        history = _history_directory(directory, session_id, create=True)
        retained = _history_reports(history, session_id, now_ms)
        if previous is not None:
            _archive(history, previous, now_ms)
        # Latest is the commit point. Prior committed reports are archived before replacement.
        if previous != report:
            _atomic(destination, canonical_bytes(report))
        try:
            _archive(history, report, now_ms)
            retained.extend([report] + ([] if previous is None else [previous]))
            ordered = sorted({item["report_id"]: item for item in retained}.values(),
                             key=lambda item: (item["reviewed_at_ms"], item["report_id"]), reverse=True)
            for expired in ordered[MAX_HISTORY:]:
                _retry_share(os.unlink, history / (expired["report_id"] + ".json"))
        except (OSError, EvidenceError) as error:
            raise EvidenceError("Latest evidence was saved, but history maintenance failed; read latest before retrying") from error
    return destination


def _input(path: str) -> bytes:
    if path == "-":
        return sys.stdin.buffer.read(MAX_BYTES + 1)
    candidate = Path(path).absolute()
    if _forbidden(candidate.parts):
        raise EvidenceError("Private transcript, account and environment paths cannot be inputs")
    _workspace(candidate.parent)
    return _read_plain(candidate, MAX_BYTES)


def _output(value: object, maximum: int = MAX_BYTES) -> None:
    raw = canonical_bytes(value)
    if len(raw) > maximum:
        raise EvidenceError("Output exceeds its byte limit")
    sys.stdout.buffer.write(raw + (b"\n" if len(raw) < maximum else b""))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    collect_parser = sub.add_parser("collect", help="Collect only explicitly listed local artifacts")
    collect_parser.add_argument("--input", required=True)
    collect_parser.add_argument("--workspace", required=True)
    validate_parser = sub.add_parser("validate", help="Validate a saved as-of receipt without reading artifacts")
    validate_parser.add_argument("--input", required=True)
    for action in ("read", "history"):
        command = sub.add_parser(action)
        command.add_argument("--session-id", required=True)
    args = parser.parse_args()
    try:
        if args.action == "collect":
            report = collect(parse_json(_input(args.input)), Path(args.workspace))
            write_report(report, data_root())
            _output(report)
        elif args.action == "validate":
            report = parse_report(_input(args.input))
            _output({"valid": True, "report_id": report["report_id"]})
        elif args.action == "read":
            _output(read_report(data_root(), args.session_id))
        else:
            _output(read_history(data_root(), args.session_id), MAX_HISTORY_OUTPUT)
        return 0
    except (EvidenceError, OSError) as error:
        message = str(error) if isinstance(error, EvidenceError) else "Local evidence I/O failed; no file contents were returned"
        print(message, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
