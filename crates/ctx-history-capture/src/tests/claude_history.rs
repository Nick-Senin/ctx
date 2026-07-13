use super::support::*;
use crate::{import_claude_history_jsonl, ClaudeHistoryImportOptions};

fn write_history(path: &Path, rows: &[Value]) {
    let body = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(path, body).unwrap();
}

fn import(path: &Path, store: &mut Store) -> ProviderImportSummary {
    import_claude_history_jsonl(
        path,
        store,
        ClaudeHistoryImportOptions {
            source_path: Some(path.to_path_buf()),
            imported_at: "2026-07-10T12:00:00Z".parse().unwrap(),
            ..ClaudeHistoryImportOptions::default()
        },
    )
    .unwrap()
}

#[test]
fn claude_history_cross_owner_copy_dedupes_by_native_record_identity() {
    let temp = tempdir();
    let fixture = provider_history_fixture("claude-history.jsonl");
    let root = temp.path().join("root-history.jsonl");
    let j2h4u = temp.path().join("j2h4u-history.jsonl");
    fs::copy(&fixture, &root).unwrap();
    fs::copy(&fixture, &j2h4u).unwrap();
    let mut store = Store::open(temp.path().join("work.sqlite")).unwrap();

    let first = import(&root, &mut store);
    assert_eq!(first.imported_events, 2);
    let second = import(&j2h4u, &mut store);
    assert_eq!(second.imported_events, 0);
    assert_eq!(second.skipped_events, 2);

    let session_id = stored_provider_session_id(
        &store,
        CaptureProvider::Claude,
        "history:00000000-0000-4000-8000-000000000001",
    );
    let events = store.events_for_session(session_id).unwrap();
    assert_eq!(events.len(), 2, "different timestamps remain distinct");
    assert_ne!(events[0].id, events[1].id);
}

#[test]
fn claude_history_confirms_instruction_lookalike_and_preserves_pastes_honestly() {
    let temp = tempdir();
    let path = provider_history_fixture("claude-history.jsonl");
    let mut store = Store::open(temp.path().join("work.sqlite")).unwrap();
    import(&path, &mut store);
    let session_id = stored_provider_session_id(
        &store,
        CaptureProvider::Claude,
        "history:00000000-0000-4000-8000-000000000001",
    );
    let events = store.events_for_session(session_id).unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| {
        event.message_provenance.authorship == MessageAuthorship::Human
            && event.message_provenance.evidence == "provider_prompt_log"
    }));
    let full_paste = events
        .iter()
        .find(|event| event.sync.metadata["metadata"]["paste_fidelity"] == "full")
        .unwrap();
    let hash_only = events
        .iter()
        .find(|event| event.sync.metadata["metadata"]["paste_fidelity"] == "hash_only")
        .unwrap();
    assert_eq!(
        full_paste.payload["body"]["timestamp_ms"],
        1783684800123_i64
    );
    assert_eq!(
        full_paste.payload["body"]["project"],
        "/workspace/sanitized-project"
    );
    assert_eq!(
        full_paste.payload["body"]["session_id"],
        "00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(
        full_paste.payload["body"]["pasted_contents"]["1"]["content"],
        "sanitized pasted operator text"
    );
    assert_eq!(
        hash_only.payload["body"]["pasted_contents"]["1"]["contentHash"],
        "sanitized-content-hash"
    );
    assert!(hash_only.payload["body"]["pasted_contents"]["1"]
        .get("content")
        .is_none());
}

#[test]
fn claude_history_orphan_session_is_stable_across_reimport_and_rebuild() {
    let temp = tempdir();
    let path = temp.path().join("history.jsonl");
    write_history(
        &path,
        &[json!({
            "display": "orphan prompt", "pastedContents": {},
            "timestamp": 1783684800789_i64, "project": "/orphan-project"
        })],
    );
    let mut first_store = Store::open(temp.path().join("first.sqlite")).unwrap();
    let first = import(&path, &mut first_store);
    assert_eq!(first.imported_events, 1);
    let orphan = format!("history-orphan:{:016x}", crate::fnv1a64(b"/orphan-project"));
    let first_session = stored_provider_session_id(&first_store, CaptureProvider::Claude, &orphan);
    let first_event = first_store.events_for_session(first_session).unwrap()[0].clone();
    let again = import(&path, &mut first_store);
    assert_eq!(again.imported_events, 0);
    assert_eq!(again.skipped_events, 1);

    let mut rebuilt_store = Store::open(temp.path().join("rebuilt.sqlite")).unwrap();
    import(&path, &mut rebuilt_store);
    let rebuilt_session =
        stored_provider_session_id(&rebuilt_store, CaptureProvider::Claude, &orphan);
    let rebuilt_event = rebuilt_store.events_for_session(rebuilt_session).unwrap()[0].clone();
    assert_eq!(first_session, rebuilt_session);
    assert_eq!(first_event.id, rebuilt_event.id);
    assert_eq!(first_event.occurred_at, rebuilt_event.occurred_at);
}

#[test]
fn claude_history_mixed_input_keeps_valid_prompts_without_advancing_cursor() {
    let temp = tempdir();
    let path = temp.path().join("history.jsonl");
    let valid = json!({
        "display": "valid before malformed",
        "pastedContents": {},
        "timestamp": 1783684800789_i64,
        "project": "/mixed-project",
        "sessionId": "00000000-0000-4000-8000-000000000002",
    });
    fs::write(&path, format!("{valid}\n{{malformed\n")).unwrap();
    let mut store = Store::open(temp.path().join("work.sqlite")).unwrap();
    let options = ClaudeHistoryImportOptions {
        machine_id: "test-machine".to_owned(),
        source_path: Some(path.clone()),
        imported_at: "2026-07-10T12:00:00Z".parse().unwrap(),
        ..ClaudeHistoryImportOptions::default()
    };

    let first = import_claude_history_jsonl(&path, &mut store, options.clone()).unwrap();
    assert_eq!(first.imported_events, 1);
    assert_eq!(first.failed, 1);
    assert_eq!(first.failures.len(), 1);
    let source_key =
        "provider-source:claude:claude_history_jsonl:history:00000000-0000-4000-8000-000000000002";
    let cursor_identity = serde_json::to_string(&(
        "provider-source-cursor-v1",
        "claude",
        "claude_history_jsonl",
        "idempotency_key",
        source_key,
    ))
    .unwrap();
    let cursor_stream = format!(
        "provider:claude:claude_history_jsonl:source:{}",
        stable_capture_uuid(&cursor_identity, "provider-cursor-source").simple()
    );
    assert!(store
        .get_sync_cursor(None, "test-machine", &cursor_stream)
        .unwrap()
        .is_none());

    let corrected = json!({
        "display": "corrected prompt",
        "pastedContents": {},
        "timestamp": 1783684800790_i64,
        "project": "/mixed-project",
        "sessionId": "00000000-0000-4000-8000-000000000002",
    });
    write_history(&path, &[valid, corrected]);
    let retry = import_claude_history_jsonl(&path, &mut store, options).unwrap();
    assert_eq!(retry.failed, 0);
    assert_eq!(retry.imported_events, 1);
    assert_eq!(retry.skipped_events, 1);
    assert!(store
        .get_sync_cursor(None, "test-machine", &cursor_stream)
        .unwrap()
        .is_some());
}
