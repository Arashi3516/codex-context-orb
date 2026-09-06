//! Inspect the same readers used by the app without opening a window or scanning sessions.
use serde::Serialize;
use serde_json::{json, Value};

fn summarize<T: Serialize>(result: Result<Vec<T>, String>) -> Value {
    match result {
        Err(_) => json!({"status": "UNREADABLE_OR_INVALID", "count": null}),
        Ok(items) => {
            let receipts: Vec<Value> = items.iter().map(|item| {
                let value = serde_json::to_value(item).expect("local receipt is serializable");
                let mut receipt = serde_json::Map::new();
                for key in ["session_id", "turn_id", "report_id", "reviewed_at_ms", "observed_at_ms", "last_event_name"] {
                    if let Some(value) = value.get(key) {
                        receipt.insert(key.into(), value.clone());
                    }
                }
                Value::Object(receipt)
            }).collect();
            json!({"status": if receipts.is_empty() { "ABSENT" } else { "VALID_RECEIPT" }, "count": receipts.len(), "receipts": receipts})
        }
    }
}

pub fn inspect(session_id: &str) -> Result<Value, String> {
    if session_id.is_empty() || session_id.len() > 128
        || !session_id.as_bytes()[0].is_ascii_alphanumeric()
        || !session_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err("An exact bounded session identifier is required.".into());
    }
    let (root, root_source) = match std::env::var_os("ORB_DATA_DIR") {
        Some(path) => (std::path::PathBuf::from(path), "ORB_DATA_DIR"),
        None => (dirs::home_dir().ok_or("Home directory unavailable.")?.join(".codex-context-orb"), "home_default"),
    };
    if !root.is_absolute() {
        return Err("ORB_DATA_DIR must be absolute.".into());
    }
    // Each layer remains useful even if a different layer is absent or invalid.
    Ok(json!({
        "schema_version": 1,
        "source": "context-orb-native-readers",
        "app_version": env!("CARGO_PKG_VERSION"),
        "session_id": session_id,
        "identity": "explicit_argument_not_host_attestation",
        "data_root": root,
        "data_root_source": root_source,
        "layers": {
            "hooks": summarize(crate::telemetry::read_hook_events(Some(session_id.into()))),
            "legacy_assessments": summarize(crate::assessments::read_semantic_assessments(Some(session_id.into()))),
            "evidence": summarize(crate::evidence::read_evidence_reports(Some(session_id.into()))),
            "history": summarize(crate::evidence::read_evidence_history(session_id.into())),
        },
        "host_dispatch": "NOT_VERIFIED",
        "native_ipc": "NOT_VERIFIED",
        "native_ui": "NOT_VERIFIED",
        "context_integrity": "NOT_EVALUATED",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_excludes_user_content_and_keeps_missing_distinct_from_invalid() {
        let summary = summarize(Ok(vec![json!({
            "session_id": "session-a", "report_id": "receipt-a", "reviewed_at_ms": 123,
            "scope": {"next_step": "private next step"}, "sources": [{"ref": "private/file"}],
        })]));
        assert_eq!(summary["receipts"][0], json!({"session_id": "session-a", "report_id": "receipt-a", "reviewed_at_ms": 123}));
        assert_eq!(summarize::<Value>(Ok(vec![]))["status"], "ABSENT");
        let error = summarize::<Value>(Err("private error details".into()));
        assert_eq!(error, json!({"status": "UNREADABLE_OR_INVALID", "count": null}));
    }

    #[test]
    fn rejects_unbounded_or_path_like_identity_before_reading() {
        for id in ["", "../secret", "/absolute", "a b", "中文", &"a".repeat(129)] {
            assert!(inspect(id).is_err());
        }
    }
}
