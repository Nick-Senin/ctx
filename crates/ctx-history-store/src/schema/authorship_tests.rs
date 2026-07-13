use std::fs;

use chrono::{DateTime, Utc};
use ctx_history_core::{
    new_id, Event, EventRole, EventType, MessageAuthorship, MessageProvenance, SyncMetadata,
};
use rusqlite::Connection;

use super::ddl::{table_has_column, CREATE_TABLES_SQL};
use crate::Store;

fn tempdir() -> tempfile::TempDir {
    let root = std::env::current_dir().unwrap().join("target/test-data");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("ctx-authorship-")
        .tempdir_in(root)
        .unwrap()
}

fn event(seq: u64, dedupe_key: Option<&str>) -> Event {
    Event {
        message_provenance: MessageProvenance::default(),
        id: new_id(),
        seq,
        history_record_id: None,
        session_id: None,
        run_id: None,
        event_type: EventType::Message,
        role: Some(EventRole::User),
        occurred_at: DateTime::parse_from_rfc3339("2026-06-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        capture_source_id: None,
        payload: serde_json::json!({"text":"payload must survive"}),
        payload_blob_id: None,
        dedupe_key: dedupe_key.map(str::to_owned),
        sync: SyncMetadata::default(),
    }
}

#[test]
fn reimport_refreshes_provenance_and_is_idempotent() {
    let dir = tempdir();
    let store = Store::open(dir.path().join("work.sqlite")).unwrap();
    let mut event = event(910001, Some("authorship-reimport"));
    let id = store.upsert_event(&event).unwrap();
    event.message_provenance = MessageProvenance {
        authorship: MessageAuthorship::Human,
        evidence: "provider_prompt_log".to_owned(),
        classifier_version: 1,
    };
    assert!(!store.insert_event_if_absent(&event).unwrap());
    assert_eq!(store.get_event(id).unwrap().payload, event.payload);
    assert_eq!(
        store.get_event(id).unwrap().message_provenance,
        event.message_provenance
    );
    assert_eq!(store.upsert_event(&event).unwrap(), id);
    let count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE dedupe_key = 'authorship-reimport'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn reimport_provenance_merge_is_versioned_conservative_and_order_independent() {
    fn provenance(
        authorship: MessageAuthorship,
        evidence: &str,
        version: u32,
    ) -> MessageProvenance {
        MessageProvenance {
            authorship,
            evidence: evidence.into(),
            classifier_version: version,
        }
    }

    let dir = tempdir();
    let store = Store::open(dir.path().join("work.sqlite")).unwrap();
    let mut incoming = event(910002, Some("authorship-version-policy"));
    incoming.message_provenance = provenance(MessageAuthorship::Human, "prompt_log", 1);
    let id = store.upsert_event(&incoming).unwrap();

    incoming.message_provenance = provenance(MessageAuthorship::Unknown, "legacy", 0);
    incoming.payload = serde_json::json!({"text":"duplicate raw payload must not replace source"});
    assert!(!store.insert_event_if_absent(&incoming).unwrap());
    assert_eq!(
        store.get_event(id).unwrap().message_provenance,
        provenance(MessageAuthorship::Human, "prompt_log", 1)
    );
    assert_eq!(
        store.get_event(id).unwrap().payload["text"],
        "payload must survive"
    );

    incoming.message_provenance = provenance(MessageAuthorship::Automated, "synthetic", 1);
    assert!(!store.insert_event_if_absent(&incoming).unwrap());
    assert_eq!(
        store.get_event(id).unwrap().message_provenance,
        provenance(MessageAuthorship::Automated, "synthetic", 1)
    );

    incoming.message_provenance = provenance(MessageAuthorship::Human, "corrected", 2);
    assert!(!store.insert_event_if_absent(&incoming).unwrap());
    assert_eq!(
        store.get_event(id).unwrap().message_provenance,
        incoming.message_provenance
    );

    let reverse_dir = tempdir();
    let reverse = Store::open(reverse_dir.path().join("work.sqlite")).unwrap();
    let mut reverse_event = event(910003, Some("authorship-equal-reverse"));
    reverse_event.message_provenance = provenance(MessageAuthorship::Automated, "synthetic", 1);
    let reverse_id = reverse.upsert_event(&reverse_event).unwrap();
    reverse_event.message_provenance = provenance(MessageAuthorship::Human, "prompt_log", 1);
    assert!(!reverse.insert_event_if_absent(&reverse_event).unwrap());
    assert_eq!(
        reverse.get_event(reverse_id).unwrap().message_provenance,
        provenance(MessageAuthorship::Automated, "synthetic", 1)
    );
}

#[test]
fn fresh_store_exposes_only_confirmed_human_messages() {
    let dir = tempdir();
    let store = Store::open(dir.path().join("work.sqlite")).unwrap();
    assert!(table_has_column(&store.conn, "events", "message_authorship").unwrap());
    store.conn.execute_batch(
        "INSERT INTO events (id, seq, event_type, role, occurred_at_ms) VALUES ('unknown', 900001, 'message', 'user', 0);
         INSERT INTO events (id, seq, event_type, role, occurred_at_ms, message_authorship) VALUES ('human', 900002, 'message', 'user', 1, 'human');",
    ).unwrap();
    let human: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM ctx_human_messages", [], |row| {
            row.get(0)
        })
        .unwrap();
    let all: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM ctx_events WHERE event_seq IN (900001,900002)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((human, all), (1, 2));

    let expected_columns = [
        "ctx_event_id",
        "ctx_session_id",
        "history_record_id",
        "provider",
        "provider_session_id",
        "event_seq",
        "event_type",
        "role",
        "occurred_at_ms",
        "payload_json",
        "fidelity",
        "cwd",
        "source_path",
        "source_format",
        "source_root",
        "source_identity",
        "message_authorship",
        "message_authorship_evidence",
        "message_authorship_classifier_version",
    ];
    for view in ["ctx_events", "ctx_human_messages"] {
        let mut statement = store
            .conn
            .prepare(&format!("PRAGMA table_info({view})"))
            .unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(columns, expected_columns, "{view}");
    }
}

#[test]
fn schema_v47_defaults_authorship_and_requeues_only_authorship_sources() {
    let dir = tempdir();
    let path = dir.path().join("work.sqlite");
    let conn = Connection::open(&path).unwrap();
    let legacy = CREATE_TABLES_SQL
        .replace("    message_authorship TEXT NOT NULL DEFAULT 'unknown' CHECK (message_authorship IN ('human', 'automated', 'unknown')),\n", "")
        .replace("    message_authorship_evidence TEXT NOT NULL DEFAULT 'ambiguous',\n", "")
        .replace("    message_authorship_classifier_version INTEGER NOT NULL DEFAULT 0 CHECK (message_authorship_classifier_version >= 0),\n", "");
    conn.execute_batch(&legacy).unwrap();
    conn.execute_batch(
        "INSERT INTO catalog_sessions (source_path, provider, source_format, source_root, agent_type, file_size_bytes, file_modified_at_ms, cataloged_at_ms, indexed_status) VALUES ('/tmp/s','codex','codex_session_jsonl','/tmp','primary',1,0,0,'indexed');
         INSERT INTO catalog_sessions (source_path, provider, source_format, source_root, agent_type, file_size_bytes, file_modified_at_ms, cataloged_at_ms, indexed_status) VALUES ('/tmp/generic','pi','pi_session_jsonl','/tmp','primary',1,0,0,'indexed');
         INSERT INTO source_import_files (provider, source_format, source_root, source_path, file_size_bytes, file_modified_at_ms, observed_at_ms, indexed_status) VALUES ('opencode','opencode_sqlite','/tmp','/tmp/o',1,0,0,'indexed');
         INSERT INTO source_import_files (provider, source_format, source_root, source_path, file_size_bytes, file_modified_at_ms, observed_at_ms, indexed_status) VALUES ('warp','warp_sqlite','/tmp','/tmp/w',1,0,0,'indexed');
         PRAGMA user_version = 46;",
    ).unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    let statuses: (String, String, String, String) = (
        store
            .conn
            .query_row("SELECT indexed_status FROM catalog_sessions WHERE source_format = 'codex_session_jsonl'", [], |r| {
                r.get(0)
            })
            .unwrap(),
        store
            .conn
            .query_row("SELECT indexed_status FROM source_import_files WHERE source_format = 'opencode_sqlite'", [], |r| {
                r.get(0)
            })
            .unwrap(),
        store
            .conn
            .query_row("SELECT indexed_status FROM catalog_sessions WHERE source_format = 'pi_session_jsonl'", [], |r| r.get(0))
            .unwrap(),
        store
            .conn
            .query_row("SELECT indexed_status FROM source_import_files WHERE source_format = 'warp_sqlite'", [], |r| r.get(0))
            .unwrap(),
    );
    assert_eq!(
        statuses,
        (
            "pending".to_owned(),
            "pending".to_owned(),
            "indexed".to_owned(),
            "indexed".to_owned(),
        )
    );
    let event = event(920001, None);
    store.upsert_event(&event).unwrap();
    assert_eq!(
        store.get_event(event.id).unwrap().message_provenance,
        MessageProvenance::default()
    );
}

#[test]
fn schema_v47_rolls_back_all_changes_when_requeue_fails() {
    let dir = tempdir();
    let conn = Connection::open(dir.path().join("work.sqlite")).unwrap();
    let legacy = CREATE_TABLES_SQL
        .replace("    message_authorship TEXT NOT NULL DEFAULT 'unknown' CHECK (message_authorship IN ('human', 'automated', 'unknown')),\n", "")
        .replace("    message_authorship_evidence TEXT NOT NULL DEFAULT 'ambiguous',\n", "")
        .replace("    message_authorship_classifier_version INTEGER NOT NULL DEFAULT 0 CHECK (message_authorship_classifier_version >= 0),\n", "");
    conn.execute_batch(&legacy).unwrap();
    conn.execute_batch(
        "INSERT INTO catalog_sessions (source_path, provider, source_format, source_root, agent_type, file_size_bytes, file_modified_at_ms, cataloged_at_ms, indexed_status) VALUES ('/tmp/s','codex','codex_session_jsonl','/tmp','primary',1,0,0,'indexed');
         CREATE TRIGGER fail_authorship_requeue BEFORE UPDATE ON catalog_sessions
         BEGIN SELECT RAISE(ABORT, 'forced authorship migration failure'); END;
         PRAGMA user_version = 46;",
    )
    .unwrap();

    assert!(super::authorship_migration::migrate_to_v47(&conn).is_err());
    assert!(!table_has_column(&conn, "events", "message_authorship").unwrap());
    assert!(!table_has_column(&conn, "events", "message_authorship_evidence").unwrap());
    assert!(!table_has_column(&conn, "events", "message_authorship_classifier_version").unwrap());
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        46
    );
    conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").unwrap();
}
