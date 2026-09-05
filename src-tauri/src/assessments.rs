use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 32_768;
const MAX_ENTRIES: usize = 512;
const MAX_REPORTS: usize = 128;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const FUTURE_TOLERANCE_MS: u64 = 60_000;
const ROOT_KEYS: [&str; 11] = [
    "schema_version", "source", "session_id", "turn_id", "reviewed_at_ms",
    "compactions_observed", "coverage", "current_goal", "next_step", "review_note", "signals",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticAssessment {
    schema_version: u8,
    source: String,
    session_id: String,
    turn_id: Option<String>,
    reviewed_at_ms: u64,
    compactions_observed: Option<u16>,
    coverage: String,
    current_goal: String,
    next_step: String,
    review_note: String,
    signals: Vec<Signal>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Signal {
    id: String,
    kind: String,
    summary: String,
    status: String,
    after_compaction: bool,
    affects_next_step: bool,
    recurrence: String,
    confidence: String,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    r#ref: String,
    note: String,
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn text(value: &str, minimum: usize, maximum: usize, multiline: bool) -> bool {
    let length = value.chars().count();
    (minimum..=maximum).contains(&length)
        && value.chars().all(|c| !c.is_control() || (multiline && c == '\n'))
}

fn valid(report: &SemanticAssessment, now_ms: u64) -> bool {
    report.schema_version == 1
        && report.source == "codex-skill-review"
        && identifier(&report.session_id)
        && report.turn_id.as_deref().map(identifier).unwrap_or(true)
        && report.reviewed_at_ms <= MAX_SAFE_INTEGER
        && report.reviewed_at_ms <= now_ms.saturating_add(FUTURE_TOLERANCE_MS)
        && report.compactions_observed.map(|n| n <= 10_000).unwrap_or(true)
        && ["sufficient", "partial"].contains(&report.coverage.as_str())
        && text(&report.current_goal, 1, 500, false)
        && text(&report.next_step, 1, 500, false)
        && text(&report.review_note, 0, 1000, true)
        && report.signals.len() <= 8
        && report.signals.iter().all(|signal| {
            identifier(&signal.id)
                && ["goal_drift", "constraint_loss", "decision_conflict", "stale_fact", "repeated_work"].contains(&signal.kind.as_str())
                && text(&signal.summary, 1, 240, false)
                && ["open", "resolved"].contains(&signal.status.as_str())
                && ["once", "after_correction"].contains(&signal.recurrence.as_str())
                && ["low", "medium", "high"].contains(&signal.confidence.as_str())
                && (1..=4).contains(&signal.evidence.len())
                && signal.evidence.iter().all(|item| text(&item.r#ref, 1, 160, false) && text(&item.note, 1, 500, false))
        })
}

fn parse_report(bytes: &[u8], now_ms: u64) -> Result<SemanticAssessment, String> {
    if bytes.len() > MAX_BYTES as usize {
        return Err("Assessment exceeds the 32 KiB limit.".into());
    }
    // Option fields must be present as explicit null; serde alone would accept absence.
    let envelope: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "Assessment is not valid UTF-8 JSON.")?;
    let fields = envelope.as_object().ok_or("Assessment must be an object.")?;
    if fields.len() != ROOT_KEYS.len() || !ROOT_KEYS.iter().all(|key| fields.contains_key(*key)) {
        return Err("Assessment has missing or unsupported fields.".into());
    }
    // Struct deserialization also rejects duplicate fields and nested unknown fields.
    let report: SemanticAssessment = serde_json::from_slice(bytes)
        .map_err(|_| "Assessment fields do not match the supported schema.")?;
    if !valid(&report, now_ms) {
        return Err("Assessment values or review time are invalid.".into());
    }
    Ok(report)
}

fn plain_directory(path: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("Assessment data directory is not readable.".into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Assessment data directory must be a regular directory, not a link.".into());
    }
    Ok(true)
}

fn filename(session_id: &str) -> String {
    format!("{:x}.json", Sha256::digest(session_id.as_bytes()))
}

fn read_report(path: &Path, session_id: Option<&str>, now_ms: u64) -> Result<Option<SemanticAssessment>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Assessment file is not readable.".into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Assessment file must be regular, not a link or device.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("Assessment exceeds the 32 KiB limit.".into());
    }
    let file = fs::File::open(path).map_err(|_| "Assessment file is not readable.")?;
    let opened = file.metadata().map_err(|_| "Assessment file metadata is not readable.")?;
    if !opened.is_file() {
        return Err("Assessment file changed while opening.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
            return Err("Assessment file changed while opening.".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| "Assessment file is not readable.")?;
    let report = parse_report(&bytes, now_ms)?;
    if path.file_name().and_then(|s| s.to_str()) != Some(filename(&report.session_id).as_str())
        || session_id.map(|id| id != report.session_id).unwrap_or(false) {
        return Err("Assessment file does not match the requested session.".into());
    }
    Ok(Some(report))
}

fn read_from(root: &Path, session_id: Option<&str>, now_ms: u64) -> Result<Vec<SemanticAssessment>, String> {
    if session_id.map(|id| !identifier(id)).unwrap_or(false) {
        return Err("An exact bounded session identifier is required.".into());
    }
    if !root.is_absolute() {
        return Err("Assessment data root must be absolute.".into());
    }
    if !plain_directory(root)? {
        return Ok(Vec::new());
    }
    let directory = root.join("assessments");
    if !plain_directory(&directory)? {
        return Ok(Vec::new());
    }
    if let Some(id) = session_id {
        // Exact selection must not disappear behind an inventory limit.
        return Ok(read_report(&directory.join(filename(id)), Some(id), now_ms)?.into_iter().collect());
    }
    let entries = fs::read_dir(&directory).map_err(|_| "Assessment data directory is not readable.")?;
    let mut reports = Vec::new();
    for entry in entries.take(MAX_ENTRIES).flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(digest) = name.strip_suffix(".json") else { continue };
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(Some(report)) = read_report(&entry.path(), None, now_ms) {
            reports.push(report);
        }
    }
    reports.sort_by(|a, b| b.reviewed_at_ms.cmp(&a.reviewed_at_ms).then(a.session_id.cmp(&b.session_id)));
    reports.truncate(MAX_REPORTS);
    Ok(reports)
}

#[tauri::command]
pub fn read_semantic_assessments(session_id: Option<String>) -> Result<Vec<SemanticAssessment>, String> {
    let root = match std::env::var_os("ORB_DATA_DIR") {
        Some(path) => PathBuf::from(path),
        None => dirs::home_dir().ok_or("Home directory unavailable.")?.join(".codex-context-orb"),
    };
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_| "System clock is unavailable.")?.as_millis();
    let now_ms = u64::try_from(now_ms).map_err(|_| "System clock is invalid.")?;
    read_from(&root, session_id.as_deref(), now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE: &str = include_str!("../../plugins/codex-context-orb/scripts/fixtures/assessment-valid.json");
    const NOW: u64 = 1_000_000;

    fn fixture(id: &str) -> serde_json::Value {
        let mut value: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        value["session_id"] = json!(id);
        value["reviewed_at_ms"] = json!(NOW);
        value
    }

    fn write(root: &Path, id: &str, value: &serde_json::Value) -> PathBuf {
        let directory = root.join("assessments");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(filename(id));
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }

    #[test]
    fn shared_fixture_and_explicit_nulls_are_supported() {
        let report: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        assert!(parse_report(FIXTURE.as_bytes(), report["reviewed_at_ms"].as_u64().unwrap()).is_ok());
        let mut value = fixture("session-a");
        value["turn_id"] = serde_json::Value::Null;
        value["compactions_observed"] = serde_json::Value::Null;
        value["signals"] = json!([]);
        assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_ok());
        value.as_object_mut().unwrap().remove("turn_id");
        assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_err());
    }

    #[test]
    fn rejects_wrong_types_unknown_fields_and_nested_changes() {
        for patch in [
            json!({"schema_version": true}), json!({"reviewed_at_ms": 1.0}),
            json!({"reviewed_at_ms": MAX_SAFE_INTEGER + 1}), json!({"compactions_observed": true}),
            json!({"compactions_observed": 10_001}), json!({"coverage": "authoritative"}),
            json!({"entropy_score": 0.9}), json!({"session_id": "../escape"}),
        ] {
            let mut value = fixture("session-a");
            value.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_err());
        }
        let mut value = fixture("session-a");
        value["signals"][0]["evidence"][0]["private_body"] = json!("synthetic rejected text");
        assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_err());
        let mut value = fixture("session-a");
        value["signals"][0]["confidence"] = json!("certain");
        assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_err());
        let raw = serde_json::to_string(&fixture("session-a")).unwrap();
        let duplicate = raw.replacen("{", "{\"schema_version\":1,", 1);
        assert!(parse_report(duplicate.as_bytes(), NOW).is_err());
    }

    #[test]
    fn text_lengths_controls_and_future_boundary_match_the_contract() {
        let mut value = fixture("session-a");
        value["current_goal"] = json!("界🟢".repeat(250));
        value["reviewed_at_ms"] = json!(NOW + FUTURE_TOLERANCE_MS);
        assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_ok());
        value["reviewed_at_ms"] = json!(NOW + FUTURE_TOLERANCE_MS + 1);
        assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_err());
        value["reviewed_at_ms"] = json!(NOW);
        value["current_goal"] = json!("界".repeat(501));
        assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_err());
        for control in ['\0', '\u{1f}', '\u{7f}', '\u{85}', '\t', '\r', '\n'] {
            value["current_goal"] = json!(format!("goal{control}"));
            assert!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_err());
            value["current_goal"] = json!("goal");
            value["review_note"] = json!(format!("note{control}"));
            assert_eq!(parse_report(&serde_json::to_vec(&value).unwrap(), NOW).is_ok(), control == '\n');
        }
    }

    #[test]
    fn exact_read_bypasses_inventory_limit_and_keeps_identity() {
        let root = tempfile::tempdir().unwrap();
        let target = write(root.path(), "selected-session", &fixture("selected-session"));
        for index in 0..MAX_ENTRIES + 1 {
            fs::write(target.parent().unwrap().join(format!("junk-{index}")), b"{}").unwrap();
        }
        let reports = read_from(root.path(), Some("selected-session"), NOW).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].session_id, "selected-session");
        assert!(read_from(root.path(), Some("missing-session"), NOW).unwrap().is_empty());
        assert!(read_from(root.path(), Some("../escape"), NOW).is_err());
        write(root.path(), "selected-session", &fixture("different-session"));
        assert!(read_from(root.path(), Some("selected-session"), NOW).is_err());
    }

    #[test]
    fn rejects_large_or_corrupt_reports_and_caps_inventory() {
        let root = tempfile::tempdir().unwrap();
        let path = write(root.path(), "oversized", &fixture("oversized"));
        fs::write(path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(read_from(root.path(), Some("oversized"), NOW).is_err());
        let path = write(root.path(), "invalid-utf8", &fixture("invalid-utf8"));
        fs::write(path, [0xff]).unwrap();
        assert!(read_from(root.path(), Some("invalid-utf8"), NOW).is_err());
        for index in 0..MAX_REPORTS + 2 {
            let id = format!("session-{index}");
            let mut value = fixture(&id);
            value["reviewed_at_ms"] = json!(index);
            write(root.path(), &id, &value);
        }
        let reports = read_from(root.path(), None, NOW).unwrap();
        assert_eq!(reports.len(), MAX_REPORTS);
        assert_eq!(reports[0].reviewed_at_ms, (MAX_REPORTS + 1) as u64);
        assert_eq!(reports.last().unwrap().reviewed_at_ms, 2);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_linked_roots_directories_and_files() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let path = write(root.path(), "session-a", &fixture("session-a"));
        let link = root.path().join("root-link");
        symlink(root.path(), &link).unwrap();
        assert!(read_from(&link, Some("session-a"), NOW).is_err());
        let linked_root = tempfile::tempdir().unwrap();
        symlink(path.parent().unwrap(), linked_root.path().join("assessments")).unwrap();
        assert!(read_from(linked_root.path(), None, NOW).is_err());
        let replacement = root.path().join("report.json");
        fs::rename(&path, &replacement).unwrap();
        symlink(replacement, &path).unwrap();
        assert!(read_from(root.path(), Some("session-a"), NOW).is_err());
        assert!(read_from(root.path(), None, NOW).unwrap().is_empty());
    }
}
