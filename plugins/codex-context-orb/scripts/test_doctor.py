"""Exact-session, read-only diagnostics using synthetic local snapshots only."""

import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import doctor

SCRIPTS = Path(__file__).parent
FIXTURES = SCRIPTS / "fixtures"
REPORT = json.loads((FIXTURES / "evidence-report-valid.json").read_text(encoding="utf-8"))
ASSESSMENT = json.loads((FIXTURES / "assessment-valid.json").read_text(encoding="utf-8"))
NOW = REPORT["reviewed_at_ms"]


class DoctorTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="orb-doctor-test-")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.root = self.base / "orb-data"
        self.session = REPORT["session_id"]
        self.environment = {"ORB_DATA_DIR": str(self.root), "CODEX_THREAD_ID": self.session}

    def diagnose(self, **kwargs):
        return doctor.diagnose(environment=self.environment, now_ms=NOW, **kwargs)

    def snapshots(self):
        hook = doctor.emit_hook_event.project_event({"session_id": self.session, "hook_event_name": "Stop"}, NOW)
        assessment = copy.deepcopy(ASSESSMENT)
        assessment.update(session_id=self.session, reviewed_at_ms=NOW, current_goal="PRIVATE_SYNTHETIC_GOAL")
        values = {"hooks": ("events", hook), "assessment_v1": ("assessments", assessment), "evidence_v2": ("evidence", REPORT)}
        paths = {}
        for layer, (folder, value) in values.items():
            directory = self.root / folder
            directory.mkdir(parents=True, exist_ok=True)
            path = directory / doctor.emit_hook_event.session_filename(self.session)
            path.write_bytes(json.dumps(value, ensure_ascii=False).encode("utf-8"))
            paths[layer] = path
        return paths

    def test_absent_root_is_not_created_or_reported_as_integration_success(self):
        result = self.diagnose()
        self.assertEqual(result["root_status"], "ABSENT")
        self.assertEqual(result["root"], str(self.root))
        self.assertFalse(self.root.exists())
        self.assertTrue(all(layer["status"] == "ABSENT" and layer["count"] == 0 for layer in result["layers"].values()))
        self.assertEqual(result["verification"], doctor.UNVERIFIED)
        self.assertNotIn("overall", result)

    def test_each_valid_layer_has_its_own_exact_byte_hash_without_prose(self):
        paths = self.snapshots()
        result = self.diagnose()
        self.assertEqual(result["identity"], {"source": "CODEX_THREAD_ID", "status": "VALID", "session_id": self.session, "runtime_match": "MATCH"})
        for layer, path in paths.items():
            self.assertEqual(result["layers"][layer]["status"], "PRESENT")
            self.assertEqual(result["layers"][layer]["count"], 1)
            self.assertEqual(result["layers"][layer]["file_sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
        self.assertEqual(result["layers"]["evidence_v2"]["report_id"], REPORT["report_id"])
        self.assertEqual(result["layers"]["evidence_v2"]["checks"], {"pass": 1, "fail": 0, "unknown": 0})
        self.assertNotIn("PRIVATE_SYNTHETIC_GOAL", json.dumps(result))
        self.assertNotIn(REPORT["ledger"][0]["text"], json.dumps(result, ensure_ascii=False))

    def test_explicit_identity_is_never_replaced_by_runtime_or_recent_files(self):
        self.snapshots()
        result = self.diagnose(session_id="another-explicit-session")
        self.assertEqual(result["identity"]["source"], "argument")
        self.assertEqual(result["identity"]["runtime_match"], "MISMATCH")
        self.assertTrue(all(layer["status"] == "ABSENT" for layer in result["layers"].values()))
        self.environment.pop("CODEX_THREAD_ID")
        self.assertEqual(self.diagnose(session_id=self.session)["identity"]["runtime_match"], "NOT_VERIFIED")
        absent = self.diagnose()
        self.assertEqual(absent["identity"]["status"], "ABSENT")
        self.assertTrue(all(layer["status"] == "NOT_CHECKED" for layer in absent["layers"].values()))

    def test_invalid_identity_does_not_read_or_echo_the_argument(self):
        with patch.object(doctor.evidence_review, "_read_plain", side_effect=AssertionError("no reads")):
            result = self.diagnose(session_id="../PRIVATE_SYNTHETIC_ID")
        self.assertEqual(result["identity"]["status"], "INVALID")
        self.assertIsNone(result["identity"]["session_id"])
        self.assertNotIn("PRIVATE_SYNTHETIC_ID", json.dumps(result))

    def test_root_precedence_and_empty_or_relative_override_are_explicit(self):
        other = self.base / "explicit-root"
        result = self.diagnose(data_dir=str(other))
        self.assertEqual((result["root_source"], result["root"]), ("argument", str(other)))
        self.environment.pop("ORB_DATA_DIR")
        with patch.object(Path, "home", return_value=self.base):
            result = self.diagnose()
        self.assertEqual((result["root_source"], result["root"]), ("default", str(self.base / ".codex-context-orb")))
        for value in ("", "relative"):
            self.environment["ORB_DATA_DIR"] = value
            self.assertEqual(self.diagnose()["root_status"], "INVALID")

    def test_sensitive_or_unbounded_roots_are_redacted_before_filesystem_access(self):
        for root in (self.base / ".codex" / "PRIVATE_SYNTHETIC_PATH", self.base / ".ssh", self.base / ".env", self.base / ("x" * 3_000)):
            with self.subTest(root=root), patch.object(Path, "lstat", side_effect=AssertionError("private root must not be inspected")):
                result = self.diagnose(data_dir=str(root))
            self.assertIsNone(result["root"])
            self.assertEqual(result["root_status"], "INVALID")
            self.assertNotIn("PRIVATE_SYNTHETIC_PATH", json.dumps(result))

    def test_corruption_in_one_layer_does_not_hide_other_valid_layers(self):
        paths = self.snapshots()
        paths["hooks"].write_bytes(b'{"PRIVATE_SYNTHETIC":')
        result = self.diagnose()
        self.assertEqual(result["layers"]["hooks"]["status"], "INVALID")
        self.assertIsNone(result["layers"]["hooks"]["count"])
        self.assertEqual(result["layers"]["assessment_v1"]["status"], "PRESENT")
        self.assertEqual(result["layers"]["evidence_v2"]["status"], "PRESENT")
        paths["hooks"].unlink()
        self.assertEqual(self.diagnose()["layers"]["hooks"]["status"], "ABSENT")

    def test_unreadable_is_distinct_from_absent_and_invalid(self):
        paths = self.snapshots()
        original = doctor.evidence_review._read_plain

        def read(path, maximum):
            if path == paths["hooks"]:
                raise PermissionError("PRIVATE_SYNTHETIC_PATH")
            return original(path, maximum)

        with patch.object(doctor.evidence_review, "_read_plain", read):
            result = self.diagnose()
        self.assertEqual(result["layers"]["hooks"]["status"], "UNREADABLE")
        self.assertEqual(result["layers"]["evidence_v2"]["status"], "PRESENT")
        self.assertNotIn("PRIVATE_SYNTHETIC_PATH", json.dumps(result))

    def test_duplicate_oversized_and_wrong_identity_snapshots_are_invalid(self):
        paths = self.snapshots()
        for name, path in paths.items():
            original = path.read_bytes()
            invalid_inputs = [original.replace(b"{", b'{"schema_version":1,', 1), b"x" * 65_537]
            value = json.loads(original)
            value["session_id"] = "wrong-session"
            if name == "evidence_v2":
                value["report_id"] = doctor.evidence_review.report_hash(value)
            invalid_inputs.append(json.dumps(value).encode())
            for raw in invalid_inputs:
                with self.subTest(layer=name, sample=raw[:20]):
                    path.write_bytes(raw)
                    self.assertEqual(self.diagnose()["layers"][name]["status"], "INVALID")
            path.write_bytes(original)

    def test_future_hook_is_invalid_and_unknown_checks_do_not_become_verified(self):
        paths = self.snapshots()
        hook = json.loads(paths["hooks"].read_bytes())
        hook["observed_at_ms"] = NOW + 60_001
        paths["hooks"].write_text(json.dumps(hook), encoding="utf-8")
        report = copy.deepcopy(REPORT)
        report["probes"][0].update(rule="manual", source_id=None, expected="", result="unknown")
        report["report_id"] = doctor.evidence_review.report_hash(report)
        paths["evidence_v2"].write_text(json.dumps(report), encoding="utf-8")
        result = self.diagnose()
        self.assertEqual(result["layers"]["hooks"]["status"], "INVALID")
        self.assertEqual(result["layers"]["evidence_v2"]["checks"], {"pass": 0, "fail": 0, "unknown": 1})
        self.assertEqual(result["verification"], doctor.UNVERIFIED)

    @unittest.skipIf(os.name == "nt", "Unix symlink fixture; shared reader also rejects Windows reparse points")
    def test_linked_roots_layers_files_and_ancestors_are_not_followed(self):
        paths = self.snapshots()
        linked = self.base / "linked-root"
        linked.symlink_to(self.root, target_is_directory=True)
        self.assertEqual(self.diagnose(data_dir=str(linked))["root_status"], "INVALID")
        self.assertEqual(self.diagnose(data_dir=str(linked / "nested"))["root_status"], "INVALID")
        paths["hooks"].unlink()
        paths["hooks"].symlink_to(paths["evidence_v2"])
        self.assertEqual(self.diagnose()["layers"]["hooks"]["status"], "INVALID")
        paths["hooks"].unlink()
        paths["hooks"].parent.rmdir()
        paths["hooks"].parent.symlink_to(paths["evidence_v2"].parent, target_is_directory=True)
        self.assertEqual(self.diagnose()["layers"]["hooks"]["status"], "INVALID")

    def test_no_inventory_or_mutation_api_is_used(self):
        self.snapshots()
        with patch.object(Path, "iterdir", side_effect=AssertionError("no inventory")), \
                patch.object(Path, "mkdir", side_effect=AssertionError("no directory writes")), \
                patch.object(os, "replace", side_effect=AssertionError("no writes")), \
                patch.object(os, "scandir", side_effect=AssertionError("no inventory")):
            result = self.diagnose()
        self.assertTrue(all(layer["status"] == "PRESENT" for layer in result["layers"].values()))

    def test_cli_is_bounded_utf8_and_imports_do_not_create_bytecode(self):
        plugin = self.base / "scripts"
        plugin.mkdir()
        for name in ("doctor.py", "assessment_store.py", "emit_hook_event.py", "evidence_review.py"):
            (plugin / name).write_bytes((SCRIPTS / name).read_bytes())
        root = self.base / "自检数据"
        command = [sys.executable, str(plugin / "doctor.py"), "--session-id", self.session, "--data-dir", str(root)]
        result = subprocess.run(command, capture_output=True, timeout=5, env={**os.environ, "PYTHONIOENCODING": "ascii"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertLessEqual(len(result.stdout), doctor.MAX_OUTPUT_BYTES)
        self.assertEqual(json.loads(result.stdout)["root"], str(root))
        self.assertFalse(root.exists())
        self.assertFalse((plugin / "__pycache__").exists())
        invalid = subprocess.run(command + ["--PRIVATE_SYNTHETIC_ARGUMENT"], capture_output=True, timeout=5)
        self.assertEqual(invalid.returncode, 2)
        self.assertNotIn(b"PRIVATE_SYNTHETIC_ARGUMENT", invalid.stdout + invalid.stderr)
        self.assertNotIn(b"Traceback", invalid.stdout + invalid.stderr)


if __name__ == "__main__":
    unittest.main()
