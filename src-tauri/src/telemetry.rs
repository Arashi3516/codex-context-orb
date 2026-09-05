use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

const MAX_BYTES: u64 = 4096;
const MAX_ENTRIES: usize = 512;
const MAX_SNAPSHOTS: usize = 128;
const ROOT_KEYS: [&str; 11] = [
    "schema_version", "source", "session_id", "turn_id", "observed_at_ms",
    "last_event_name", "model", "trigger", "context_used_tokens", "context_window_tokens", "binding",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookSnapshot {
    schema_version: u8,
    source: String,
    session_id: String,
    turn_id: Option<String>,
    observed_at_ms: u64,
    last_event_name: String,
    model: Option<String>,
    trigger: Option<String>,
    context_used_tokens: (),
    context_window_tokens: (),
    binding: String,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn valid(snapshot: &HookSnapshot) -> bool {
    let events = ["SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "Stop", "Interrupt"];
    let compact = snapshot.last_event_name == "PreCompact" || snapshot.last_event_name == "PostCompact";
    snapshot.schema_version == 1
        && snapshot.source == "codex-hook"
        && snapshot.binding == "unbound"
        && snapshot.observed_at_ms <= 9_007_199_254_740_991
        && identifier(&snapshot.session_id)
        && snapshot.turn_id.as_deref().map(identifier).unwrap_or(true)
        && events.contains(&snapshot.last_event_name.as_str())
        && snapshot.trigger.as_deref().map(|s| compact && ["manual", "auto"].contains(&s)).unwrap_or(true)
        && snapshot.model.as_deref().map(|s| !s.is_empty() && s.len() <= 96
            && s.as_bytes()[0].is_ascii_alphanumeric()
            && s.chars().all(|c| c.is_ascii_alphanumeric() || "._:-".contains(c))).unwrap_or(true)
}

fn plain_directory(directory: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("Local event directory is not readable.".into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Local event directory must be a regular directory, not a link.".into());
    }
    Ok(true)
}

fn filename(session_id: &str) -> String {
    format!("{:x}.json", Sha256::digest(session_id.as_bytes()))
}

fn read_snapshot(path: &Path, session_id: Option<&str>) -> Result<Option<HookSnapshot>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Local event file is not readable.".into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Local event file must be regular, not a link or device.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("Local event file exceeds the size limit.".into());
    }
    let file = fs::File::open(path).map_err(|_| "Local event file is not readable.")?;
    let opened = file.metadata().map_err(|_| "Local event file metadata is not readable.")?;
    if !opened.is_file() {
        return Err("Local event file changed while opening.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
            return Err("Local event file changed while opening.".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| "Local event file is not readable.")?;
    if bytes.len() > MAX_BYTES as usize {
        return Err("Local event file exceeds the size limit.".into());
    }
    // Nullable fields are required by the wire schema; serde accepts missing Option fields.
    let envelope: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Local event file is not valid UTF-8 JSON.")?;
    let fields = envelope.as_object().ok_or("Local event snapshot must be an object.")?;
    if fields.len() != ROOT_KEYS.len() || !ROOT_KEYS.iter().all(|key| fields.contains_key(*key)) {
        return Err("Local event snapshot has missing or unsupported fields.".into());
    }
    let snapshot: HookSnapshot = serde_json::from_slice(&bytes)
        .map_err(|_| "Local event fields do not match the supported schema.")?;
    if !valid(&snapshot) {
        return Err("Local event snapshot contains invalid values.".into());
    }
    if path.file_name().and_then(|name| name.to_str()) != Some(filename(&snapshot.session_id).as_str())
        || session_id.map(|id| id != snapshot.session_id).unwrap_or(false) {
        return Err("Local event file does not match the requested session.".into());
    }
    Ok(Some(snapshot))
}

fn read_from(directory: &Path, session_id: Option<&str>) -> Result<Vec<HookSnapshot>, String> {
    if session_id.map(|id| !identifier(id)).unwrap_or(false) {
        return Err("An exact bounded session identifier is required.".into());
    }
    if !plain_directory(directory)? {
        return Ok(Vec::new());
    }
    if let Some(id) = session_id {
        // A fixed session must not disappear behind directory or result limits.
        return Ok(read_snapshot(&directory.join(filename(id)), Some(id))?.into_iter().collect());
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Local event directory is not readable.".into()),
    };
    let mut snapshots = Vec::new();
    for entry in entries.take(MAX_ENTRIES).flatten() {
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else { continue };
        let Some(digest) = filename.strip_suffix(".json") else { continue };
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(Some(snapshot)) = read_snapshot(&entry.path(), None) {
            snapshots.push(snapshot);
        }
    }
    snapshots.sort_by(|a, b| b.observed_at_ms.cmp(&a.observed_at_ms));
    snapshots.truncate(MAX_SNAPSHOTS);
    Ok(snapshots)
}

#[tauri::command]
pub fn read_hook_events(session_id: Option<String>) -> Result<Vec<HookSnapshot>, String> {
    if session_id.as_deref().map(|id| !identifier(id)).unwrap_or(false) {
        return Err("An exact bounded session identifier is required.".into());
    }
    let root = match std::env::var_os("ORB_DATA_DIR") {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            if !path.is_absolute() {
                return Err("ORB_DATA_DIR must be absolute.".into());
            }
            path
        }
        None => dirs::home_dir()
            .ok_or("Home directory unavailable.")?
            .join(".codex-context-orb"),
    };
    if !plain_directory(&root)? {
        return Ok(Vec::new());
    }
    read_from(&root.join("events"), session_id.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(id: &str) -> serde_json::Value {
        json!({
            "schema_version": 1, "source": "codex-hook", "session_id": id,
            "turn_id": null, "observed_at_ms": 1000000, "last_event_name": "Stop",
            "model": null, "trigger": null,
            "context_used_tokens": null, "context_window_tokens": null, "binding": "unbound"
        })
    }

    fn write(directory: &Path, id: &str, value: &serde_json::Value) {
        let filename = format!("{:x}.json", Sha256::digest(id.as_bytes()));
        fs::write(directory.join(filename), serde_json::to_vec(value).unwrap()).unwrap();
    }

    #[test]
    fn absent_directory_is_empty() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_from(&root.path().join("absent"), None).unwrap().is_empty());
        assert!(read_from(&root.path().join("absent"), Some("session-a")).unwrap().is_empty());
    }

    #[test]
    fn preserves_unknown_and_never_returns_extra_fields() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "session-a", &fixture("session-a"));
        let mut contaminated = fixture("session-b");
        contaminated["prompt"] = json!("private content");
        write(root.path(), "session-b", &contaminated);
        let snapshots = read_from(root.path(), None).unwrap();
        assert_eq!(snapshots.len(), 1);
        let output = serde_json::to_value(&snapshots[0]).unwrap();
        assert!(output["context_used_tokens"].is_null());
        assert_eq!(output["binding"], "unbound");
    }

    #[test]
    fn rejects_injected_metrics_and_mismatched_identity() {
        let root = tempfile::tempdir().unwrap();
        let mut fake = fixture("session-a");
        fake["context_used_tokens"] = json!(125000);
        write(root.path(), "session-a", &fake);
        write(root.path(), "session-b", &fixture("session-c"));
        assert!(read_from(root.path(), None).unwrap().is_empty());
    }

    #[test]
    fn rejects_large_files_and_bad_models() {
        let root = tempfile::tempdir().unwrap();
        let filename = format!("{:x}.json", Sha256::digest(b"large"));
        fs::write(root.path().join(filename), vec![b' '; 8192]).unwrap();
        let mut fake = fixture("session-a");
        fake["model"] = json!("url with private data");
        write(root.path(), "session-a", &fake);
        assert!(read_from(root.path(), None).unwrap().is_empty());
    }

    #[test]
    fn skips_unicode_filenames_without_losing_valid_snapshots() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "session-a", &fixture("session-a"));
        let unicode_digest = format!("{}é.json", "a".repeat(62));
        let split_character = format!("{}é.jso", "a".repeat(63));
        assert_eq!(unicode_digest.len(), 69);
        assert_eq!(split_character.len(), 69);
        assert!(!split_character.is_char_boundary(64));
        for name in [unicode_digest, split_character] {
            fs::write(root.path().join(name), b"{}").unwrap();
        }

        let snapshots = read_from(root.path(), None).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].session_id, "session-a");
    }

    #[test]
    fn limits_results_to_the_newest_valid_snapshots() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..MAX_SNAPSHOTS + 2 {
            let id = format!("session-{index}");
            let mut value = fixture(&id);
            value["observed_at_ms"] = json!(index);
            write(root.path(), &id, &value);
        }

        let snapshots = read_from(root.path(), None).unwrap();
        assert_eq!(snapshots.len(), MAX_SNAPSHOTS);
        assert_eq!(snapshots[0].session_id, format!("session-{}", MAX_SNAPSHOTS + 1));
        assert_eq!(snapshots.last().unwrap().session_id, "session-2");
        assert!(snapshots.windows(2).all(|pair| pair[0].observed_at_ms > pair[1].observed_at_ms));
    }

    #[test]
    fn exact_read_bypasses_directory_and_result_limits() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..MAX_ENTRIES + 1 {
            let id = format!("session-{index}");
            write(root.path(), &id, &fixture(&id));
        }
        let mut selected = fixture("selected-session");
        selected["observed_at_ms"] = json!(0);
        write(root.path(), "selected-session", &selected);

        let inventory = read_from(root.path(), None).unwrap();
        assert_eq!(inventory.len(), MAX_SNAPSHOTS);
        assert!(inventory.iter().all(|snapshot| snapshot.session_id != "selected-session"));
        let snapshots = read_from(root.path(), Some("selected-session")).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].session_id, "selected-session");
        assert_eq!(snapshots[0].observed_at_ms, 0);
        assert!(read_from(root.path(), Some("missing-session")).unwrap().is_empty());
    }

    #[test]
    fn exact_read_rejects_invalid_identifiers_and_hash_binding() {
        let root = tempfile::tempdir().unwrap();
        for id in ["", "../escape", "session.with-dots", "会话"] {
            assert!(read_from(root.path(), Some(id)).is_err());
            assert!(read_from(&root.path().join("absent"), Some(id)).is_err());
        }
        assert!(read_from(root.path(), Some(&"a".repeat(129))).is_err());
        write(root.path(), "session-a", &fixture("session-b"));
        assert!(read_from(root.path(), Some("session-a")).is_err());
        assert!(read_from(root.path(), None).unwrap().is_empty());
    }

    #[test]
    fn exact_read_rejects_invalid_or_missing_fields_and_unsafe_times() {
        let root = tempfile::tempdir().unwrap();
        for patch in [
            json!({"schema_version": true}), json!({"source": "other-source"}),
            json!({"observed_at_ms": -1}), json!({"observed_at_ms": 1.0}),
            json!({"observed_at_ms": 9_007_199_254_740_992_u64}),
            json!({"context_used_tokens": 1}), json!({"turn_id": "../escape"}),
            json!({"binding": "foreground"}), json!({"trigger": "manual"}),
            json!({"prompt": "synthetic rejected text"}),
        ] {
            let mut value = fixture("session-a");
            value.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            write(root.path(), "session-a", &value);
            assert!(read_from(root.path(), Some("session-a")).is_err());
        }
        for field in ["turn_id", "model", "trigger", "context_used_tokens"] {
            let mut value = fixture("session-a");
            value.as_object_mut().unwrap().remove(field);
            write(root.path(), "session-a", &value);
            assert!(read_from(root.path(), Some("session-a")).is_err());
        }
        let raw = serde_json::to_string(&fixture("session-a")).unwrap();
        fs::write(root.path().join(filename("session-a")), raw.replacen("{", "{\"schema_version\":1,", 1)).unwrap();
        assert!(read_from(root.path(), Some("session-a")).is_err());
    }

    #[test]
    fn exact_read_reports_corrupt_oversized_and_non_file_targets() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(filename("session-a"));
        for bytes in [b"{broken".to_vec(), vec![0xff], vec![b' '; MAX_BYTES as usize + 1]] {
            fs::write(&path, bytes).unwrap();
            assert!(read_from(root.path(), Some("session-a")).is_err());
            assert!(read_from(root.path(), None).unwrap().is_empty());
        }
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(read_from(root.path(), Some("session-a")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn exact_read_rejects_linked_directories_and_files() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let events = root.path().join("events");
        fs::create_dir(&events).unwrap();
        write(root.path(), "session-a", &fixture("session-a"));
        symlink(root.path().join(filename("session-a")), events.join(filename("session-a"))).unwrap();
        assert!(read_from(&events, Some("session-a")).is_err());
        fs::remove_file(root.path().join(filename("session-a"))).unwrap();
        assert!(read_from(&events, Some("session-a")).is_err(), "a dangling link is invalid, not missing");

        let linked_directory = root.path().join("linked-events");
        symlink(&events, &linked_directory).unwrap();
        assert!(read_from(&linked_directory, Some("missing-session")).is_err());
        assert!(read_from(&linked_directory, None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn exact_read_reports_unreadable_files_as_errors() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        write(root.path(), "session-a", &fixture("session-a"));
        let path = root.path().join(filename("session-a"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let access_denied = fs::File::open(&path).is_err();
        let result = read_from(root.path(), Some("session-a"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        // Privileged test runners can bypass Unix file permissions.
        if access_denied {
            assert!(result.is_err(), "unreadable data must not appear to be absent");
        } else {
            assert_eq!(result.unwrap().len(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let events = root.path().join("events");
        fs::create_dir(&events).unwrap();
        write(root.path(), "linked-session", &fixture("linked-session"));
        let filename = format!("{:x}.json", Sha256::digest(b"linked-session"));
        symlink(root.path().join(&filename), events.join(filename)).unwrap();
        write(&events, "session-a", &fixture("session-a"));

        let snapshots = read_from(&events, None).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].session_id, "session-a");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn skips_non_utf8_filenames() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let root = tempfile::tempdir().unwrap();
        let mut invalid_name = vec![b'a'; 63];
        invalid_name.push(0xff);
        invalid_name.extend_from_slice(b".json");
        fs::write(root.path().join(OsString::from_vec(invalid_name)), b"{}").unwrap();
        assert!(read_from(root.path(), None).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn inaccessible_ancestor_is_not_reported_as_an_empty_directory() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let ancestor = root.path().join("private");
        let events = ancestor.join("events");
        fs::create_dir_all(&events).unwrap();
        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o000)).unwrap();
        let access_denied = fs::read_dir(&events).is_err();
        let result = read_from(&events, None);
        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700)).unwrap();

        // Privileged test runners can bypass Unix directory permissions.
        if access_denied {
            assert!(result.is_err(), "unreadable data must not appear to be absent");
        } else {
            assert!(result.unwrap().is_empty());
        }
    }
}
