"""Synthetic-only contract, identity, concurrency and failure tests for reviews."""

import concurrent.futures
import copy
import errno
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import assessment_store as store

FIXTURE = json.loads((Path(__file__).parent / "fixtures" / "assessment-valid.json").read_text(encoding="utf-8"))
NOW = 1_000_000


class AssessmentStoreTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="orb-assessment-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.session_id = "synthetic-session-a"

    def report(self, timestamp=NOW, session_id=None):
        value = copy.deepcopy(FIXTURE)
        value["session_id"] = self.session_id if session_id is None else session_id
        value["reviewed_at_ms"] = timestamp
        return value

    def write(self, value):
        return store.write_assessment(value, self.root, self.session_id, NOW)

    def run_cli(self, action, value=None, session_id=None):
        raw = json.dumps(value, ensure_ascii=False) if isinstance(value, dict) else value
        return subprocess.run([sys.executable, str(Path(store.__file__)), action, "--session", session_id or self.session_id],
                              input=raw, text=True, encoding="utf-8", capture_output=True, timeout=5,
                              env={**os.environ, "ORB_DATA_DIR": str(self.root)})

    def test_shared_fixture_roundtrip_and_nullable_fields(self):
        store.validate_assessment(FIXTURE, FIXTURE["reviewed_at_ms"])
        value = self.report()
        value.update(turn_id=None, compactions_observed=None, signals=[])
        path = self.write(value)
        self.assertEqual(path.parent, self.root / "assessments")
        self.assertEqual(path.name, store.assessment_filename(self.session_id))
        self.assertEqual(store.read_assessment(self.root, self.session_id, NOW), value)
        if os.name != "nt":
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_cli_read_is_utf8_under_legacy_console_encoding(self):
        value = self.report()
        self.write(value)
        result = subprocess.run([sys.executable, str(Path(store.__file__)), "read", "--session", self.session_id],
                                capture_output=True, timeout=5,
                                env={**os.environ, "ORB_DATA_DIR": str(self.root), "PYTHONIOENCODING": "ascii"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout.decode("utf-8")), value)

    def test_rejects_missing_extra_and_wrongly_typed_fields(self):
        for changes in [{"schema_version": True}, {"reviewed_at_ms": True}, {"reviewed_at_ms": 1.0},
                        {"reviewed_at_ms": store.MAX_SAFE_INTEGER + 1}, {"compactions_observed": True},
                        {"compactions_observed": 10_001}, {"source": "automatic-api"},
                        {"coverage": "authoritative"}, {"entropy_score": 0.9}, {"session_id": "../escape"}]:
            with self.subTest(changes=tuple(changes)):
                with self.assertRaises(store.AssessmentError):
                    store.validate_assessment({**self.report(), **changes}, NOW)
        for field in store.ROOT_KEYS:
            value = self.report()
            del value[field]
            with self.subTest(missing=field), self.assertRaises(store.AssessmentError):
                store.validate_assessment(value, NOW)
        for level in ("signal", "evidence"):
            value = self.report()
            target = value["signals"][0] if level == "signal" else value["signals"][0]["evidence"][0]
            target["unsupported"] = "synthetic"
            with self.subTest(level=level), self.assertRaises(store.AssessmentError):
                store.validate_assessment(value, NOW)

    def test_rejects_signal_enums_impact_types_and_collection_limits(self):
        for field, invalid in [("kind", "entropy"), ("status", "confirmed"), ("after_compaction", 1),
                               ("affects_next_step", 0), ("recurrence", "repeated"), ("confidence", "certain"),
                               ("evidence", [])]:
            value = self.report()
            value["signals"][0][field] = invalid
            with self.subTest(field=field), self.assertRaises(store.AssessmentError):
                store.validate_assessment(value, NOW)
        value = self.report()
        value["signals"] *= 9
        with self.assertRaises(store.AssessmentError):
            store.validate_assessment(value, NOW)
        value = self.report()
        value["signals"][0]["evidence"] *= 3
        with self.assertRaises(store.AssessmentError):
            store.validate_assessment(value, NOW)

    def test_unicode_lengths_controls_and_future_boundary(self):
        value = self.report(NOW + store.FUTURE_TOLERANCE_MS)
        value["current_goal"] = "界🟢" * 250
        store.validate_assessment(value, NOW)
        value["reviewed_at_ms"] += 1
        with self.assertRaises(store.AssessmentError):
            store.validate_assessment(value, NOW)
        value = self.report()
        value["current_goal"] = "界" * 501
        with self.assertRaises(store.AssessmentError):
            store.validate_assessment(value, NOW)
        for control in ("\0", "\x1f", "\x7f", "\x85", "\t", "\r", "\n", "\ud800"):
            value = self.report()
            value["current_goal"] = "goal" + control
            with self.subTest(control=repr(control)), self.assertRaises(store.AssessmentError):
                store.validate_assessment(value, NOW)
            value["current_goal"] = "goal"
            value["review_note"] = "note" + control
            if control == "\n":
                store.validate_assessment(value, NOW)
            else:
                with self.assertRaises(store.AssessmentError):
                    store.validate_assessment(value, NOW)

    def test_parser_rejects_duplicate_fields_bad_encoding_and_oversize(self):
        raw = json.dumps(self.report()).encode()
        duplicate = raw.replace(b"{", b'{"schema_version":1,', 1)
        for invalid in (duplicate, b"\xff", b" " * (store.MAX_BYTES + 1), b'{"schema_version":NaN}',
                        b'{"reviewed_at_ms":' + b"9" * 5000 + b"}"):
            with self.subTest(kind=invalid[:25]), self.assertRaises(store.AssessmentError):
                store.parse_assessment(invalid, NOW)
        value = self.report()
        signal = value["signals"][0]
        signal["summary"] = "界" * 240
        signal["evidence"] = [{"ref": "界" * 160, "note": "界" * 500} for _ in range(4)]
        value["signals"] = [copy.deepcopy(signal) for _ in range(8)]
        with self.assertRaises(store.AssessmentError):
            self.write(value)
        self.assertFalse((self.root / "assessments").exists())

    def test_exact_identity_and_missing_reports(self):
        self.assertIsNone(store.read_assessment(self.root, self.session_id, NOW))
        with self.assertRaises(store.AssessmentError):
            self.write(self.report(session_id="synthetic-session-b"))
        path = self.write(self.report())
        for index in range(513):
            (path.parent / f"unrelated-{index}").write_text("{}", encoding="utf-8")
        self.assertEqual(store.read_assessment(self.root, self.session_id, NOW)["session_id"], self.session_id)
        path.write_text(json.dumps(self.report(session_id="synthetic-session-b")), encoding="utf-8")
        with self.assertRaises(store.AssessmentError):
            store.read_assessment(self.root, self.session_id, NOW)
        with self.assertRaises(store.AssessmentError):
            store.read_assessment(self.root, "../escape", NOW)

    def test_older_and_conflicting_equal_timestamp_never_replace_newer(self):
        path = self.write(self.report(NOW + 10))
        original = path.read_bytes()
        with self.assertRaises(store.AssessmentError):
            self.write(self.report(NOW))
        conflicting = self.report(NOW + 10)
        conflicting["next_step"] = "different synthetic next step"
        with self.assertRaises(store.AssessmentError):
            self.write(conflicting)
        self.assertEqual(path.read_bytes(), original)
        self.assertEqual(self.write(self.report(NOW + 10)), path)
        self.assertEqual(path.read_bytes(), original)

    def test_concurrent_writers_keep_the_newest_report(self):
        def write(timestamp):
            try:
                self.write(self.report(timestamp))
                return None
            except store.AssessmentError as error:
                return str(error)
        timestamps = [NOW + offset for offset in (8, 1, 15, 4, 10, 0, 16, 7, 12, 5, 2, 14)]
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            failures = [error for error in pool.map(write, timestamps) if error]
        self.assertTrue(all("newer assessment" in error for error in failures))
        self.assertEqual(store.read_assessment(self.root, self.session_id, NOW)["reviewed_at_ms"], max(timestamps))
        self.assertEqual(len(list((self.root / "assessments").glob("*.json"))), 1)

    def test_failed_fsync_or_replace_preserves_complete_previous_report(self):
        path = self.write(self.report())
        previous = path.read_bytes()
        for operation in ("fsync", "replace"):
            with self.subTest(operation=operation), patch.object(store.os, operation, side_effect=OSError(errno.EIO, "synthetic failure")):
                with self.assertRaises(OSError):
                    self.write(self.report(NOW + 1))
            self.assertEqual(path.read_bytes(), previous)
            self.assertEqual(list(path.parent.glob(".assessment-*.tmp")), [])

    def test_separate_cli_processes_cannot_roll_back_the_newest_report(self):
        timestamps = [NOW + offset for offset in (7, 2, 9, 1, 5, 3)]
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
            results = list(pool.map(lambda timestamp: self.run_cli("write", self.report(timestamp)), timestamps))
        self.assertTrue(all(result.returncode == 0 or "newer assessment" in result.stderr for result in results))
        self.assertEqual(store.read_assessment(self.root, self.session_id, NOW)["reviewed_at_ms"], max(timestamps))

    def test_lock_contention_is_bounded_and_does_not_write(self):
        directory = store._directory(self.root, create=True)
        with store._session_lock(directory, self.session_id, 0.1):
            started = time.monotonic()
            with self.assertRaisesRegex(store.AssessmentError, "busy"):
                store.write_assessment(self.report(), self.root, self.session_id, NOW, lock_timeout=0.02)
            self.assertLess(time.monotonic() - started, 0.5)
        self.assertIsNone(store.read_assessment(self.root, self.session_id, NOW))
        self.write(self.report())

    def test_cli_is_explicit_and_does_not_echo_report_text_on_write_or_error(self):
        value = self.report()
        value["review_note"] = "DO_NOT_ECHO_REVIEW_TEXT"
        written = self.run_cli("write", value)
        self.assertEqual(written.returncode, 0, written.stderr)
        self.assertEqual(set(json.loads(written.stdout)), {"session_id", "report_path"})
        self.assertNotIn("DO_NOT_ECHO", written.stdout + written.stderr)
        read = self.run_cli("read")
        self.assertEqual(json.loads(read.stdout), value)
        invalid = self.run_cli("write", {**value, "extra": "DO_NOT_ECHO_REVIEW_TEXT"})
        self.assertEqual(invalid.returncode, 1)
        self.assertEqual(invalid.stdout, "")
        self.assertNotIn("DO_NOT_ECHO", invalid.stderr)
        too_large_integer = self.run_cli("write", '{"reviewed_at_ms":' + "9" * 5000 + "}")
        self.assertEqual(too_large_integer.returncode, 1)
        self.assertNotIn("Traceback", too_large_integer.stderr)
        self.assertEqual(self.run_cli("read", session_id="missing-session").stdout.strip(), "null")

    @unittest.skipIf(os.name == "nt", "Unix link creation does not require elevated permissions")
    def test_symlink_roots_directories_files_and_locks_are_rejected(self):
        with tempfile.TemporaryDirectory(prefix="orb-assessment-other-") as other:
            external = Path(other)
            linked_root = self.root / "linked-root"
            linked_root.symlink_to(external, target_is_directory=True)
            with self.assertRaises(store.AssessmentError):
                store.write_assessment(self.report(), linked_root, self.session_id, NOW)
            with self.assertRaises(store.AssessmentError):
                store.read_assessment(linked_root, self.session_id, NOW)
            assessments = self.root / "assessments"
            assessments.symlink_to(external, target_is_directory=True)
            with self.assertRaises(store.AssessmentError):
                self.write(self.report())
            assessments.unlink()
            assessments.mkdir()
            external_report = external / "synthetic.json"
            external_report.write_text(json.dumps(self.report()), encoding="utf-8")
            report_path = assessments / store.assessment_filename(self.session_id)
            report_path.symlink_to(external_report)
            with self.assertRaises(store.AssessmentError):
                self.write(self.report(NOW + 1))
            with self.assertRaises(store.AssessmentError):
                store.read_assessment(self.root, self.session_id, NOW)
            report_path.unlink()
            locks = assessments / ".locks"
            locks.mkdir(exist_ok=True)
            (locks / (store.assessment_filename(self.session_id) + ".lock")).unlink(missing_ok=True)
            (locks / (store.assessment_filename(self.session_id) + ".lock")).symlink_to(external_report)
            with self.assertRaises(store.AssessmentError):
                self.write(self.report())
            self.assertEqual(json.loads(external_report.read_text(encoding="utf-8")), self.report())

    @unittest.skipIf(os.name == "nt", "Unix permission semantics")
    def test_unreadable_ancestor_is_an_error_not_missing(self):
        ancestor = self.root / "private"
        data = ancestor / "data"
        data.mkdir(parents=True)
        ancestor.chmod(0o000)
        try:
            try:
                list(data.iterdir())
            except PermissionError:
                with self.assertRaises(OSError):
                    store.read_assessment(data, self.session_id, NOW)
        finally:
            ancestor.chmod(0o700)


if __name__ == "__main__":
    unittest.main()
