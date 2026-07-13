use chrono::{DateTime, Utc};
use ctx_history_core::CaptureProvider;

use super::{SourceImportFile, SourceImportFileIndexUpdate};
use crate::{connection::timestamp_ms, Store};

fn tempdir() -> tempfile::TempDir {
    let root = std::env::var_os("TEST_TMPDIR")
        .map(|path| std::path::PathBuf::from(path).join("test-data"))
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("target/test-data"));
    std::fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("ctx-history-store-catalog-authorship-")
        .tempdir_in(root)
        .unwrap()
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-23T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn indexed_file(store: &Store, metadata: serde_json::Value) -> (SourceImportFile, i64) {
    let observed_at_ms = timestamp_ms(fixed_time());
    let file = SourceImportFile {
        provider: CaptureProvider::Claude,
        source_format: "claude_projects_jsonl_tree".into(),
        source_root: "/home/user/.claude/projects".into(),
        source_path: "/home/user/.claude/projects/session.jsonl".into(),
        file_size_bytes: 42,
        file_modified_at_ms: observed_at_ms,
        observed_at_ms,
        metadata,
    };
    store
        .upsert_source_import_files(std::slice::from_ref(&file))
        .unwrap();
    store
        .mark_source_import_file_indexed(
            CaptureProvider::Claude,
            SourceImportFileIndexUpdate {
                source_root: &file.source_root,
                source_path: &file.source_path,
                file_size_bytes: file.file_size_bytes,
                file_modified_at_ms: file.file_modified_at_ms,
                indexed_at_ms: observed_at_ms + 1,
            },
        )
        .unwrap();
    (file, observed_at_ms)
}

#[test]
fn source_import_manifest_upsert_ignores_unrelated_metadata_changes() {
    let temp = tempdir();
    let store = Store::open(temp.path().join("work.sqlite")).unwrap();
    let (mut file, _) = indexed_file(
        &store,
        serde_json::json!({
            "message_authorship_classifier_revision": 1,
            "runtime_observation": "before",
        }),
    );

    file.metadata["runtime_observation"] = serde_json::json!("after");
    file.observed_at_ms += 1;
    store
        .upsert_source_import_files(std::slice::from_ref(&file))
        .unwrap();

    assert!(store
        .list_pending_source_import_files(CaptureProvider::Claude, &file.source_root)
        .unwrap()
        .is_empty());
}

#[test]
fn source_import_classifier_revision_change_marks_same_stat_file_pending() {
    let temp = tempdir();
    let store = Store::open(temp.path().join("work.sqlite")).unwrap();
    let (mut file, _) = indexed_file(
        &store,
        serde_json::json!({"message_authorship_classifier_revision": 1}),
    );

    file.metadata["message_authorship_classifier_revision"] = serde_json::json!(2);
    file.observed_at_ms += 1;
    store
        .upsert_source_import_files(std::slice::from_ref(&file))
        .unwrap();

    assert_eq!(
        store
            .list_pending_source_import_files(CaptureProvider::Claude, &file.source_root)
            .unwrap(),
        vec![file]
    );
}
