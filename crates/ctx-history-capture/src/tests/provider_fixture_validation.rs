use super::support::*;

#[test]
fn provider_import_does_not_reuse_legacy_session_without_source_proof() {
    let temp = tempdir();
    let mut store = Store::open(temp.path().join("work.sqlite")).unwrap();
    let provider = CaptureProvider::Claude;
    let provider_session_id = "unknown-legacy-provider-session";
    let source_format = "provider_format";
    let raw_source_path = temp
        .path()
        .join("unknown-legacy-source.jsonl")
        .display()
        .to_string();
    let occurred_at = DateTime::parse_from_rfc3339("2026-06-23T17:00:01Z")
        .unwrap()
        .with_timezone(&Utc);
    let legacy_session_id = provider_session_uuid(provider, provider_session_id);
    store
        .upsert_session(&Session {
            id: legacy_session_id,
            history_record_id: None,
            parent_session_id: None,
            root_session_id: None,
            capture_source_id: None,
            provider,
            external_session_id: Some(provider_session_id.to_owned()),
            external_agent_id: None,
            agent_type: AgentType::Primary,
            role_hint: Some("primary".to_owned()),
            is_primary: true,
            status: SessionStatus::Imported,
            transcript_blob_id: None,
            started_at: occurred_at,
            ended_at: None,
            timestamps: timestamps(occurred_at),
            sync: provider_sync_metadata(Fidelity::Imported, json!({"legacy": true})),
        })
        .unwrap();

    let summary = import_normalized_provider_captures(
        &mut store,
        ProviderNormalizationResult {
            summary: ProviderImportSummary::default(),
            captures: vec![(
                1,
                provider_collision_capture(
                    provider,
                    provider_session_id,
                    source_format,
                    &raw_source_path,
                    occurred_at,
                ),
            )],
            files_touched: vec![],
        },
        NormalizedProviderImportOptions::default(),
    )
    .unwrap();

    assert_eq!(summary.failed, 0, "{:?}", summary.failures);
    assert_eq!(summary.imported_sessions, 1);
    assert_eq!(
        store
            .get_session(legacy_session_id)
            .unwrap()
            .capture_source_id,
        None
    );
    let source_identity = provider_source_root_identity(provider, source_format, &raw_source_path);
    let source_session_id = provider_source_session_uuid(&source_identity, provider_session_id);
    assert!(store.get_session(source_session_id).is_ok());
    let sessions = store
        .sessions_by_external_session_limited(provider, provider_session_id, 10)
        .unwrap()
        .into_iter()
        .map(|session| session.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        sessions,
        BTreeSet::from([legacy_session_id, source_session_id])
    );
    assert_eq!(
        store.events_for_session(source_session_id).unwrap().len(),
        1
    );
    assert!(store
        .events_for_session(legacy_session_id)
        .unwrap()
        .is_empty());
}

#[test]
fn provider_source_event_seq_keeps_large_provider_indices_distinct() {
    let source_id = Uuid::parse_str("018fe2e4-2266-7000-8000-000000000001").unwrap();

    assert_ne!(
        provider_source_event_seq(source_id, 0),
        provider_source_event_seq(source_id, 1_048_576)
    );
    assert_eq!(
        provider_source_event_seq(source_id, 1_048_576) & 0xffff_ffff,
        1_048_576
    );
}

#[test]
fn native_provider_import_rejects_tool_only_without_real_message() {
    let temp = tempdir();
    let mut store = Store::open(temp.path().join("work.sqlite")).unwrap();
    let provider = CaptureProvider::Claude;
    let mut capture = provider_collision_capture(
        provider,
        "tool-only-native-session",
        "provider_format",
        "/tmp/tool-only-native-session.jsonl",
        DateTime::parse_from_rfc3339("2026-06-23T17:00:01Z")
            .unwrap()
            .with_timezone(&Utc),
    );
    let event = capture.event.as_mut().unwrap();
    event.event_type = EventType::ToolCall;
    event.role = Some(EventRole::Tool);
    event.payload = json!({"text": "tool: shell | status: success"});

    let summary = import_normalized_provider_captures(
        &mut store,
        ProviderNormalizationResult {
            summary: ProviderImportSummary::default(),
            captures: vec![(1, capture)],
            files_touched: vec![],
        },
        NormalizedProviderImportOptions::default(),
    )
    .unwrap();

    assert_eq!(summary.failed, 1, "{:?}", summary.failures);
    assert!(summary.failures[0]
        .error
        .contains("no real conversation message"));
    assert!(store.list_sessions().unwrap().is_empty());
    assert!(store.search_event_hits("tool", 10).unwrap().is_empty());
}

#[test]
fn native_provider_import_skips_mixed_metadata_only_session() {
    let temp = tempdir();
    let mut store = Store::open(temp.path().join("work.sqlite")).unwrap();
    let provider = CaptureProvider::Claude;
    let occurred_at = DateTime::parse_from_rfc3339("2026-06-23T17:00:01Z")
        .unwrap()
        .with_timezone(&Utc);
    let real_capture = provider_collision_capture(
        provider,
        "real-native-session",
        "provider_format",
        "/tmp/mixed-native-session.jsonl",
        occurred_at,
    );
    let mut metadata_only_capture = provider_collision_capture(
        provider,
        "metadata-only-native-session",
        "provider_format",
        "/tmp/mixed-native-session.jsonl",
        occurred_at,
    );
    metadata_only_capture.event = None;
    let metadata_only_touch = provider_collision_file_touch(
        provider,
        "metadata-only-native-session",
        "provider_format",
        "/tmp/mixed-native-session.jsonl",
        occurred_at,
    );

    let summary = import_normalized_provider_captures(
        &mut store,
        ProviderNormalizationResult {
            summary: ProviderImportSummary::default(),
            captures: vec![(1, real_capture), (2, metadata_only_capture)],
            files_touched: vec![(2, metadata_only_touch)],
        },
        NormalizedProviderImportOptions::default(),
    )
    .unwrap();

    assert_eq!(summary.failed, 0, "{:?}", summary.failures);
    assert_eq!(summary.imported_sessions, 1);
    assert_eq!(summary.imported_events, 1);
    assert_eq!(summary.skipped_sessions, 1);
    assert!(store
        .sessions_by_external_session_limited(provider, "metadata-only-native-session", 10)
        .unwrap()
        .is_empty());
    assert_eq!(store.export_archive().unwrap().files_touched.len(), 0);
}
