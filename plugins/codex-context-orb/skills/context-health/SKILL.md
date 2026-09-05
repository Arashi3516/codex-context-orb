---
name: context-health
description: Inspect Codex Context Orb lifecycle metadata for an explicitly identified session, explain unknown context-health signals, or prepare a user-requested handoff to a fresh session. Use when the user asks about Orb, a crowded Codex conversation, or moving work into a new session. Do not use for account quota or billing.
---

# Context Health

This development-stage plugin records lifecycle metadata for the separate Orb desktop app. It does not know which Codex window is foreground and does not measure semantic clutter.

## Inspect the available evidence

1. Use an exact session identifier already supplied by the runtime or the user. Do not infer the selected session from the newest file, a matching title, or recent activity.
2. Run `scripts/inspect_events.py` from this plugin, with `--session` and that identifier. Resolve the script relative to this file: `../../scripts/inspect_events.py`. Use `python3` on macOS or `py -3` on Windows. The command is read-only.
3. If no exact identifier is available, the command without `--session` can list bounded local evidence. Describe it as an inventory and leave the current-session binding unknown; do not select the first result.
4. Report the observed event, observation age, and source only when a matching snapshot exists. `last_event_name` means most recently received, not the current authoritative execution state: asynchronous hooks may arrive out of order.
5. Preserve `context_used_tokens: null` and `context_window_tokens: null` as unknown. Hooks in this version do not report these measurements. Account limits, cumulative tokens, transcript bytes, and cached tokens are not substitutes for context-window occupancy.

The script reads only Orb's own data under `~/.codex-context-orb/events`, or the absolute root selected with `ORB_DATA_DIR`. Do not inspect transcripts, auth files, unrelated sessions, or application internals to fill missing values.

## Help the user continue in a fresh session

When requested, draft a short handoff using the conversation already available to you:

- Current objective and constraints.
- Decisions that still apply.
- Work completed and evidence actually verified.
- Open questions and the next concrete action.
- Relevant file paths or artifact links already known in this conversation.

Exclude secrets, superseded instructions, repeated logs, and unrelated work. Let the user review the draft. Creating a new session requires a request to do so; preparing a handoff does not itself authorize it. Never archive, clear, compact, interrupt, or modify a session just to improve a health indicator.

## Explain the limits clearly

The hooks run only after Codex's normal review and trust flow. Installation does not establish that hooks ran successfully. A missing snapshot can also mean missing Python, disabled hooks, unsupported runtime behavior, oversized input, or a local write failure. Do not claim the desktop companion is running, foreground tracking works, or a session is healthy without corresponding evidence.

For the current package behavior and commands, read `../../README.md` only when needed.
