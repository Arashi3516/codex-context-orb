"""Privacy and boundary tests for the local hook adapter; no Codex installation required."""

import concurrent.futures
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import emit_hook_event as emitter
import inspect_events as reader


class HookAdapterTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="orb-hook-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.payload = {"session_id": "thr-alpha", "turn_id": "turn-one", "model": "gpt-test",
                        "hook_event_name": "UserPromptSubmit"}

    def run_hook(self, payload):
        content = json.dumps(payload) if not isinstance(payload, str) else payload
        environment = {**os.environ, "ORB_DATA_DIR": str(self.root)}
        return subprocess.run([sys.executable, str(Path(emitter.__file__))], input=content,
                              text=True, capture_output=True, timeout=5, env=environment)

    def test_only_metadata_is_persisted_and_nothing_is_injected(self):
        private = {"prompt": "PRIVATE_USER_TEXT", "cwd": "/PRIVATE_PATH", "transcript_path": "/PRIVATE_TRANSCRIPT",
                   "tool_response": "PRIVATE_TOOL_OUTPUT", "last_assistant_message": "PRIVATE_ASSISTANT_TEXT",
                   "context_used_tokens": 9, "context_window_tokens": 10}
        process = self.run_hook({**self.payload, **private})
        self.assertEqual((process.returncode, process.stdout, process.stderr), (0, "{}\n", ""))
        path = next((self.root / "events").glob("*.json"))
        text = path.read_text()
        self.assertNotIn("PRIVATE", text)
        snapshot = json.loads(text)
        self.assertEqual(set(snapshot), emitter.SNAPSHOT_KEYS)
        self.assertIsNone(snapshot["context_used_tokens"])
        self.assertIsNone(snapshot["context_window_tokens"])
        self.assertEqual(snapshot["binding"], "unbound")

    def test_invalid_and_oversized_inputs_are_advisory_no_ops(self):
        for value in ["{bad", "[]", {**self.payload, "session_id": "../../escape"},
                      {**self.payload, "hook_event_name": "SubagentStop"},
                      "x" * (emitter.MAX_INPUT_BYTES + 1)]:
            with self.subTest(value_type=type(value).__name__):
                process = self.run_hook(value)
                self.assertEqual((process.returncode, process.stdout, process.stderr), (0, "{}\n", ""))
        self.assertFalse((self.root / "events").exists())

    def test_missing_and_invalid_optional_values_stay_unknown(self):
        snapshot = emitter.project_event({**self.payload, "turn_id": "bad id", "model": "https://private.invalid",
                                          "hook_event_name": "SessionStart", "trigger": "auto"}, 100)
        self.assertIsNone(snapshot["turn_id"])
        self.assertIsNone(snapshot["model"])
        self.assertIsNone(snapshot["trigger"])

    def test_compaction_is_an_event_not_a_token_estimate(self):
        snapshot = emitter.project_event({**self.payload, "hook_event_name": "PostCompact", "trigger": "auto"}, 100)
        self.assertEqual(snapshot["trigger"], "auto")
        self.assertIsNone(snapshot["context_used_tokens"])
        self.assertNotIn("compaction_count", snapshot)

    def test_snapshot_validation_rejects_extra_fields_and_false_precision(self):
        snapshot = emitter.project_event(self.payload, 100)
        self.assertTrue(emitter.valid_snapshot(snapshot))
        for changes in [{"prompt": "private"}, {"context_used_tokens": 0}, {"binding": "foreground"},
                        {"observed_at_ms": True}, {"schema_version": True}]:
            self.assertFalse(emitter.valid_snapshot({**snapshot, **changes}))

    def test_repeat_session_overwrites_one_complete_snapshot(self):
        for timestamp in (100, 200):
            emitter.write_snapshot(emitter.project_event(self.payload, timestamp), self.root)
        paths = list((self.root / "events").iterdir())
        self.assertEqual(len(paths), 1)
        self.assertEqual(reader.read_snapshot(paths[0])["observed_at_ms"], 200)

    def test_concurrent_writers_never_leave_partial_json(self):
        def write(timestamp):
            return emitter.write_snapshot(emitter.project_event(self.payload, timestamp), self.root)
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(write, range(16)))
        paths = list((self.root / "events").iterdir())
        self.assertEqual(len(paths), 1)
        self.assertTrue(emitter.valid_snapshot(json.loads(paths[0].read_text())))
        # No assertion about event causality: completion order is deliberately advisory.

    def test_explicit_session_does_not_follow_newer_background_activity(self):
        emitter.write_snapshot(emitter.project_event(self.payload, 100), self.root)
        emitter.write_snapshot(emitter.project_event({**self.payload, "session_id": "thr-background"}, 200), self.root)
        report = reader.inspect(self.root, "thr-alpha")
        self.assertEqual([event["session_id"] for event in report["events"]], ["thr-alpha"])
        self.assertEqual(report["binding"], "unbound")

    def test_reader_rejects_wrong_filename_oversize_and_extra_fields(self):
        path = emitter.write_snapshot(emitter.project_event(self.payload, 100), self.root)
        original = path.read_text()
        path.write_text(json.dumps({**json.loads(original), "prompt": "private"}))
        self.assertIsNone(reader.read_snapshot(path))
        path.write_text("x" * (emitter.MAX_SNAPSHOT_BYTES + 1))
        self.assertIsNone(reader.read_snapshot(path))
        path.write_text(original)
        wrong = path.with_name("0" * 64 + ".json")
        path.rename(wrong)
        self.assertIsNone(reader.read_snapshot(wrong))

    def test_reader_leaves_missing_session_unknown(self):
        report = reader.inspect(self.root, "thr-missing")
        self.assertEqual(report["events"], [])
        self.assertEqual(report["context_status"], "unknown")

    def test_reader_marks_bounded_inventory_as_partial(self):
        for index in range(reader.MAX_SNAPSHOTS + 1):
            emitter.write_snapshot(emitter.project_event({**self.payload, "session_id": f"thr-{index}"}, index), self.root)
        report = reader.inspect(self.root)
        self.assertEqual(len(report["events"]), reader.MAX_SNAPSHOTS)
        self.assertEqual(report["coverage"], "partial")

    def test_hooks_are_async_and_inject_no_context(self):
        hooks = json.loads((Path(emitter.__file__).parent.parent / "hooks" / "hooks.json").read_text())["hooks"]
        self.assertEqual(set(hooks), emitter.EVENT_NAMES)
        for groups in hooks.values():
            for group in groups:
                for hook in group["hooks"]:
                    self.assertIs(hook["async"], True)
                    self.assertLessEqual(hook["timeout"], 2)
                    self.assertIn("PLUGIN_ROOT", hook["command"])
                    self.assertIn("py -3", hook["commandWindows"])

    def test_configured_command_works_from_an_installed_path_with_spaces(self):
        plugin_root = self.root / "plugin with spaces"
        scripts = plugin_root / "scripts"
        scripts.mkdir(parents=True)
        shutil.copyfile(Path(emitter.__file__), scripts / "emit_hook_event.py")
        hook = json.loads((Path(emitter.__file__).parent.parent / "hooks" / "hooks.json").read_text())["hooks"]["SessionStart"][0]["hooks"][0]
        command = hook["commandWindows"] if os.name == "nt" else hook["command"]
        process = subprocess.run(command, shell=True, input=json.dumps(self.payload), text=True,
                                 capture_output=True, timeout=5,
                                 env={**os.environ, "PLUGIN_ROOT": str(plugin_root), "ORB_DATA_DIR": str(self.root / "orb data")})
        self.assertEqual((process.returncode, process.stdout, process.stderr), (0, "{}\n", ""))
        self.assertEqual(len(list((self.root / "orb data" / "events").glob("*.json"))), 1)


if __name__ == "__main__":
    unittest.main()
