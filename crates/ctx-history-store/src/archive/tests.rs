use std::{fs, path::Path};

use chrono::{DateTime, Utc};
use ctx_history_core::{
    new_id, Artifact, ArtifactKind, EntityTimestamps, Event, EventRole, EventType, Fidelity,
    MessageAuthorship, MessageProvenance, SessionHistoryArchive, SyncMetadata, SyncState,
    Visibility,
};
use uuid::Uuid;

use crate::archive::{validate_archive_artifact_record_blob, validate_archive_version};
use crate::object_store::{object_relative_path, sha256_hex};
use crate::{Store, StoreError};

fn tempdir() -> tempfile::TempDir {
    let root = std::env::var_os("TEST_TMPDIR")
        .map(|path| std::path::PathBuf::from(path).join("test-data"))
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("target/test-data"));
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("ctx-history-store-archive-validation-")
        .tempdir_in(root)
        .unwrap()
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-23T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn artifact(id: Uuid, blob_hash: String, byte_size: u64) -> Artifact {
    Artifact {
        id,
        kind: ArtifactKind::Markdown,
        blob_path: object_relative_path(&blob_hash),
        blob_hash,
        byte_size,
        media_type: Some("text/markdown".into()),
        preview_text: Some("synthetic local preview blob".into()),
        timestamps: EntityTimestamps {
            created_at: fixed_time(),
            updated_at: fixed_time(),
        },
        source_id: None,
        sync: SyncMetadata {
            visibility: Visibility::LocalOnly,
            fidelity: Fidelity::Imported,
            sync_state: SyncState::LocalOnly,
            sync_version: 0,
            deleted_at: None,
            metadata: serde_json::json!({}),
        },
    }
}

fn write_blob(blob_dir: &Path, blob_hash: &str, content: &[u8]) {
    let path = blob_dir.join(&blob_hash[..2]).join(blob_hash);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn assert_artifact_error(error: StoreError, matches_expected: impl FnOnce(&StoreError) -> bool) {
    assert!(
        matches_expected(&error),
        "unexpected archive artifact validation error: {error:?}"
    );
}

#[test]
fn archive_blob_validation_fails_closed_when_blob_is_missing() {
    let temp = tempdir();
    let content = b"missing synthetic blob";
    let artifact = artifact(new_id(), sha256_hex(content), content.len() as u64);

    let error = validate_archive_artifact_record_blob(temp.path(), &artifact).unwrap_err();
    assert_artifact_error(
        error,
        |error| matches!(error, StoreError::ArchiveArtifactMissingContent { id } if *id == artifact.id),
    );
}

#[test]
fn archive_blob_validation_fails_closed_when_hash_differs() {
    let temp = tempdir();
    let stored_content = b"stored bytes";
    let expected_content = b"expected bytes";
    let artifact = artifact(
        new_id(),
        sha256_hex(expected_content),
        stored_content.len() as u64,
    );
    write_blob(temp.path(), &artifact.blob_hash, stored_content);

    let error = validate_archive_artifact_record_blob(temp.path(), &artifact).unwrap_err();
    assert_artifact_error(
        error,
        |error| matches!(error, StoreError::ArchiveArtifactHashMismatch { id } if *id == artifact.id),
    );
}

#[test]
fn archive_blob_validation_fails_closed_when_byte_size_differs() {
    let temp = tempdir();
    let content = b"size checked bytes";
    let artifact = artifact(new_id(), sha256_hex(content), content.len() as u64 + 1);
    write_blob(temp.path(), &artifact.blob_hash, content);

    let error = validate_archive_artifact_record_blob(temp.path(), &artifact).unwrap_err();
    assert_artifact_error(
        error,
        |error| matches!(error, StoreError::ArchiveArtifactSizeMismatch { id } if *id == artifact.id),
    );
}

#[test]
fn archive_blob_validation_fails_closed_when_blob_path_mismatches_hash() {
    let temp = tempdir();
    let content = b"path checked bytes";
    let mut artifact = artifact(new_id(), sha256_hex(content), content.len() as u64);
    artifact.blob_path = "objects/ff/not-the-recorded-hash".into();
    write_blob(temp.path(), &artifact.blob_hash, content);

    let error = validate_archive_artifact_record_blob(temp.path(), &artifact).unwrap_err();
    assert_artifact_error(
        error,
        |error| matches!(error, StoreError::ArchiveArtifactPathMismatch { id } if *id == artifact.id),
    );
}

#[test]
fn archive_blob_validation_fails_closed_when_blob_is_not_regular_file() {
    let temp = tempdir();
    let content = b"directory at blob path";
    let artifact = artifact(new_id(), sha256_hex(content), content.len() as u64);
    let path = temp
        .path()
        .join(&artifact.blob_hash[..2])
        .join(&artifact.blob_hash);
    fs::create_dir_all(&path).unwrap();

    let error = validate_archive_artifact_record_blob(temp.path(), &artifact).unwrap_err();
    assert_artifact_error(
        error,
        |error| matches!(error, StoreError::ArchiveArtifactNonRegularFile { id, .. } if *id == artifact.id),
    );
}

#[test]
fn archive_version_validation_rejects_future_version() {
    let archive = SessionHistoryArchive {
        schema_version: 3,
        version: 3,
        ..SessionHistoryArchive::default()
    };

    let error = validate_archive_version(&archive).unwrap_err();
    assert!(matches!(
        error,
        StoreError::UnsupportedArchiveVersion(version) if version == 3
    ));
}

#[test]
fn archive_round_trip_preserves_message_provenance() {
    let source_dir = tempdir();
    let source = Store::open(source_dir.path().join("source.sqlite")).unwrap();
    let event = Event {
        message_provenance: MessageProvenance {
            authorship: MessageAuthorship::Human,
            evidence: "provider_prompt_log".to_owned(),
            classifier_version: 1,
        },
        id: new_id(),
        seq: 930001,
        history_record_id: None,
        session_id: None,
        run_id: None,
        event_type: EventType::Message,
        role: Some(EventRole::User),
        occurred_at: fixed_time(),
        capture_source_id: None,
        payload: serde_json::json!({"text":"archive provenance"}),
        payload_blob_id: None,
        dedupe_key: Some("archive-provenance".to_owned()),
        sync: SyncMetadata::default(),
    };
    source.upsert_event(&event).unwrap();
    let archive = source.export_archive().unwrap();

    let destination_dir = tempdir();
    let mut destination = Store::open(destination_dir.path().join("destination.sqlite")).unwrap();
    destination.import_archive(&archive, false).unwrap();
    assert_eq!(
        destination.get_event(event.id).unwrap().message_provenance,
        event.message_provenance
    );
}

#[test]
fn archive_import_uses_versioned_provenance_merge_without_replacing_raw_event() {
    let source_dir = tempdir();
    let source = Store::open(source_dir.path().join("source.sqlite")).unwrap();
    let mut event = Event {
        message_provenance: MessageProvenance {
            authorship: MessageAuthorship::Human,
            evidence: "old_prompt_log".into(),
            classifier_version: 1,
        },
        id: new_id(),
        seq: 930002,
        history_record_id: None,
        session_id: None,
        run_id: None,
        event_type: EventType::Message,
        role: Some(EventRole::User),
        occurred_at: fixed_time(),
        capture_source_id: None,
        payload: serde_json::json!({"text":"archive incoming raw"}),
        payload_blob_id: None,
        dedupe_key: Some("archive-version-policy".into()),
        sync: SyncMetadata::default(),
    };
    source.upsert_event(&event).unwrap();
    let old_archive = source.export_archive().unwrap();

    let destination_dir = tempdir();
    let mut destination = Store::open(destination_dir.path().join("destination.sqlite")).unwrap();
    let mut stored = event.clone();
    stored.payload = serde_json::json!({"text":"destination original raw"});
    stored.message_provenance = MessageProvenance {
        authorship: MessageAuthorship::Automated,
        evidence: "new_classifier".into(),
        classifier_version: 2,
    };
    destination.upsert_event(&stored).unwrap();
    destination.import_archive(&old_archive, true).unwrap();
    let after_old = destination.get_event(event.id).unwrap();
    assert_eq!(after_old.payload, stored.payload);
    assert_eq!(after_old.message_provenance, stored.message_provenance);

    event.message_provenance = MessageProvenance {
        authorship: MessageAuthorship::Human,
        evidence: "explicit_correction".into(),
        classifier_version: 3,
    };
    source.upsert_event(&event).unwrap();
    destination
        .import_archive(&source.export_archive().unwrap(), true)
        .unwrap();
    let corrected = destination.get_event(event.id).unwrap();
    assert_eq!(corrected.payload, stored.payload);
    assert_eq!(corrected.message_provenance, event.message_provenance);
}
