use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

const MAX_BYTES: u64 = 4096;
const MAX_ENTRIES: usize = 512;
const MAX_SNAPSHOTS: usize = 128;

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

fn read_from(directory: &Path) -> Result<Vec<HookSnapshot>, String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Local event directory is not readable.".into()),
    };
    let mut snapshots = Vec::new();
    for entry in entries.take(MAX_ENTRIES).flatten() {
        // Do not follow symlinked event files.
        if !entry.file_type().map(|kind| kind.is_file()).unwrap_or(false) {
            continue;
        }
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else { continue };
        let Some(digest) = filename.strip_suffix(".json") else { continue };
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if entry.metadata().map(|m| m.len() > MAX_BYTES).unwrap_or(true) {
            continue;
        }
        let Ok(file) = fs::File::open(entry.path()) else { continue };
        let mut bytes = Vec::new();
        if file.take(MAX_BYTES + 1).read_to_end(&mut bytes).is_err() || bytes.len() > MAX_BYTES as usize {
            continue;
        }
        let Ok(snapshot) = serde_json::from_slice::<HookSnapshot>(&bytes) else { continue };
        if !valid(&snapshot) { continue }
        let expected = format!("{:x}.json", Sha256::digest(snapshot.session_id.as_bytes()));
        if filename != expected { continue }
        snapshots.push(snapshot);
    }
    snapshots.sort_by(|a, b| b.observed_at_ms.cmp(&a.observed_at_ms));
    snapshots.truncate(MAX_SNAPSHOTS);
    Ok(snapshots)
}

#[tauri::command]
pub fn read_hook_events() -> Result<Vec<HookSnapshot>, String> {
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
    read_from(&root.join("events"))
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
        assert!(read_from(&root.path().join("absent")).unwrap().is_empty());
    }

    #[test]
    fn preserves_unknown_and_never_returns_extra_fields() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "session-a", &fixture("session-a"));
        let mut contaminated = fixture("session-b");
        contaminated["prompt"] = json!("private content");
        write(root.path(), "session-b", &contaminated);
        let snapshots = read_from(root.path()).unwrap();
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
        assert!(read_from(root.path()).unwrap().is_empty());
    }

    #[test]
    fn rejects_large_files_and_bad_models() {
        let root = tempfile::tempdir().unwrap();
        let filename = format!("{:x}.json", Sha256::digest(b"large"));
        fs::write(root.path().join(filename), vec![b' '; 8192]).unwrap();
        let mut fake = fixture("session-a");
        fake["model"] = json!("url with private data");
        write(root.path(), "session-a", &fake);
        assert!(read_from(root.path()).unwrap().is_empty());
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

        let snapshots = read_from(root.path()).unwrap();
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

        let snapshots = read_from(root.path()).unwrap();
        assert_eq!(snapshots.len(), MAX_SNAPSHOTS);
        assert_eq!(snapshots[0].session_id, format!("session-{}", MAX_SNAPSHOTS + 1));
        assert_eq!(snapshots.last().unwrap().session_id, "session-2");
        assert!(snapshots.windows(2).all(|pair| pair[0].observed_at_ms > pair[1].observed_at_ms));
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

        let snapshots = read_from(&events).unwrap();
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
        assert!(read_from(root.path()).unwrap().is_empty());
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
        let result = read_from(&events);
        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700)).unwrap();

        // Privileged test runners can bypass Unix directory permissions.
        if access_denied {
            assert!(result.is_err(), "unreadable data must not appear to be absent");
        } else {
            assert!(result.unwrap().is_empty());
        }
    }
}
