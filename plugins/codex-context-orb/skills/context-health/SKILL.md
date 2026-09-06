---
name: context-health
description: Check the current task's declared constraints against selected local artifacts, save a requested Context Orb evidence receipt, or prepare a sourced next-step brief. Use when context conflicts or stale decisions may affect the next action. Not for account quota or token occupancy.
---

# Context Health

Produce a bounded, as-of evidence receipt. Current checks establish literal file conditions; they do not measure model entropy, prove all context is intact, or evaluate the benefit of restarting.

## Declare the scope

Use the conversation already visible and task-relevant authorized artifacts. Verify the exact current session ID from runtime context before saving. A supplied ID must match; never infer identity from titles or latest activity. Without verified identity, answer in the conversation only. Use the current turn ID or null, never an old hook's turn. Set scope.origin to main only when the review comes from the current main session; otherwise unknown.

Identify the goal, next action, active constraints, facts, decisions and progress. Preserve superseded entries with explicit replacement links. Each entry needs a real source anchor and a short paraphrase; do not invent message IDs. Statement anchors are declarations, not independent captures of user messages. Mark unresolved assumptions as hypothesis, scope gaps as partial/unknowns. Do not silently drop a hard-to-check constraint to obtain a pass.

Do not open private transcript, account or credential files, other sessions, or application internals. Do not store raw conversations, secrets or long logs. Even short paraphrases and paths may be private; keep user reports outside the repository.

## Define checks before collection

Read [the manifest schema](../../scripts/evidence-manifest.schema.json) and [synthetic example](../../scripts/fixtures/evidence-manifest-valid.json). Resolve these and the scripts from this skill's location.

Choose explicit relative artifact paths within the task workspace. Derive expected values from the requirement or an independently established baseline, not by copying the observed file content to make it pass. Use contains/not_contains for literal text and sha256 for byte identity. These cannot establish behavioral correctness. For requirements needing execution or human judgment, use manual with source_id null and expected empty; its result remains unknown. External test outcomes described in a file are still only file evidence here.

The manifest contains no result, detail, hash, timestamp or report_id fields. The collector supplies these from the bytes it reads. Each source is read once; all its probes use those same bytes. Missing, prohibited, changed, non-UTF-8 or oversized files produce unknown. The collector does not execute code or follow symlinks.

Observations about goal drift, lost constraints, conflicting decisions, stale facts or repeated work are explicitly unverified commentary. Exclude legitimate requirement changes, resolved issues, intentional retests and tool failures before recording a concern. An open observation alone cannot establish contextual degradation or justify restarting.

## Collect and read back

A requested Orb check includes saving a minimal local receipt unless the user requests conversation-only output. Write a UTF-8 manifest in a private temporary location outside the repository, then run:

```text
python3 <plugin-root>/scripts/evidence_review.py collect --input <manifest.json> --workspace <absolute-task-workspace>
python3 <plugin-root>/scripts/evidence_review.py read --session-id <verified-session-id>
```

Supply canonical absolute workspace and temporary-manifest paths; resolve OS temporary-directory aliases before creating the manifest. Artifact refs must remain explicit relative paths with no symlinks. On Windows use py -3 and UTF-8 file or subprocess bytes. Do not interpolate prose into shell code. Remove the temporary manifest after use. Confirm matching session_id and report_id on readback before claiming the Orb was updated. An error means collection or persistence was not confirmed; report that briefly without fabricating a receipt.

Default storage is ~/.codex-context-orb/evidence with the latest snapshot and up to eight historical receipts per session. An absolute ORB_DATA_DIR override must match the desktop process. Input/output is limited to 64 KiB per report, with at most 16 sources, 24 ledger entries, 32 probes, and 8 observations. Files are limited to 1 MiB and only hashes are retained. The report hash checks consistency, not truth or host identity.

## Explain the result

Report failed checks first, then unknowns and the scope of passed checks. Every active constraint and critical entry needs actual file verification for the all-listed-checks-pass label. No compaction-count or elapsed-time threshold changes this result. Later activity does not erase a historical receipt; changed requirements or files need fresh collection.

Restart benefit remains not_evaluated. A next-step brief retains active requirements with source anchors, exact checked file versions, failures and unknowns; clearly separate declarations from verified file conditions and label superseded instructions. The same brief can support repair in the current session or the user's chosen new session. Do not automatically create, fork, archive, clear, compact or interrupt a session. This version has no automatic model review or system notification service.
