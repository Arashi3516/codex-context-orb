"""Synthetic-only evidence collection, provenance, privacy and storage checks."""

import concurrent.futures
import copy
import errno
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import evidence_review as evidence

FIXTURES = Path(__file__).parent / "fixtures"
MANIFEST = json.loads((FIXTURES / "evidence-manifest-valid.json").read_text(encoding="utf-8"))
REPORT = json.loads((FIXTURES / "evidence-report-valid.json").read_text(encoding="utf-8"))
NOW = REPORT["reviewed_at_ms"]


class EvidenceReviewTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="orb-evidence-test-")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.root = self.base / "data"
        self.workspace = self.base / "workspace"
        self.workspace.mkdir()
        (self.workspace / "status.txt").write_bytes((FIXTURES / "evidence-workspace" / "status.txt").read_bytes())
        self.session = REPORT["session_id"]

    def manifest(self):
        return copy.deepcopy(MANIFEST)

    def report(self, timestamp=NOW):
        value = copy.deepcopy(REPORT)
        value["reviewed_at_ms"] = timestamp
        return self.rehash(value)

    def rehash(self, value):
        value["report_id"] = evidence.report_hash(value)
        return value

    def write(self, value):
        return evidence.write_report(value, self.root, NOW)

    def test_shared_fixture_collection_and_hash_are_identical(self):
        evidence.validate_manifest(MANIFEST)
        evidence.validate_report(REPORT, NOW)
        self.assertEqual(evidence.collect(MANIFEST, self.workspace, NOW), REPORT)
        different_order = {key: REPORT[key] for key in reversed(REPORT)}
        self.assertEqual(evidence.report_hash(different_order), REPORT["report_id"])
        self.assertEqual(evidence.parse_report(json.dumps(REPORT, ensure_ascii=True).encode(), NOW), REPORT)

    def test_all_checks_use_one_capture_and_do_not_store_artifact_contents(self):
        manifest = self.manifest()
        raw = "release.status=ready\nPRIVATE_SYNTHETIC_BODY_🟢\n".encode()
        manifest["probes"].extend([
            {"id": "probe-absent", "item_id": "constraint-status", "source_id": "source-status", "rule": "not_contains", "expected": "blocked"},
            {"id": "probe-fails", "item_id": "constraint-status", "source_id": "source-status", "rule": "contains", "expected": "blocked"},
            {"id": "probe-sha", "item_id": "constraint-status", "source_id": "source-status", "rule": "sha256", "expected": hashlib.sha256(raw).hexdigest()},
            {"id": "probe-manual", "item_id": "constraint-status", "source_id": None, "rule": "manual", "expected": ""},
        ])
        with patch.object(evidence, "_capture", side_effect=[raw, b"changed"]) as capture:
            report = evidence.collect(manifest, self.workspace, NOW)
        self.assertEqual(capture.call_count, 1)
        self.assertEqual([probe["result"] for probe in report["probes"]], ["pass", "pass", "fail", "pass", "unknown"])
        self.assertEqual(report["sources"][1]["sha256"], hashlib.sha256(raw).hexdigest())
        self.assertNotIn("PRIVATE_SYNTHETIC_BODY", evidence.canonical_bytes(report).decode())
        self.assertEqual(report["sources"][0]["status"], "attested")
        self.assertIsNone(report["sources"][0]["sha256"])

    def test_unavailable_non_utf8_oversize_and_private_sources_remain_unknown(self):
        (self.workspace / "binary.txt").write_bytes(b"\xff\xfe")
        (self.workspace / "large.txt").write_bytes(b"x" * (evidence.MAX_ARTIFACT_BYTES + 1))
        (self.workspace / ".env").write_text("SYNTHETIC_PRIVATE_VALUE", encoding="utf-8")
        (self.workspace / ".codex").mkdir()
        (self.workspace / ".codex" / "session.jsonl").write_text("SYNTHETIC_PRIVATE_TRANSCRIPT", encoding="utf-8")
        for ref in ("missing.txt", "binary.txt", "large.txt", ".env", ".env.example", ".codex/session.jsonl"):
            manifest = self.manifest()
            manifest["sources"][1]["ref"] = ref
            with self.subTest(ref=ref):
                report = evidence.collect(manifest, self.workspace, NOW)
                self.assertEqual(report["sources"][1]["status"], "unavailable")
                self.assertIsNone(report["sources"][1]["sha256"])
                self.assertEqual(report["probes"][0]["result"], "unknown")
        with patch.object(evidence.os, "open", side_effect=AssertionError("must not open")):
            self.assertIsNone(evidence._capture(self.workspace, [".env"]))
            self.assertIsNone(evidence._capture(self.workspace, [".codex", "session.jsonl"]))

    def test_invalid_paths_are_rejected_before_any_collection(self):
        for ref in ("../escape", "/absolute", "a/../b", "a/./b", "a//b", "a/", "C:/private", "a\\b", "file:stream", "NUL", "con.txt", "dir/file.", "dir/file "):
            manifest = self.manifest()
            manifest["sources"][1]["ref"] = ref
            with self.subTest(ref=ref), patch.object(evidence, "_capture", side_effect=AssertionError("no reads")), self.assertRaises(evidence.EvidenceError):
                evidence.collect(manifest, self.workspace, NOW)
        with self.assertRaises(evidence.EvidenceError):
            evidence.collect(self.manifest(), Path("relative"), NOW)

    def test_manifest_cannot_supply_results_receipts_or_report_metadata(self):
        for location, field, value in (("root", "report_id", "0" * 64), ("root", "reviewed_at_ms", NOW),
                                       ("source", "status", "captured"), ("source", "sha256", "0" * 64),
                                       ("probe", "result", "pass"), ("probe", "detail", "pretend verified")):
            manifest = self.manifest()
            target = manifest if location == "root" else manifest["sources"][1] if location == "source" else manifest["probes"][0]
            target[field] = value
            with self.subTest(location=location, field=field), self.assertRaises(evidence.EvidenceError):
                evidence.validate_manifest(manifest)
        for field in evidence.MANIFEST_KEYS:
            manifest = self.manifest()
            del manifest[field]
            with self.subTest(missing=field), self.assertRaises(evidence.EvidenceError):
                evidence.validate_manifest(manifest)

    def test_strict_ids_references_active_goal_and_supersession(self):
        mutations = [
            lambda m: m["sources"].append(copy.deepcopy(m["sources"][0])),
            lambda m: m["ledger"][1].update(source_ids=["unknown-source"]),
            lambda m: m["ledger"][1].update(source_ids=["source-status", "source-status"]),
            lambda m: m["scope"].update(goal_id="constraint-status"),
            lambda m: m["ledger"][0].update(status="hypothesis"),
            lambda m: m["probes"][0].update(item_id="missing-item"),
            lambda m: m["probes"][0].update(source_id="source-statement"),
            lambda m: m["probes"][0].update(source_id="missing-source"),
            lambda m: m["ledger"][1].update(status="superseded"),
            lambda m: m["ledger"][1].update(supersedes=["goal-review-status"]),
            lambda m: m["ledger"][1].update(supersedes=["missing-item"]),
            lambda m: m["observations"].append({"id": "obs", "item_id": "missing", "kind": "goal_drift", "summary": "synthetic", "status": "open", "recurrence": "once", "source_ids": ["source-statement"]}),
        ]
        for index, mutate in enumerate(mutations):
            manifest = self.manifest()
            mutate(manifest)
            with self.subTest(index=index), self.assertRaises(evidence.EvidenceError):
                evidence.validate_manifest(manifest)
        manifest = self.manifest()
        previous = {**copy.deepcopy(manifest["ledger"][1]), "id": "previous-rule", "status": "superseded"}
        manifest["ledger"].append(previous)
        manifest["ledger"][1]["supersedes"] = ["previous-rule"]
        evidence.validate_manifest(manifest)
        previous["supersedes"] = ["previous-rule"]
        with self.assertRaisesRegex(evidence.EvidenceError, "cycle"):
            evidence.validate_manifest(manifest)

    def test_receipt_rules_and_hash_are_checked_without_reopening_artifacts(self):
        changes = [lambda r: r["sources"][0].update(status="captured", sha256="0" * 64),
                   lambda r: r["sources"][1].update(ref=".codex/sessions/file.jsonl"),
                   lambda r: r["sources"][1].update(status="captured", sha256=None),
                   lambda r: r["sources"][1].update(status="unavailable", sha256=None),
                   lambda r: r["probes"][0].update(result="unknown"),
                   lambda r: r["probes"][0].update(rule="manual", expected="", source_id=None, result="pass"),
                   lambda r: r["probes"][0].update(rule="manual", expected="", result="unknown"),
                   lambda r: r["probes"][0].update(rule="sha256", expected="0" * 64, result="pass")]
        for index, change in enumerate(changes):
            value = self.report()
            change(value)
            self.rehash(value)
            with self.subTest(index=index), self.assertRaises(evidence.EvidenceError):
                evidence.validate_report(value, NOW)
        tampered = self.report()
        tampered["scope"]["next_step"] = "tampered"
        with self.assertRaisesRegex(evidence.EvidenceError, "hash"):
            evidence.validate_report(tampered, NOW)
        with patch.object(evidence, "_capture", side_effect=AssertionError("report validation must not read artifacts")):
            evidence.validate_report(self.report(), NOW)

    def test_types_text_limits_json_duplicates_and_byte_limits(self):
        for value in (True, 2.0, 3):
            report = self.report()
            report["schema_version"] = value
            with self.subTest(version=value), self.assertRaises(evidence.EvidenceError):
                evidence.validate_report(self.rehash(report), NOW)
        for timestamp in (True, 1.0, -1, evidence.MAX_SAFE_INTEGER + 1, NOW + 60_001):
            report = self.report()
            report["reviewed_at_ms"] = timestamp
            with self.subTest(timestamp=timestamp), self.assertRaises(evidence.EvidenceError):
                evidence.validate_report(self.rehash(report), NOW)
        evidence.validate_report(self.report(NOW + 60_000), NOW)
        for control in ("\0", "\x1f", "\x7f", "\x85", "\t", "\r", "\n", "\ud800"):
            manifest = self.manifest()
            manifest["scope"]["next_step"] += control
            with self.subTest(control=repr(control)), self.assertRaises(evidence.EvidenceError):
                evidence.validate_manifest(manifest)
        manifest = self.manifest()
        manifest["scope"]["next_step"] = "界🟢" * 250
        evidence.validate_manifest(manifest)
        manifest["scope"]["next_step"] += "界"
        with self.assertRaises(evidence.EvidenceError):
            evidence.validate_manifest(manifest)
        raw = evidence.canonical_bytes(REPORT)
        for invalid in (raw.replace(b"{", b'{"schema_version":2,', 1), b"\xff", b" " * (evidence.MAX_BYTES + 1),
                        b'{"schema_version":NaN}', b'{"schema_version":1.0}', b'{"reviewed_at_ms":' + b"9" * 5000 + b"}"):
            with self.subTest(raw=invalid[:24]), self.assertRaises(evidence.EvidenceError):
                evidence.parse_report(invalid, NOW)
        for key, count in (("sources", 17), ("ledger", 25), ("probes", 33), ("observations", 9)):
            manifest = self.manifest()
            manifest[key] = [{}] * count
            with self.subTest(collection=key), self.assertRaises(evidence.EvidenceError):
                evidence.validate_manifest(manifest)

    def test_byte_limit_applies_even_when_each_individual_field_is_valid(self):
        report = self.report()
        goal = report["ledger"][0]
        goal["text"] = "界" * 500
        report["ledger"] = [goal] + [{**copy.deepcopy(report["ledger"][1]), "id": f"constraint-{index}", "text": "界" * 500} for index in range(23)]
        report["probes"] = [{**copy.deepcopy(REPORT["probes"][0]), "id": f"probe-{index}", "item_id": "constraint-0",
                             "expected": "界" * 240, "detail": "界" * 240} for index in range(32)]
        self.rehash(report)
        with self.assertRaisesRegex(evidence.EvidenceError, "64 KiB"):
            self.write(report)
        self.assertFalse(self.root.exists())

    @unittest.skipIf(os.name == "nt", "Unix fixture for descriptor-relative traversal and FIFOs")
    def test_links_directories_and_fifo_are_unavailable_without_blocking(self):
        external = self.base / "external"
        external.mkdir()
        (external / "private.txt").write_text("DO_NOT_READ_SYNTHETIC", encoding="utf-8")
        (self.workspace / "link.txt").symlink_to(external / "private.txt")
        (self.workspace / "linked").symlink_to(external, target_is_directory=True)
        os.mkfifo(self.workspace / "fifo")
        for ref in ("link.txt", "linked/private.txt", "fifo", "linked"):
            manifest = self.manifest()
            manifest["sources"][1]["ref"] = ref
            started = time.monotonic()
            with self.subTest(ref=ref):
                report = evidence.collect(manifest, self.workspace, NOW)
                self.assertEqual(report["probes"][0]["result"], "unknown")
                self.assertLess(time.monotonic() - started, 0.5)
        self.assertTrue(evidence._is_link(SimpleNamespace(st_mode=0o100600, st_file_attributes=0x400)))

    @unittest.skipIf(os.name == "nt", "Unix descriptor read mutation test")
    def test_changed_during_read_invalidates_hash_and_checks(self):
        original = evidence.os.fstat
        calls = 0

        def changed(descriptor):
            nonlocal calls
            metadata = original(descriptor)
            calls += 1
            if calls == 2:
                return SimpleNamespace(st_dev=metadata.st_dev, st_ino=metadata.st_ino, st_size=metadata.st_size,
                                       st_mtime_ns=metadata.st_mtime_ns + 1, st_ctime_ns=metadata.st_ctime_ns,
                                       st_mode=metadata.st_mode)
            return metadata

        with patch.object(evidence.os, "fstat", side_effect=changed):
            report = evidence.collect(self.manifest(), self.workspace, NOW)
        self.assertEqual(report["sources"][1]["status"], "unavailable")
        self.assertIsNone(report["sources"][1]["sha256"])
        self.assertEqual(report["probes"][0]["result"], "unknown")

    def test_storage_latest_exact_identity_and_eight_immutable_history_reports(self):
        self.assertIsNone(evidence.read_report(self.root, self.session, NOW))
        self.assertEqual(evidence.read_history(self.root, self.session, NOW), [])
        saved = {}
        for offset in range(12):
            report = self.report(NOW + offset)
            destination = self.write(report)
            archived = destination.parent / "history" / evidence.session_hash(self.session) / (report["report_id"] + ".json")
            saved[report["report_id"]] = archived.read_bytes()
        history = evidence.read_history(self.root, self.session, NOW)
        self.assertEqual([r["reviewed_at_ms"] for r in history], list(range(NOW + 11, NOW + 3, -1)))
        for report in history:
            archived = destination.parent / "history" / evidence.session_hash(self.session) / (report["report_id"] + ".json")
            self.assertEqual(archived.read_bytes(), saved[report["report_id"]])
        self.assertEqual(len(list(archived.parent.glob("*.json"))), 8)
        for index in range(513):
            (destination.parent / f"junk-{index}").write_text("{}", encoding="utf-8")
        self.assertEqual(evidence.read_report(self.root, self.session, NOW), history[0])
        self.assertIsNone(evidence.read_report(self.root, "missing-session", NOW))
        with self.assertRaises(evidence.EvidenceError):
            evidence.read_report(self.root, "../escape", NOW)
        if os.name != "nt":
            self.assertEqual(destination.stat().st_mode & 0o777, 0o600)

    def test_older_and_conflicting_equal_timestamp_are_rejected(self):
        path = self.write(self.report(NOW + 2))
        before = path.read_bytes()
        with self.assertRaises(evidence.EvidenceError):
            self.write(self.report(NOW + 1))
        value = self.report(NOW + 2)
        value["scope"]["next_step"] = "different declaration"
        with self.assertRaises(evidence.EvidenceError):
            self.write(self.rehash(value))
        self.assertEqual(self.write(self.report(NOW + 2)), path)
        self.assertEqual(path.read_bytes(), before)

    def test_concurrent_writers_keep_the_newest_valid_report(self):
        timestamps = [NOW + offset for offset in (8, 1, 11, 4, 10, 0, 12, 7, 5, 2)]

        def write(timestamp):
            try:
                self.write(self.report(timestamp))
                return None
            except evidence.EvidenceError as error:
                return str(error)

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            errors = [error for error in pool.map(write, timestamps) if error]
        self.assertTrue(all("newer evidence" in error for error in errors), errors)
        self.assertEqual(evidence.read_report(self.root, self.session, NOW)["reviewed_at_ms"], max(timestamps))
        self.assertLessEqual(len(evidence.read_history(self.root, self.session, NOW)), 8)

    def test_lock_wait_and_windows_share_retry_are_bounded(self):
        directory = evidence._directory(self.root, create=True)
        with evidence._session_lock(directory, self.session, 0.1):
            started = time.monotonic()
            with self.assertRaisesRegex(evidence.EvidenceError, "busy"):
                evidence.write_report(self.report(), self.root, NOW, lock_timeout=0.02)
            self.assertLess(time.monotonic() - started, 0.5)
        sharing = OSError(errno.EACCES, "synthetic Windows sharing conflict")
        sharing.winerror = 32
        operation = unittest.mock.Mock(side_effect=[sharing, sharing, "done"])
        self.assertEqual(evidence._retry_share(operation), "done")
        self.assertEqual(operation.call_count, 3)
        with patch.object(evidence, "RENAME_TIMEOUT_SECONDS", 0.02):
            started = time.monotonic()
            with self.assertRaises(OSError):
                evidence._retry_share(unittest.mock.Mock(side_effect=sharing))
            self.assertLess(time.monotonic() - started, 0.5)

    def test_atomic_failures_preserve_latest_and_history_failure_reports_commit(self):
        destination = self.write(self.report())
        previous = destination.read_bytes()
        for operation in ("fsync", "replace"):
            with self.subTest(operation=operation), patch.object(evidence.os, operation, side_effect=OSError(errno.EIO, "synthetic")):
                with self.assertRaises(OSError):
                    self.write(self.report(NOW + 1))
            self.assertEqual(destination.read_bytes(), previous)
            self.assertEqual(list(destination.parent.glob(".evidence-*.tmp")), [])
        original = evidence._archive

        def archive(directory, report, now_ms):
            if report["reviewed_at_ms"] > NOW:
                raise OSError(errno.EIO, "synthetic archive failure")
            return original(directory, report, now_ms)

        with patch.object(evidence, "_archive", side_effect=archive), self.assertRaisesRegex(evidence.EvidenceError, "Latest evidence was saved"):
            self.write(self.report(NOW + 1))
        self.assertEqual(evidence.read_report(self.root, self.session, NOW)["reviewed_at_ms"], NOW + 1)
        self.assertEqual(evidence.read_history(self.root, self.session, NOW)[0]["reviewed_at_ms"], NOW + 1)

    def test_corrupt_history_is_not_overwritten_and_history_scan_is_bounded(self):
        destination = self.write(self.report())
        history = destination.parent / "history" / evidence.session_hash(self.session)
        archived = history / (REPORT["report_id"] + ".json")
        archived.write_text("{}", encoding="utf-8")
        with self.assertRaises(evidence.EvidenceError):
            self.write(self.report(NOW + 1))
        self.assertEqual(archived.read_text(), "{}")
        archived.write_bytes(evidence.canonical_bytes(REPORT))
        for index in range(evidence.MAX_HISTORY_ENTRIES + 1):
            (history / f"unknown-{index}").write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(evidence.EvidenceError, "bounded inventory"):
            evidence.read_history(self.root, self.session, NOW)

    @unittest.skipIf(os.name == "nt", "Unix link fixture")
    def test_storage_and_input_symlinks_or_private_paths_are_rejected(self):
        external = self.base / "external"
        external.mkdir()
        self.root.symlink_to(external, target_is_directory=True)
        with self.assertRaises(evidence.EvidenceError):
            self.write(self.report())
        self.root.unlink()
        destination = self.write(self.report())
        target = external / "report.json"
        target.write_bytes(destination.read_bytes())
        destination.unlink()
        destination.symlink_to(target)
        with self.assertRaises(evidence.EvidenceError):
            evidence.read_report(self.root, self.session, NOW)
        private = self.workspace / ".codex"
        private.mkdir()
        (private / "transcript.jsonl").write_text("PRIVATE_SYNTHETIC", encoding="utf-8")
        with self.assertRaises(evidence.EvidenceError):
            evidence._input(str(private / "transcript.jsonl"))

    def test_cli_utf8_collect_validate_read_history_and_failures(self):
        manifest_path = self.workspace / "manifest.json"
        manifest_path.write_text(json.dumps(MANIFEST, ensure_ascii=False), encoding="utf-8")

        def run(*args, raw=None):
            return subprocess.run([sys.executable, str(Path(evidence.__file__)), *args], input=raw,
                                  capture_output=True, timeout=5,
                                  env={**os.environ, "ORB_DATA_DIR": str(self.root), "PYTHONIOENCODING": "ascii"})

        result = run("collect", "--input", str(manifest_path), "--workspace", str(self.workspace))
        self.assertEqual(result.returncode, 0, result.stderr)
        report = evidence.parse_report(result.stdout)
        self.assertEqual(report["scope"]["next_step"], REPORT["scope"]["next_step"])
        validated = run("validate", "--input", "-", raw=result.stdout)
        self.assertEqual(json.loads(validated.stdout), {"valid": True, "report_id": report["report_id"]})
        self.assertEqual(json.loads(run("read", "--session-id", self.session).stdout), report)
        self.assertEqual(json.loads(run("history", "--session-id", self.session).stdout), [report])
        self.assertIsNone(json.loads(run("read", "--session-id", "missing").stdout))
        invalid = run("collect", "--input", "-", "--workspace", str(self.workspace), raw=b'{"private":"DO_NOT_ECHO_SYNTHETIC"}')
        self.assertEqual(invalid.returncode, 1)
        self.assertEqual(invalid.stdout, b"")
        self.assertNotIn(b"DO_NOT_ECHO", invalid.stderr)
        self.assertNotIn(b"Traceback", invalid.stderr)


if __name__ == "__main__":
    unittest.main()
