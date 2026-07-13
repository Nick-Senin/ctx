use ctx_history_core::{MessageAuthorship, MessageProvenance};
use serde_json::Value;

pub(crate) fn opencode_message_provenance(data: &Value) -> MessageProvenance {
    let synthetic = data.get("synthetic").and_then(Value::as_bool) == Some(true);
    MessageProvenance {
        authorship: if synthetic {
            MessageAuthorship::Automated
        } else {
            MessageAuthorship::Unknown
        },
        evidence: if synthetic {
            "provider_synthetic"
        } else {
            "ambiguous"
        }
        .to_owned(),
        classifier_version: crate::MESSAGE_AUTHORSHIP_CLASSIFIER_REVISION,
    }
}
