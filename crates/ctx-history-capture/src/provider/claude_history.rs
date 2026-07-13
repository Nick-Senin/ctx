use std::{collections::BTreeMap, fs::File, io::BufReader, path::Path};

use chrono::{DateTime, Utc};
use ctx_history_core::{
    AgentType, CaptureProvider, EventRole, EventType, Fidelity, MessageAuthorship,
    MessageProvenance, ProviderCaptureEnvelope, ProviderCursorCheckpoint, ProviderCursorRange,
    ProviderEventEnvelope, ProviderSessionEnvelope, ProviderSourceEnvelope, ProviderSourceTrust,
    SessionStatus, PROVIDER_CAPTURE_ENVELOPE_SCHEMA_VERSION,
};
use ctx_history_store::Store;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::common::io::{
    ensure_regular_provider_transcript_file, read_provider_jsonl_record_or_skip_oversized,
};
use crate::provider::importer::{import_normalized_provider_captures, provider_cursor_stream};
use crate::{
    compute_payload_hash, fnv1a64, ClaudeHistoryImportOptions, ClaudeHistoryJsonlAdapter,
    NormalizedProviderImportOptions, ProviderAdapterContext, ProviderCaptureAdapter,
    ProviderImportFailure, ProviderImportSummary, ProviderNormalizationResult, Result,
    CLAUDE_HISTORY_SOURCE_FORMAT,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeHistoryLine {
    display: String,
    #[serde(default)]
    pasted_contents: Map<String, Value>,
    timestamp: i64,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

impl ProviderCaptureAdapter for ClaudeHistoryJsonlAdapter {
    fn provider(&self) -> CaptureProvider {
        CaptureProvider::Claude
    }

    fn source_format(&self) -> &str {
        CLAUDE_HISTORY_SOURCE_FORMAT
    }

    fn normalize_path(
        &self,
        path: &Path,
        context: &ProviderAdapterContext,
    ) -> Result<ProviderNormalizationResult> {
        ensure_regular_provider_transcript_file(path)?;
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut result = ProviderNormalizationResult::default();
        let mut parsed = Vec::new();
        let mut first_seen = BTreeMap::new();
        let mut line = Vec::new();
        let mut line_number = 0usize;

        while read_provider_jsonl_record_or_skip_oversized(
            &mut reader,
            &mut line,
            &mut line_number,
            &mut result.summary,
        )? {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let history: ClaudeHistoryLine = match serde_json::from_slice(&line) {
                Ok(history) => history,
                Err(err) => {
                    result.summary.failed += 1;
                    result.summary.failures.push(ProviderImportFailure {
                        line: line_number,
                        error: err.to_string(),
                    });
                    continue;
                }
            };
            let Some(occurred_at) = DateTime::<Utc>::from_timestamp_millis(history.timestamp)
            else {
                result.summary.failed += 1;
                result.summary.failures.push(ProviderImportFailure {
                    line: line_number,
                    error: format!(
                        "claude history line has invalid unix millisecond timestamp {}",
                        history.timestamp
                    ),
                });
                continue;
            };
            let provider_session_id = claude_history_session_id(&history);
            first_seen
                .entry(provider_session_id.clone())
                .and_modify(|existing: &mut DateTime<Utc>| {
                    if occurred_at < *existing {
                        *existing = occurred_at;
                    }
                })
                .or_insert(occurred_at);
            parsed.push((line_number, history, provider_session_id, occurred_at));
        }

        result.captures = parsed
            .into_iter()
            .map(|(line_number, history, provider_session_id, occurred_at)| {
                let project_owned = history.project.clone();
                let native_session_id_owned = history.session_id.clone();
                let project = project_owned.as_deref().filter(|value| !value.trim().is_empty());
                let native_session_id = native_session_id_owned
                    .as_deref()
                    .filter(|value| !value.trim().is_empty());
                let native_payload = json!({
                    "display": history.display,
                    "pastedContents": history.pasted_contents,
                    "timestamp": history.timestamp,
                    "project": project_owned.clone(),
                    "sessionId": native_session_id_owned.clone(),
                });
                let native_identity = compute_payload_hash(&json!({
                    "sessionId": native_session_id,
                    "timestamp": history.timestamp,
                    "display": native_payload["display"],
                    "pastedContents": native_payload["pastedContents"],
                    "project": project,
                }))
                .expect("serializing a parsed Claude history record cannot fail");
                let provider_event_index = fnv1a64(native_identity.as_bytes());
                let paste_fidelity = claude_history_paste_fidelity(&native_payload["pastedContents"]);
                let started_at = first_seen[&provider_session_id];
                (
                    line_number,
                    ProviderCaptureEnvelope {
                        schema_version: PROVIDER_CAPTURE_ENVELOPE_SCHEMA_VERSION,
                        provider: CaptureProvider::Claude,
                        source: ProviderSourceEnvelope {
                            source_format: CLAUDE_HISTORY_SOURCE_FORMAT.to_owned(),
                            machine_id: context.machine_id.clone(),
                            observed_at: context.imported_at,
                            // A copied prompt log is the same provider-native source. Keep the
                            // observed path in metadata, outside identity-bearing fields.
                            raw_source_path: None,
                            source_root: None,
                            trust: ProviderSourceTrust::ProviderNative,
                            fidelity: Fidelity::SummaryOnly,
                            cursor: Some(ProviderCursorRange {
                                before: None,
                                after: Some(ProviderCursorCheckpoint {
                                    stream: provider_cursor_stream(
                                        CaptureProvider::Claude,
                                        CLAUDE_HISTORY_SOURCE_FORMAT,
                                    ),
                                    cursor: format!("line:{line_number}"),
                                    observed_at: occurred_at,
                                }),
                            }),
                            idempotency_key: Some(format!(
                                "provider-source:claude:{CLAUDE_HISTORY_SOURCE_FORMAT}:{provider_session_id}"
                            )),
                            metadata: json!({
                                "adapter": CLAUDE_HISTORY_SOURCE_FORMAT,
                                "source_fidelity": "prompt_log_only",
                                "observed_source_path": context.source_path.as_ref().map(|path| path.display().to_string()),
                            }),
                        },
                        session: ProviderSessionEnvelope {
                            provider_session_id: provider_session_id.clone(),
                            parent_provider_session_id: None,
                            root_provider_session_id: None,
                            external_agent_id: None,
                            agent_type: AgentType::Primary,
                            role_hint: Some("primary".to_owned()),
                            is_primary: true,
                            status: SessionStatus::Imported,
                            started_at,
                            ended_at: None,
                            cwd: project.map(str::to_owned),
                            fidelity: Fidelity::SummaryOnly,
                            idempotency_key: Some(format!(
                                "provider-session:claude:{CLAUDE_HISTORY_SOURCE_FORMAT}:{provider_session_id}"
                            )),
                            artifacts: Vec::new(),
                            metadata: json!({
                                "source_format": CLAUDE_HISTORY_SOURCE_FORMAT,
                                "source_fidelity": "prompt_log_only",
                                "native_session_id": native_session_id,
                                "session_identity": if native_session_id.is_some() { "provider_session_id" } else { "derived_orphan_project" },
                                "project": project,
                                "limitations": [
                                    "operator prompts only",
                                    "no assistant responses",
                                    "no tool calls or command output",
                                    "pasted content may be represented by a provider hash only"
                                ],
                            }),
                        },
                        event: Some(ProviderEventEnvelope {
                            message_provenance: MessageProvenance {
                                authorship: MessageAuthorship::Human,
                                evidence: "provider_prompt_log".to_owned(),
                                classifier_version: crate::MESSAGE_AUTHORSHIP_CLASSIFIER_REVISION,
                            },
                            provider_event_index,
                            provider_event_hash: Some(native_identity.clone()),
                            cursor: Some(format!("timestamp:{}:{native_identity}", history.timestamp)),
                            event_type: EventType::Message,
                            role: Some(EventRole::User),
                            occurred_at,
                            fidelity: Fidelity::SummaryOnly,
                            idempotency_key: Some(format!(
                                "provider-event:claude:{CLAUDE_HISTORY_SOURCE_FORMAT}:{native_identity}"
                            )),
                            artifacts: Vec::new(),
                            payload: json!({
                                "text": native_payload["display"],
                                "display": native_payload["display"],
                                "pasted_contents": native_payload["pastedContents"],
                                "timestamp_ms": history.timestamp,
                                "project": project,
                                "session_id": native_session_id,
                                "source_format": CLAUDE_HISTORY_SOURCE_FORMAT,
                                "native": native_payload,
                            }),
                            metadata: json!({
                                "source": "claude_history",
                                "source_format": CLAUDE_HISTORY_SOURCE_FORMAT,
                                "source_fidelity": "prompt_log_only",
                                "native_identity": native_identity,
                                "paste_fidelity": paste_fidelity,
                                "observed_line": line_number,
                            }),
                        }),
                    },
                )
            })
            .collect();

        Ok(result)
    }
}

fn claude_history_session_id(history: &ClaudeHistoryLine) -> String {
    if let Some(session_id) = history
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        // Keep prompt-log sessions distinct from the richer project transcript
        // session that may carry the same native sessionId.
        return format!("history:{session_id}");
    }
    let project = history
        .project
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown-project");
    format!("history-orphan:{:016x}", fnv1a64(project.as_bytes()))
}

fn claude_history_paste_fidelity(pasted_contents: &Value) -> &'static str {
    let Some(entries) = pasted_contents.as_object() else {
        return "none";
    };
    if entries.is_empty() {
        return "none";
    }
    let has_content = entries.values().any(|entry| entry.get("content").is_some());
    let has_hash_only = entries
        .values()
        .any(|entry| entry.get("content").is_none() && entry.get("contentHash").is_some());
    match (has_content, has_hash_only) {
        (true, true) => "mixed",
        (true, false) => "full",
        (false, true) => "hash_only",
        (false, false) => "metadata_only",
    }
}

pub fn import_claude_history_jsonl(
    path: impl AsRef<Path>,
    store: &mut Store,
    options: ClaudeHistoryImportOptions,
) -> Result<ProviderImportSummary> {
    let path = path.as_ref();
    let source_path = options
        .source_path
        .clone()
        .unwrap_or_else(|| path.to_path_buf());
    let normalization = ClaudeHistoryJsonlAdapter.normalize_path(
        path,
        &ProviderAdapterContext {
            machine_id: options.machine_id,
            source_path: Some(source_path),
            source_root: None,
            imported_at: options.imported_at,
        },
    )?;
    import_normalized_provider_captures(
        store,
        normalization,
        NormalizedProviderImportOptions {
            history_record_id: options.history_record_id,
            persist_cursors: true,
            wrap_transaction: true,
            fast_event_inserts: true,
        },
    )
}
