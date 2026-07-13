use super::support::*;
use crate::provider::codex::events::codex_provider_event;
use crate::provider::importer::provider_event_identity_hash;
use crate::provider::providers::opencode_authorship::opencode_message_provenance;
use crate::{message_authorship_classifier_revision, MESSAGE_AUTHORSHIP_CLASSIFIER_REVISION};
use ctx_history_core::MessageProvenance;

#[test]
fn classifier_revision_is_scoped_to_authorship_aware_native_sources() {
    for source_format in [
        "codex_session_jsonl",
        "codex_session_jsonl_tree",
        "codex_history_jsonl",
        "claude_projects_jsonl_tree",
        "claude_history_jsonl",
        "opencode_sqlite",
    ] {
        assert_eq!(
            message_authorship_classifier_revision(source_format),
            Some(MESSAGE_AUTHORSHIP_CLASSIFIER_REVISION),
            "{source_format}"
        );
    }
    for source_format in ["pi_session_jsonl", "kilo_sqlite", "warp_sqlite"] {
        assert_eq!(message_authorship_classifier_revision(source_format), None);
    }
}

#[test]
fn codex_rollout_user_injections_and_human_lookalikes_remain_unknown() {
    for text in [
        "<environment_context>AGENTS.md</environment_context>",
        "skill expansion and approval request",
        "a human pasted <system> literally",
    ] {
        let event = codex_provider_event(
            1,
            "2026-07-01T00:00:00Z".parse().unwrap(),
            EventType::Message,
            Some(EventRole::User),
            json!({"text": text}),
            json!({"source": "codex_session"}),
        );
        assert_eq!(
            event.message_provenance.authorship,
            MessageAuthorship::Unknown
        );
    }
}

#[test]
fn opencode_synthetic_true_is_automated_false_and_absent_are_unknown() {
    assert_eq!(
        opencode_message_provenance(&json!({"synthetic": true})).authorship,
        MessageAuthorship::Automated
    );
    for data in [json!({"synthetic": false}), json!({})] {
        assert_eq!(
            opencode_message_provenance(&data).authorship,
            MessageAuthorship::Unknown
        );
    }
}

#[test]
fn opencode_sibling_parts_are_classified_independently() {
    let parts = [json!({"synthetic": true}), json!({})];
    assert_eq!(
        opencode_message_provenance(&parts[0]).authorship,
        MessageAuthorship::Automated
    );
    assert_eq!(
        opencode_message_provenance(&parts[1]).authorship,
        MessageAuthorship::Unknown
    );
}

#[test]
fn additive_provenance_does_not_change_legacy_event_identity_hash() {
    let mut event = codex_provider_event(
        1,
        "2026-07-01T00:00:00Z".parse().unwrap(),
        EventType::Message,
        Some(EventRole::User),
        json!({"text":"same raw event"}),
        json!({}),
    );
    let unknown_hash = provider_event_identity_hash(&event).unwrap();
    let source_id = Uuid::parse_str("018fe2e4-2266-7000-8000-000000000001").unwrap();
    let unknown_identity = provider_source_event_import_identity(source_id, 1, &unknown_hash);
    event.message_provenance = MessageProvenance {
        authorship: MessageAuthorship::Human,
        evidence: "provider_prompt_log".to_owned(),
        classifier_version: 1,
    };
    let human_hash = provider_event_identity_hash(&event).unwrap();
    let human_identity = provider_source_event_import_identity(source_id, 1, &human_hash);
    assert_eq!(human_hash, unknown_hash);
    assert_eq!(human_identity.id, unknown_identity.id);
    assert_eq!(human_identity.dedupe_key, unknown_identity.dedupe_key);
}
