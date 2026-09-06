//! Read-only validation of local as-of receipts. No artifact or transcript collection.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 65_536;
const MAX_ENTRIES: usize = 512;
const MAX_REPORTS: usize = 128;
const MAX_HISTORY: usize = 8;
const MAX_HISTORY_ENTRIES: usize = 64;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const FUTURE_TOLERANCE_MS: u64 = 60_000;

// Nullable fields must still be explicitly present in every object.
fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where D: Deserializer<'de>, T: Deserialize<'de> {
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReport {
    schema_version: u8,
    source: String,
    report_id: String,
    session_id: String,
    #[serde(deserialize_with = "required_option")]
    turn_id: Option<String>,
    reviewed_at_ms: u64,
    scope: EvidenceScope,
    sources: Vec<EvidenceSource>,
    ledger: Vec<LedgerItem>,
    probes: Vec<ProbeResult>,
    observations: Vec<Observation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceScope {
    mode: String,
    origin: String,
    action_id: String,
    goal_id: String,
    next_step: String,
    coverage: String,
    unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceSource {
    id: String,
    kind: String,
    r#ref: String,
    note: String,
    status: String,
    #[serde(deserialize_with = "required_option")]
    sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerItem {
    id: String,
    kind: String,
    text: String,
    source_ids: Vec<String>,
    status: String,
    supersedes: Vec<String>,
    critical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeResult {
    id: String,
    item_id: String,
    #[serde(deserialize_with = "required_option")]
    source_id: Option<String>,
    rule: String,
    expected: String,
    result: String,
    detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    id: String,
    item_id: String,
    kind: String,
    summary: String,
    status: String,
    recurrence: String,
    source_ids: Vec<String>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn text(value: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&value.chars().count()) && value.chars().all(|c| !c.is_control())
}

fn unique_ids(values: &[String], minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&values.len()) && values.iter().all(|id| identifier(id))
        && values.iter().collect::<HashSet<_>>().len() == values.len()
}

fn artifact_ref(value: &str) -> bool {
    text(value, 1, 240) && !value.contains(['\\', ':']) && value.split('/').all(|part| {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        !["", ".", ".."].contains(&part) && !part.ends_with([' ', '.'])
            && !["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
            && !(stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && (b'1'..=b'9').contains(&stem.as_bytes()[3]))
    })
}

fn prohibited_ref(value: &str) -> bool {
    value.split('/').any(|part| {
        let lower = part.to_ascii_lowercase();
        lower == ".codex" || lower == ".env" || lower.starts_with(".env.")
    })
}

fn acyclic<'a>(id: &'a str, ledger: &HashMap<&'a str, &'a LedgerItem>,
               visiting: &mut HashSet<&'a str>, visited: &mut HashSet<&'a str>) -> bool {
    if visiting.contains(id) { return false; }
    if visited.contains(id) { return true; }
    visiting.insert(id);
    for target in &ledger[id].supersedes {
        if !acyclic(target, ledger, visiting, visited) { return false; }
    }
    visiting.remove(id);
    visited.insert(id);
    true
}

fn valid(report: &EvidenceReport, now_ms: u64) -> bool {
    if report.schema_version != 2 || report.source != "codex-evidence-review"
        || !digest(&report.report_id) || !identifier(&report.session_id)
        || !report.turn_id.as_deref().map(identifier).unwrap_or(true)
        || report.reviewed_at_ms > MAX_SAFE_INTEGER
        || report.reviewed_at_ms > now_ms.saturating_add(FUTURE_TOLERANCE_MS)
        || report.scope.mode != "as_of" || !["main", "unknown"].contains(&report.scope.origin.as_str())
        || !["declared", "partial"].contains(&report.scope.coverage.as_str())
        || !identifier(&report.scope.action_id) || !identifier(&report.scope.goal_id)
        || !text(&report.scope.next_step, 1, 500) || report.scope.unknowns.len() > 8
        || !report.scope.unknowns.iter().all(|value| text(value, 1, 240))
        || report.sources.len() > 16 || report.ledger.len() > 24 || report.probes.len() > 32
        || report.observations.len() > 8 {
        return false;
    }
    let sources: HashMap<&str, &EvidenceSource> = report.sources.iter().map(|source| (source.id.as_str(), source)).collect();
    let ledger: HashMap<&str, &LedgerItem> = report.ledger.iter().map(|item| (item.id.as_str(), item)).collect();
    if sources.len() != report.sources.len() || ledger.len() != report.ledger.len()
        || report.probes.iter().map(|probe| &probe.id).collect::<HashSet<_>>().len() != report.probes.len()
        || report.observations.iter().map(|item| &item.id).collect::<HashSet<_>>().len() != report.observations.len() {
        return false;
    }
    for source in &report.sources {
        if !identifier(&source.id) || !text(&source.r#ref, 1, 240) || !text(&source.note, 1, 240) { return false; }
        match source.kind.as_str() {
            "statement" if source.status == "attested" && source.sha256.is_none() => {},
            "artifact" if artifact_ref(&source.r#ref) && (
                (source.status == "captured" && !prohibited_ref(&source.r#ref) && source.sha256.as_deref().map(digest).unwrap_or(false))
                || (source.status == "unavailable" && source.sha256.is_none())) => {},
            _ => return false,
        }
    }
    for item in &report.ledger {
        if !identifier(&item.id) || !["goal", "constraint", "fact", "decision", "progress"].contains(&item.kind.as_str())
            || !["active", "superseded", "hypothesis"].contains(&item.status.as_str()) || !text(&item.text, 1, 500)
            || !unique_ids(&item.source_ids, 1, 4) || !item.source_ids.iter().all(|id| sources.contains_key(id.as_str()))
            || !unique_ids(&item.supersedes, 0, 4) || !item.supersedes.iter().all(|id| ledger.get(id.as_str()).map(|target| target.status == "superseded").unwrap_or(false)) {
            return false;
        }
    }
    if !ledger.get(report.scope.goal_id.as_str()).map(|item| item.kind == "goal" && item.status == "active").unwrap_or(false) {
        return false;
    }
    let (mut visiting, mut visited) = (HashSet::new(), HashSet::new());
    if !ledger.keys().all(|id| acyclic(id, &ledger, &mut visiting, &mut visited)) { return false; }
    for probe in &report.probes {
        if !identifier(&probe.id) || !identifier(&probe.item_id)
            || !ledger.get(probe.item_id.as_str()).map(|item| item.status == "active").unwrap_or(false)
            || !["pass", "fail", "unknown"].contains(&probe.result.as_str()) || !text(&probe.detail, 1, 240) {
            return false;
        }
        if probe.rule == "manual" {
            if probe.source_id.is_some() || !probe.expected.is_empty() || probe.result != "unknown" { return false; }
            continue;
        }
        if !["contains", "not_contains", "sha256"].contains(&probe.rule.as_str()) { return false; }
        let source = match probe.source_id.as_deref().filter(|id| identifier(id)).and_then(|id| sources.get(id)) {
            Some(source) if source.kind == "artifact" => source,
            _ => return false,
        };
        if (probe.rule == "sha256" && !digest(&probe.expected))
            || (probe.rule != "sha256" && !text(&probe.expected, 1, 240)) { return false; }
        if source.status == "unavailable" {
            if probe.result != "unknown" { return false; }
        } else {
            if !["pass", "fail"].contains(&probe.result.as_str()) { return false; }
            if probe.rule == "sha256" {
                let matches = source.sha256.as_deref() == Some(probe.expected.as_str());
                if (probe.result == "pass") != matches { return false; }
            }
        }
    }
    report.observations.iter().all(|observation| {
        identifier(&observation.id) && identifier(&observation.item_id) && ledger.contains_key(observation.item_id.as_str())
            && ["goal_drift", "constraint_loss", "decision_conflict", "stale_fact", "repeated_work"].contains(&observation.kind.as_str())
            && text(&observation.summary, 1, 240) && ["open", "resolved"].contains(&observation.status.as_str())
            && ["once", "after_correction"].contains(&observation.recurrence.as_str())
            && unique_ids(&observation.source_ids, 1, 4)
            && observation.source_ids.iter().all(|id| sources.contains_key(id.as_str()))
    })
}

fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(map.into_iter().map(|(key, value)| (key, sorted(value)))
            .collect::<BTreeMap<_, _>>().into_iter().collect()),
        Value::Array(values) => Value::Array(values.into_iter().map(sorted).collect()),
        other => other,
    }
}

fn report_hash(report: &EvidenceReport) -> Result<String, String> {
    let mut value = serde_json::to_value(report).map_err(|_| "Evidence cannot be serialized.")?;
    value.as_object_mut().ok_or("Evidence must be an object.")?.remove("report_id");
    let bytes = serde_json::to_vec(&sorted(value)).map_err(|_| "Evidence cannot be serialized.")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn parse_report(bytes: &[u8], now_ms: u64) -> Result<EvidenceReport, String> {
    if bytes.len() > MAX_BYTES as usize { return Err("Evidence exceeds the 64 KiB limit.".into()); }
    // Struct decoding rejects duplicate keys, unknown keys, missing nullable fields and non-integers.
    let report: EvidenceReport = serde_json::from_slice(bytes).map_err(|_| "Evidence fields do not match the supported schema.")?;
    if !valid(&report, now_ms) { return Err("Evidence values, references or receipts are invalid.".into()); }
    if report_hash(&report)? != report.report_id { return Err("Evidence hash does not match its contents.".into()); }
    Ok(report)
}

fn linked(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() { return true; }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return true; }
    }
    false
}

fn plain_directory(path: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("Evidence directory is not readable.".into()),
    };
    if !metadata.is_dir() || linked(&metadata) { return Err("Evidence directory must be regular, not a link or reparse point.".into()); }
    Ok(true)
}

fn session_hash(session_id: &str) -> String {
    format!("{:x}", Sha256::digest(session_id.as_bytes()))
}

fn same_file(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() { return false; }
    }
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
        && after.is_file() && !linked(after)
}

fn read_file(path: &Path, session_id: Option<&str>, history: bool, now_ms: u64) -> Result<Option<EvidenceReport>, String> {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Evidence file is not readable.".into()),
    };
    if !before.is_file() || linked(&before) { return Err("Evidence file must be regular, not a link or device.".into()); }
    if before.len() > MAX_BYTES { return Err("Evidence exceeds the 64 KiB limit.".into()); }
    let mut file = fs::File::open(path).map_err(|_| "Evidence file is not readable.")?;
    let opened = file.metadata().map_err(|_| "Evidence metadata is not readable.")?;
    if !same_file(&before, &opened) { return Err("Evidence changed while opening.".into()); }
    let mut bytes = Vec::new();
    (&mut file).take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| "Evidence file is not readable.")?;
    let after = file.metadata().map_err(|_| "Evidence metadata is not readable.")?;
    let current = fs::symlink_metadata(path).map_err(|_| "Evidence changed during reading.")?;
    if !same_file(&opened, &after) || !same_file(&after, &current) { return Err("Evidence changed during reading.".into()); }
    let report = parse_report(&bytes, now_ms)?;
    let expected_name = format!("{}.json", if history { report.report_id.clone() } else { session_hash(&report.session_id) });
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
        || session_id.map(|id| id != report.session_id).unwrap_or(false) {
        return Err("Evidence filename, hash or session does not match.".into());
    }
    Ok(Some(report))
}

fn directory(root: &Path) -> Result<Option<PathBuf>, String> {
    if !root.is_absolute() { return Err("Evidence data root must be absolute.".into()); }
    if !plain_directory(root)? { return Ok(None); }
    let directory = root.join("evidence");
    Ok(plain_directory(&directory)?.then_some(directory))
}

fn read_from(root: &Path, session_id: Option<&str>, now_ms: u64) -> Result<Vec<EvidenceReport>, String> {
    if session_id.map(|id| !identifier(id)).unwrap_or(false) { return Err("An exact bounded session identifier is required.".into()); }
    let directory = match directory(root)? { Some(directory) => directory, None => return Ok(Vec::new()) };
    if let Some(session_id) = session_id {
        return Ok(read_file(&directory.join(format!("{}.json", session_hash(session_id))), Some(session_id), false, now_ms)?.into_iter().collect());
    }
    let entries = fs::read_dir(&directory).map_err(|_| "Evidence inventory is not readable.")?;
    let mut reports = Vec::new();
    for entry in entries.take(MAX_ENTRIES).flatten() {
        let name = entry.file_name();
        let name = match name.to_str().and_then(|name| name.strip_suffix(".json")) { Some(name) if digest(name) => name, _ => continue };
        if name.len() != 64 { continue; }
        if let Ok(Some(report)) = read_file(&entry.path(), None, false, now_ms) { reports.push(report); }
    }
    reports.sort_by(|a, b| b.reviewed_at_ms.cmp(&a.reviewed_at_ms).then_with(|| a.session_id.cmp(&b.session_id)));
    reports.truncate(MAX_REPORTS);
    Ok(reports)
}

fn history_from(root: &Path, session_id: &str, now_ms: u64) -> Result<Vec<EvidenceReport>, String> {
    if !identifier(session_id) { return Err("An exact bounded session identifier is required.".into()); }
    let directory = match directory(root)? { Some(directory) => directory, None => return Ok(Vec::new()) };
    let mut reports = read_from(root, Some(session_id), now_ms)?;
    let parent = directory.join("history");
    if plain_directory(&parent)? {
        let history = parent.join(session_hash(session_id));
        if plain_directory(&history)? {
            let entries = fs::read_dir(&history).map_err(|_| "Evidence history is not readable.")?;
            for (index, entry) in entries.enumerate() {
                if index >= MAX_HISTORY_ENTRIES { return Err("Evidence history exceeds the bounded inventory; explicit cleanup is required.".into()); }
                let entry = entry.map_err(|_| "Evidence history is not readable.")?;
                if !entry.file_name().to_str().and_then(|name| name.strip_suffix(".json")).map(digest).unwrap_or(false) { continue; }
                if let Some(report) = read_file(&entry.path(), Some(session_id), true, now_ms)? { reports.push(report); }
            }
        }
    }
    reports.sort_by(|a, b| b.reviewed_at_ms.cmp(&a.reviewed_at_ms).then_with(|| b.report_id.cmp(&a.report_id)));
    let mut ids = HashSet::new();
    reports.retain(|report| ids.insert(report.report_id.clone()));
    reports.truncate(MAX_HISTORY);
    Ok(reports)
}

fn root() -> Result<PathBuf, String> {
    let root = match std::env::var_os("ORB_DATA_DIR") {
        Some(value) => PathBuf::from(value),
        None => dirs::home_dir().ok_or("Home directory is unavailable.")?.join(".codex-context-orb"),
    };
    if !root.is_absolute() { return Err("ORB_DATA_DIR must be absolute.".into()); }
    Ok(root)
}

fn now_ms() -> Result<u64, String> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "System time is unavailable.")?.as_millis() as u64)
}

#[tauri::command]
pub fn read_evidence_reports(session_id: Option<String>) -> Result<Vec<EvidenceReport>, String> {
    read_from(&root()?, session_id.as_deref(), now_ms()?)
}

#[tauri::command]
pub fn read_evidence_history(session_id: String) -> Result<Vec<EvidenceReport>, String> {
    history_from(&root()?, &session_id, now_ms()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE: &str = include_str!("../../plugins/codex-context-orb/scripts/fixtures/evidence-report-valid.json");
    const NOW: u64 = 1_788_652_800_000;

    fn fixture(session_id: &str, offset: u64) -> Value {
        let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
        value["session_id"] = json!(session_id);
        value["reviewed_at_ms"] = json!(NOW + offset);
        rehash(&mut value);
        value
    }

    fn rehash(value: &mut Value) {
        let mut unsigned = value.clone();
        unsigned.as_object_mut().unwrap().remove("report_id");
        value["report_id"] = json!(format!("{:x}", Sha256::digest(serde_json::to_vec(&sorted(unsigned)).unwrap())));
    }

    fn accepted(value: &Value) -> bool {
        parse_report(&serde_json::to_vec(value).unwrap(), NOW).is_ok()
    }

    fn write(root: &Path, value: &Value, history: bool) -> PathBuf {
        let id = value["session_id"].as_str().unwrap();
        let directory = if history { root.join("evidence/history").join(session_hash(id)) } else { root.join("evidence") };
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{}.json", if history { value["report_id"].as_str().unwrap().to_owned() } else { session_hash(id) }));
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }

    #[test]
    fn shared_python_fixture_matches_canonical_hash_and_null_contract() {
        let report = parse_report(FIXTURE.as_bytes(), NOW).unwrap();
        assert_eq!(report_hash(&report).unwrap(), report.report_id);
        assert_eq!(report.scope.mode, "as_of");
        assert_eq!(report.ledger[1].source_ids, ["source-statement"]);
        let mut value = fixture("session-a", 0);
        value["turn_id"] = Value::Null;
        rehash(&mut value);
        assert!(accepted(&value));
        value.as_object_mut().unwrap().remove("turn_id");
        rehash(&mut value);
        assert!(!accepted(&value));
        let mut value = fixture("session-a", 0);
        value["sources"][0].as_object_mut().unwrap().remove("sha256");
        rehash(&mut value);
        assert!(!accepted(&value));
        let mut value = fixture("session-a", 0);
        value["probes"][0]["rule"] = json!("manual");
        value["probes"][0]["expected"] = json!("");
        value["probes"][0]["result"] = json!("unknown");
        value["probes"][0]["source_id"] = Value::Null;
        rehash(&mut value);
        assert!(accepted(&value));
        value["probes"][0].as_object_mut().unwrap().remove("source_id");
        rehash(&mut value);
        assert!(!accepted(&value));
    }

    #[test]
    fn rejects_unknown_duplicate_missing_fields_and_wrong_types() {
        for patch in [json!({"schema_version": true}), json!({"schema_version": 2.0}),
            json!({"reviewed_at_ms": 1.0}), json!({"reviewed_at_ms": MAX_SAFE_INTEGER + 1}),
            json!({"source": "host-attested"}), json!({"session_id": "../escape"}),
            json!({"restart_probability": 0.9})] {
            let mut value = fixture("session-a", 0);
            value.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            rehash(&mut value);
            assert!(!accepted(&value));
        }
        let mut value = fixture("session-a", 0);
        value["sources"][1]["contents"] = json!("synthetic rejected body");
        rehash(&mut value);
        assert!(!accepted(&value));
        let raw = serde_json::to_string(&fixture("session-a", 0)).unwrap();
        assert!(parse_report(raw.replacen('{', "{\"schema_version\":2,", 1).as_bytes(), NOW).is_err());
        assert!(parse_report(raw.replacen("\"mode\":\"as_of\"", "\"mode\":\"as_of\",\"mode\":\"as_of\"", 1).as_bytes(), NOW).is_err());
        assert!(parse_report(&vec![b' '; MAX_BYTES as usize + 1], NOW).is_err());
        assert!(parse_report(b"\xff", NOW).is_err());
    }

    #[test]
    fn checks_references_receipt_semantics_and_supersession_cycles() {
        for (pointer, replacement) in [
            ("/scope/goal_id", json!("constraint-status")), ("/scope/origin", json!("attested")),
            ("/ledger/1/source_ids", json!(["unknown"])), ("/ledger/1/source_ids", json!(["source-statement", "source-statement"])),
            ("/ledger/1/status", json!("superseded")), ("/ledger/1/supersedes", json!(["goal-review-status"])),
            ("/probes/0/item_id", json!("unknown")), ("/probes/0/source_id", json!("source-statement")),
            ("/probes/0/result", json!("unknown")), ("/sources/0/status", json!("captured")),
            ("/sources/1/sha256", Value::Null), ("/ledger/1/critical", json!(1))] {
            let mut value = fixture("session-a", 0);
            *value.pointer_mut(pointer).unwrap() = replacement;
            rehash(&mut value);
            assert!(!accepted(&value), "{pointer}");
        }
        let mut value = fixture("session-a", 0);
        let duplicate = value["sources"][0].clone();
        value["sources"].as_array_mut().unwrap().push(duplicate);
        rehash(&mut value);
        assert!(!accepted(&value));
        let mut value = fixture("session-a", 0);
        let mut previous = value["ledger"][1].clone();
        previous["id"] = json!("previous-rule");
        previous["status"] = json!("superseded");
        value["ledger"].as_array_mut().unwrap().push(previous);
        value["ledger"][1]["supersedes"] = json!(["previous-rule"]);
        rehash(&mut value);
        assert!(accepted(&value));
        value["ledger"][2]["supersedes"] = json!(["previous-rule"]);
        rehash(&mut value);
        assert!(!accepted(&value));
    }

    #[test]
    fn manual_unavailable_and_sha_receipts_cannot_claim_false_passes() {
        let mut value = fixture("session-a", 0);
        value["sources"][1]["status"] = json!("unavailable");
        value["sources"][1]["sha256"] = Value::Null;
        rehash(&mut value);
        assert!(!accepted(&value));
        value["probes"][0]["result"] = json!("unknown");
        rehash(&mut value);
        assert!(accepted(&value));
        let mut value = fixture("session-a", 0);
        value["probes"][0]["rule"] = json!("sha256");
        value["probes"][0]["expected"] = value["sources"][1]["sha256"].clone();
        rehash(&mut value);
        assert!(accepted(&value));
        value["probes"][0]["result"] = json!("fail");
        rehash(&mut value);
        assert!(!accepted(&value));
        value["probes"][0]["rule"] = json!("manual");
        value["probes"][0]["expected"] = json!("");
        value["probes"][0]["source_id"] = Value::Null;
        value["probes"][0]["result"] = json!("unknown");
        rehash(&mut value);
        assert!(accepted(&value));
        value["probes"][0]["source_id"] = json!("source-status");
        rehash(&mut value);
        assert!(!accepted(&value));
    }

    #[test]
    fn portable_paths_unicode_and_time_bounds_match_python() {
        for path in ["../escape", "/absolute", "a/../b", "a/./b", "a//b", "a/", "C:/private", "a\\b", "file:stream", "NUL", "con.txt", "dir/file.", "dir/file "] {
            let mut value = fixture("session-a", 0);
            value["sources"][1]["ref"] = json!(path);
            rehash(&mut value);
            assert!(!accepted(&value), "{path}");
        }
        for path in [".env", ".env.example", ".codex/sessions/file.jsonl"] {
            let mut value = fixture("session-a", 0);
            value["sources"][1]["ref"] = json!(path);
            rehash(&mut value);
            assert!(!accepted(&value));
            value["sources"][1]["status"] = json!("unavailable");
            value["sources"][1]["sha256"] = Value::Null;
            value["probes"][0]["result"] = json!("unknown");
            rehash(&mut value);
            assert!(accepted(&value));
        }
        for control in ['\0', '\u{1f}', '\u{7f}', '\u{85}', '\t', '\r', '\n'] {
            let mut value = fixture("session-a", 0);
            value["scope"]["next_step"] = json!(format!("step{control}"));
            rehash(&mut value);
            assert!(!accepted(&value));
        }
        let mut value = fixture("session-a", FUTURE_TOLERANCE_MS);
        value["scope"]["next_step"] = json!("界🟢".repeat(250));
        rehash(&mut value);
        assert!(accepted(&value));
        value["reviewed_at_ms"] = json!(NOW + FUTURE_TOLERANCE_MS + 1);
        rehash(&mut value);
        assert!(!accepted(&value));
    }

    #[test]
    fn exact_pinned_reads_bypass_inventory_cap_and_reject_wrong_identity() {
        let root = tempfile::tempdir().unwrap();
        let value = fixture("pinned-session", 0);
        let path = write(root.path(), &value, false);
        for index in 0..MAX_ENTRIES + 1 { fs::write(path.parent().unwrap().join(format!("junk-{index}")), b"{}").unwrap(); }
        let reports = read_from(root.path(), Some("pinned-session"), NOW).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].session_id, "pinned-session");
        assert!(read_from(root.path(), Some("missing"), NOW).unwrap().is_empty());
        assert!(read_from(root.path(), Some("../escape"), NOW).is_err());
        fs::write(&path, serde_json::to_vec(&fixture("other-session", 0)).unwrap()).unwrap();
        assert!(read_from(root.path(), Some("pinned-session"), NOW).is_err());
    }

    #[test]
    fn bounded_inventory_skips_invalid_entries_and_sorts_valid_reports() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..MAX_REPORTS + 2 { write(root.path(), &fixture(&format!("session-{index}"), index as u64), false); }
        fs::write(root.path().join("evidence").join(format!("{}.json", "é".repeat(32))), b"{}").unwrap();
        let reports = read_from(root.path(), None, NOW).unwrap();
        assert_eq!(reports.len(), MAX_REPORTS);
        assert_eq!(reports[0].reviewed_at_ms, NOW + MAX_REPORTS as u64 + 1);
        assert!(reports.windows(2).all(|pair| pair[0].reviewed_at_ms >= pair[1].reviewed_at_ms));
    }

    #[test]
    fn history_is_exact_bounded_and_includes_latest_after_interrupted_archival() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..10 { write(root.path(), &fixture("session-a", index), true); }
        write(root.path(), &fixture("session-b", 20), true);
        let latest = fixture("session-a", 10);
        write(root.path(), &latest, false);
        let history = history_from(root.path(), "session-a", NOW).unwrap();
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(history[0].report_id, latest["report_id"].as_str().unwrap());
        assert!(history.iter().all(|report| report.session_id == "session-a"));
        write(root.path(), &latest, true);
        assert_eq!(history_from(root.path(), "session-a", NOW).unwrap().len(), MAX_HISTORY);
        let directory = root.path().join("evidence/history").join(session_hash("session-a"));
        for index in 0..MAX_HISTORY_ENTRIES + 1 { fs::write(directory.join(format!("junk-{index}")), b"{}").unwrap(); }
        assert!(history_from(root.path(), "session-a", NOW).is_err());
    }

    #[test]
    fn exact_reads_fail_closed_on_corruption_and_oversize() {
        let root = tempfile::tempdir().unwrap();
        let path = write(root.path(), &fixture("session-a", 0), false);
        fs::write(&path, b"{}").unwrap();
        assert!(read_from(root.path(), Some("session-a"), NOW).is_err());
        assert!(read_from(root.path(), None, NOW).unwrap().is_empty());
        fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(read_from(root.path(), Some("session-a"), NOW).is_err());
        let path = write(root.path(), &fixture("session-b", 0), true);
        fs::write(&path, serde_json::to_vec(&fixture("session-a", 0)).unwrap()).unwrap();
        assert!(history_from(root.path(), "session-b", NOW).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn roots_files_and_history_links_are_rejected() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let external = write(outside.path(), &fixture("session-a", 0), false);
        let linked_root = root.path().join("linked-root");
        symlink(outside.path(), &linked_root).unwrap();
        assert!(read_from(&linked_root, Some("session-a"), NOW).is_err());
        let path = write(root.path(), &fixture("session-a", 0), false);
        fs::remove_file(&path).unwrap();
        symlink(&external, &path).unwrap();
        assert!(read_from(root.path(), Some("session-a"), NOW).is_err());
        assert!(read_from(root.path(), None, NOW).unwrap().is_empty());
        fs::remove_file(&path).unwrap();
        symlink(outside.path(), root.path().join("evidence/history")).unwrap();
        assert!(history_from(root.path(), "session-a", NOW).is_err());
    }
}
