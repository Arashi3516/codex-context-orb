# Evidence report v2 (implementation contract)

This is the bounded Phase A implementation of the 2026-09-06 research design.
It records a manually declared task scope and checks explicit local artifacts.
It is always an **as-of report**, never an observation of the complete model input.
No private Codex transcript is opened. No model or network call is made by the verifier.
The UI never derives a restart recommendation from this report version.

## Manifest and CLI

`evidence_review.py collect --input manifest.json --workspace /absolute/project`
reads only artifact paths explicitly listed in the manifest, evaluates literal checks,
and atomically saves the report under `ORB_DATA_DIR` (default `~/.codex-context-orb`).
`validate --input report.json`, `read --session-id ID`, and
`history --session-id ID` validate/read local stored reports without collecting files.

Manifest exact root fields: `schema_version: 1`, `session_id`, `turn_id: string|null`,
`scope`, `sources`, `ledger`, `probes`, `observations`.
Manifest sources contain `id, kind, ref, note` only. Manifest probes contain
`id, item_id, source_id, rule, expected` only. The verifier supplies receipt fields.

All identifiers match `[A-Za-z0-9][A-Za-z0-9_-]{0,127}`. All object shapes reject
unknown/duplicate keys. Booleans cannot satisfy integer fields. Text is Unicode,
without control characters, and bounded in code points. Input/output hard limit is
64 KiB per manifest or report. A history response is limited to 8 reports (at most
8 × 64 KiB plus fixed array overhead). There are at most 16 sources, 24 ledger entries,
32 probes and 8 observations per report.

## Report

Exact root fields:

```ts
interface EvidenceReport {
  schema_version: 2
  source: 'codex-evidence-review'
  report_id: string // lower-case SHA-256 of canonical report excluding report_id
  session_id: string
  turn_id: string | null
  reviewed_at_ms: number // integer safe JS epoch, <= now + 60s
  scope: EvidenceScope
  sources: EvidenceSource[]
  ledger: LedgerItem[]
  probes: ProbeResult[]
  observations: Observation[]
}
interface EvidenceScope {
  mode: 'as_of'
  origin: 'main' | 'unknown' // declared identity, not host attestation
  action_id: string
  goal_id: string // active goal entry in ledger
  next_step: string // 1..500
  coverage: 'declared' | 'partial' // declared scope is not proven completeness
  unknowns: string[] // 0..8, each 1..240
}
interface EvidenceSource {
  id: string
  kind: 'statement' | 'artifact'
  ref: string // 1..240; artifact ref is an explicit workspace-relative file path
  note: string // 1..240; source description, never a complete transcript
  status: 'attested' | 'captured' | 'unavailable'
  sha256: string | null // captured file bytes only
}
interface LedgerItem {
  id: string
  kind: 'goal' | 'constraint' | 'fact' | 'decision' | 'progress'
  text: string // 1..500
  source_ids: string[] // 1..4 exact source ids
  status: 'active' | 'superseded' | 'hypothesis'
  supersedes: string[] // 0..4 ledger ids; targets retained as superseded
  critical: boolean
}
interface ProbeResult {
  id: string
  item_id: string // active ledger item
  source_id: string | null // artifact source for non-manual rules
  rule: 'contains' | 'not_contains' | 'sha256' | 'manual'
  expected: string // 1..240 for literal rules; 64 lower hex for sha256; '' for manual
  result: 'pass' | 'fail' | 'unknown'
  detail: string // 1..240, bounded verifier explanation without raw file contents
}
interface Observation {
  id: string
  item_id: string
  kind: 'goal_drift' | 'constraint_loss' | 'decision_conflict' | 'stale_fact' | 'repeated_work'
  summary: string // 1..240
  status: 'open' | 'resolved'
  recurrence: 'once' | 'after_correction'
  source_ids: string[] // 1..4; recorded commentary, not independent verification
}
```

Canonical hashing: UTF-8 JSON with sorted object keys, compact separators, Unicode
preserved, no floats. The report hash proves snapshot consistency, not truth or host identity.
The report's source, rule receipts and referential integrity are validated in Python and Rust.

## Collection rules

- Statement sources are attested, with no hash. They are manually supplied claims,
  not independently verified user-message capture.
- Artifact sources are bounded regular UTF-8 files (max 1 MiB), relative to the
  explicitly supplied workspace. Reject traversal, absolute paths and symlink/reparse
  components. Do not allow private `.codex` transcript/account paths or `.env` files.
  An unavailable or changed-during-read source yields unavailable + unknown checks.
- Capture each source once per collection and evaluate every check against those
  exact bytes. Non-UTF-8 content is unavailable. Store its hash, never full contents.
- `contains`/`not_contains` are literal substring checks; they do not prove code semantics.
  `sha256` is exact byte identity. `manual` always returns unknown; supplied results
  must never be accepted from a manifest.
- Empty expected strings for literal rules are invalid. Unknown sources, duplicate
  IDs, invalid references, supersession cycles, and active supersession targets fail validation.
  `goal_id` must identify an active goal. Statements may only be attested; captured
  artifacts must have a valid hash; unavailable artifacts must have null hash.
- Non-manual results must be unknown when the referenced source is unavailable.
  Captured non-manual results must be pass/fail. A sha256 result must agree with the
  source hash comparison. Manual source_id must be null, expected empty and result unknown.
  A report alone is not a fresh file check. A changed world requires a new collection.

## Snapshot storage

Latest report: `evidence/<sha256(session_id)>.json`.
History: `evidence/history/<sha256(session_id)>/<report_id>.json`.
History snapshots are immutable; retain the latest 8 per session, with explicit bounded
pruning. Same timestamp + different payload is rejected; older reports cannot overwrite
latest. Recollecting the same declared manifest produces a new timestamp and report.
Use bounded OS locking and atomic replacement, including Windows transient-share retries.
Native inventory is bounded, and exact pinned reads bypass the enumeration cap.

Native commands: `read_evidence_reports({sessionId: null | ID})` and
`read_evidence_history({sessionId: ID})`. Malformed absolute/traversal artifact refs fail
manifest validation; prohibited, missing or unreadable files become unavailable without opening.
Latest is the storage commit point. Archive the old latest before replacing it; history
reads merge the latest report so a crash before archiving the new snapshot cannot hide it.

## Phase A decision policy

The returned UI level is only `healthy` (worded **所列检查通过**), `watch` (**检查未通过**),
or `unknown` (**证据待补齐**). It never returns `handoff`.

- Validate exact session identity and bounded timestamps; no 20-minute timeout or
  compaction-count gate. Later hooks do not erase an as-of receipt; show newer activity
  as a separate recheck notice. No current-state guarantee is made.
- Any actual failed artifact check for an active item produces watch, even with unknowns.
- With no failure: unknown origin, partial coverage, scope unknowns, unavailable active
  sources, active hypotheses, open unverified observations, or unknown probes yield unknown.
- Every active critical constraint needs a captured artifact probe result; missing
  verification remains unknown. Every active constraint must be covered for a pass label.
- At least one actual artifact check must pass before showing a pass label. Statement
  declarations are still labeled as such, including when all listed checks pass.
- Superseded entries do not enter the current checklist, but are retained visibly.
- Restart benefit is always `not_evaluated`. An editable task-state brief may be used
  in the same conversation or a new one; it does not imply a recommendation.

Legacy schema v1 reviews are displayed only for historical reference. Their compression
counts, self-rated confidence and reference counts no longer produce active advice.
