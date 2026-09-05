# Codex Context Orb plugin

Version 0.1.0 is an unofficial development companion for the separate Orb desktop app. The package contains a usable `context-health` skill and a small, asynchronous hook adapter. It does not install a desktop application, measure context occupancy, or detect the currently selected Codex window.

The intended public repository is [Arashi3516/codex-context-orb](https://github.com/Arashi3516/codex-context-orb). Remote publication and release artifacts are separate from the local package validation described here; see [Distribution](../../docs/DISTRIBUTION.md).

## What the hooks do

After normal Codex installation, enablement, and hook trust review, six lifecycle events can update one local metadata snapshot per session: `SessionStart`, `UserPromptSubmit`, `PreCompact`, `PostCompact`, `Stop`, and `Interrupt`.

The adapter accepts at most 64 KiB on stdin and projects the input onto an explicit field allowlist. Although Codex can include prompt or response text in hook input, the adapter never persists that text, reads a transcript, or sends a network request. It writes a JSON file smaller than 4 KiB and returns only `{}` to Codex. Missing/invalid input and ordinary local write failures are advisory no-ops. All configured handlers use `async: true` and a two-second timeout. `SessionEnd` is deliberately omitted because Codex always runs it synchronously. [Official hook behavior](https://learn.chatgpt.com/docs/hooks)

The snapshot is written to:

```text
~/.codex-context-orb/events/<sha256(session_id)>.json
```

On Windows, `~` means the current user's profile. `ORB_DATA_DIR` can select an absolute data root; both the desktop app and hook process must receive the same value. This variable changes Orb's directory only. An invalid relative override disables writes.

Files contain session and optional turn identifiers, the event name, the reported model slug, a compaction trigger when present, and the adapter's observation timestamp. Both context fields remain `null`, and `binding` remains `unbound`. See the [snapshot schema](scripts/hook-event.schema.json) and [architecture](../../docs/ARCHITECTURE.md).

Snapshots replace the previous snapshot for that session atomically. Async callbacks can arrive out of order, so the latest received event is not an authoritative current execution state. This version does not count compactions and has no automatic retention policy across sessions. The read-only inspector limits directory enumeration to 512 entries and snapshot reads to 128 files, reporting partial coverage when necessary.

## Run without installing hooks

Requires Python 3.9 or newer. macOS uses `python3`; Windows uses the Python launcher's `py -3`. The adapter uses only the standard library. Run these commands from the repository root.

Read Orb's existing snapshots on macOS:

```sh
python3 plugins/codex-context-orb/scripts/inspect_events.py
python3 plugins/codex-context-orb/scripts/inspect_events.py --session thr-explicit-selection
```

On Windows:

```powershell
py -3 plugins/codex-context-orb/scripts/inspect_events.py
py -3 plugins/codex-context-orb/scripts/inspect_events.py --session thr-explicit-selection
```

`thr-explicit-selection` illustrates a user-selected identifier; it is not inferred from recent activity. A missing identifier returns an empty result with unknown context status.

Run the adapter's privacy and boundary tests:

```sh
python3 plugins/codex-context-orb/scripts/test_hooks.py
```

Use `py -3` for the same tests on Windows. Tests write only to temporary directories and never access real Codex transcripts or install hooks.

## Install and use the package

See [Distribution](../../docs/DISTRIBUTION.md) for repository installation and the separate public-directory review process. Codex discovers `hooks/hooks.json` at the plugin root, so the manifest does not need a `hooks` field. The hook command reads the installed `PLUGIN_ROOT` environment variable from Python, allowing paths with spaces without depending on PowerShell versus cmd variable syntax. [Official plugin packaging](https://developers.openai.com/plugins/build/plugins)

After the plugin is installed and its hooks are trusted, ask Codex to check context-health evidence or prepare a handoff. The skill can inspect an explicitly identified session and draft a handoff from the conversation already available to the assistant. It does not automatically open, archive, compact, clear, or interrupt a session.

No native Codex UI extension, Pets behavior override, server, login, or API key is included. Live trust/dispatch behavior and Windows execution require separate platform acceptance; passing the local Python tests does not establish those results.
