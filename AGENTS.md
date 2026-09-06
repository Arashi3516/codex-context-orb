# Context Orb contributor instructions

- Keep all work scoped to this repository. Never import private Codex transcripts or account credentials.
- This is an unofficial, independent companion, not an OpenAI product.
- Preserve UNKNOWN. Missing or invalid reports, unverified requirements, incomplete scope and hypotheses cannot produce an all-listed-checks-pass status.
- Evidence v2 is always as-of. Later activity prompts a recheck; age and compaction count do not erase historical receipts or produce restart advice.
- Separate declared requirements, captured file conditions and unverified observations. A literal check or valid report hash does not prove behavioral correctness or complete context integrity.
- Capacity, turn count, duration and compaction count alone do not establish semantic degradation.
- Never present latest activity as the foreground/selected session. Manual pinning must be labeled.
- Keep demo fixtures, hook metadata, and user-requested semantic reviews distinguishable in types and UI.
- Metadata hooks must remain bounded and fail open. Do not block a Codex turn or modify its transcript.
- Host operations require explicit user authorization, a verified connection to the exact task, applicable evidence, and confirmed operation events. The current reader has no task controller: expose preparation and details only, and never present these as executed host operations.
- Keep windowing, telemetry, evaluation, and presentation separate.
- Match the calm floating-orb design in docs/UIUX.md. Support keyboard input and reduced motion.
- Run affected tests and npm run build. Native changes also require cargo test/check when available.
- Release artifacts, Windows runtime acceptance, signing, and plugin listing approval are separate gates.
