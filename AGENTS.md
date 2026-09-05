# Context Orb contributor instructions

- Keep all work scoped to this repository. Never import private Codex transcripts or account credentials.
- This is an unofficial, independent companion, not an OpenAI product.
- Preserve UNKNOWN. Missing, invalid, stale or superseded semantic reviews cannot produce a healthy status.
- Capacity, turn count, duration and compaction count alone do not establish semantic degradation.
- Never present latest activity as the foreground/selected session. Manual pinning must be labeled.
- Keep demo fixtures, hook metadata, and user-requested semantic reviews distinguishable in types and UI.
- Metadata hooks must remain bounded and fail open. Do not block a Codex turn or modify its transcript.
- Do not create, fork, compact, or interrupt a Codex session automatically.
- Keep windowing, telemetry, evaluation, and presentation separate.
- Match the calm floating-orb design in docs/UIUX.md. Support keyboard input and reduced motion.
- Run affected tests and npm run build. Native changes also require cargo test/check when available.
- Release artifacts, Windows runtime acceptance, signing, and plugin listing approval are separate gates.
