# Context Orb plugin

Version 0.3.0 · optional companion for the separate desktop orb.

Includes the context-health skill, asynchronous metadata-only hooks, and a local artifact checker with bounded report history. Installation does not install the native window or establish official directory approval.

## Use

From the cloned repository root, register and install the included local catalog:

```sh
codex plugin marketplace add . --json
codex plugin add codex-context-orb@personal --json
codex plugin list --json --marketplace personal
```

Review this plugin's definitions through the client's `/hooks` trust flow. Start a new task to load the installed skills; this loading boundary does not imply a context-health recommendation. Installation and hook trust are separate.

After enabling the plugin through your Codex client's installation and trust flow, request `$context-health` in the target session. The skill declares the task scope, source anchors and expected conditions. The Python collector reads explicitly selected workspace files and supplies the results. Exact session identity is required before saving; otherwise the skill answers in the conversation only.

Manually pin the matching session in the desktop. Copying the prompt does not run a check. Results always describe the declared scope as of collection. A literal check cannot prove code behavior or complete context integrity. Restart benefit is not evaluated.

[Plugin installation](https://developers.openai.com/plugins/build/plugins) · [Hooks and trust](https://learn.chatgpt.com/docs/hooks)

## Data and dependencies

Python 3.9+ is required: python3 on macOS, py -3 on Windows. No extra Python package is needed.

- events: lifecycle metadata only, below 4 KiB per snapshot.
- evidence: latest report plus eight historical receipts per session, each at most 64 KiB. They include necessary requirement paraphrases, paths, hashes and checks, which can contain private project information.
- assessments: legacy v1 reports are retained only for historical reference.
- Root: ~/.codex-context-orb or an absolute ORB_DATA_DIR configured consistently for desktop and scripts. Only v2 history has bounded retention; no global cleanup is implemented.

The collector reads only explicitly selected regular UTF-8 workspace files up to 1 MiB. It does not follow symlinks, read private Codex transcripts, persist full file contents, execute code or make network calls. Checks for one source use the same captured bytes. Hashes establish consistency, not authenticity.

Hooks remain metadata-only, output empty JSON and do not inject review instructions. Automatic foreground tracking, background model reviews and system notifications are not implemented.

## Local commands

```sh
python3 scripts/evidence_review.py collect --input /absolute/manifest.json --workspace /absolute/project
python3 scripts/evidence_review.py read --session-id verified-session-id
python3 scripts/evidence_review.py history --session-id verified-session-id
python3 scripts/evidence_review.py validate --input /absolute/report.json
python3 scripts/doctor.py --session-id verified-session-id
```

Use [the manifest schema](scripts/evidence-manifest.schema.json) and [synthetic example](scripts/fixtures/evidence-manifest-valid.json). Manifests cannot supply check results. Store temporary manifests and user reports outside the repository. The report hash can be validated without re-reading files; that is not a fresh artifact check.

The doctor reads only that session's three snapshots and reports each layer separately. It can use the current `CODEX_THREAD_ID` when the argument is omitted. It creates no directories, does not print task content, and does not prove host dispatch or native display. Use the [native reader diagnostic and runtime checklist](../../docs/DISTRIBUTION.md#核对接入) for the next steps.

```sh
python3 scripts/test_hooks.py
python3 scripts/test_assessments.py
python3 scripts/test_evidence_review.py
python3 scripts/test_doctor.py
```

[Evidence contract](../../docs/EVIDENCE-CONTRACT.md) · [Design](../../docs/SEMANTIC-REVIEW.md) · [Distribution](../../docs/DISTRIBUTION.md). Real-client integration, algorithm calibration, platform acceptance, signed installers and marketplace approval remain separate gates.
