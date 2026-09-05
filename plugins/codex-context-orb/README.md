# Context Orb plugin

Version 0.2.0 · optional companion for the separate desktop orb.

Contains a context-health skill, asynchronous metadata-only lifecycle hooks and a bounded local assessment store. Installation does not install the native window or prove official directory approval.

## Use

After enabling the plugin through your Codex client's installation and trust flow, request `$context-health` in the target session. The skill reviews visible goals, constraints and recent actions, checks counterexamples, then saves a minimal report only when current session identity is verified. Otherwise it responds in the conversation only.

Manually pin the matching session in the desktop. Copying a review prompt does not execute it. Automatic foreground tracking, background AI reviews and system notifications are not implemented.

[Plugin installation](https://developers.openai.com/plugins/build/plugins) · [Hooks and trust](https://learn.chatgpt.com/docs/hooks)

## Data and dependencies

Python 3.9+ is required: python3 on macOS, py -3 on Windows. No additional Python package is required.

- events: one atomic snapshot per session below 4 KiB, containing lifecycle metadata only.
- assessments: up to 32 KiB per session, containing minimal goal, next step, issue paraphrases and source references. These may contain private project information.
- Both use ~/.codex-context-orb or an absolute ORB_DATA_DIR consistently configured for desktop and scripts. No automatic retention cleanup is implemented.

The desktop uploads no reports and adds no external AI calls. The skill uses the user's existing Codex model and visible context. Hooks output empty JSON, never inject self-review instructions, and cannot establish authoritative event order or running state.

## Independent checks

```sh
python3 scripts/test_hooks.py
python3 scripts/test_assessments.py
python3 scripts/inspect_events.py --session thr-example
python3 scripts/assessment_store.py read --session thr-example
```

The inspector is bounded/read-only and never chooses a foreground session. Store write requires exact ID and UTF-8 JSON on stdin, validates [assessment.schema.json](scripts/assessment.schema.json), serializes per-session writes and rejects old/conflicting reports. Fixtures are synthetic.

[Semantic design](../../docs/SEMANTIC-REVIEW.md) · [Distribution](../../docs/DISTRIBUTION.md). Rule accuracy, real-client integration, platform acceptance, signed installers and marketplace approval remain separate gates.
