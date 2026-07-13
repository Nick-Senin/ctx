use ctx_history_core::{MessageAuthorship, MessageProvenance};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::connection::{nonnegative_i64_to_u64, parse_text_enum};
use crate::Result;

/// Merge classifier output without making reimport order observable.
///
/// Newer classifier versions replace older versions. Older versions never
/// replace newer output. At the same version, identical output is a no-op;
/// conflicts converge conservatively (`unknown` > `automated` > `human`) and
/// then use evidence text as a stable tie-breaker. A later classifier version
/// is the explicit correction mechanism.
pub(crate) fn merge_message_provenance(
    conn: &Connection,
    event_id: Uuid,
    incoming: &MessageProvenance,
) -> Result<()> {
    let stored = conn
        .query_row(
            "SELECT message_authorship, message_authorship_evidence, message_authorship_classifier_version FROM events WHERE id = ?1",
            params![event_id.to_string()],
            |row| {
                Ok(MessageProvenance {
                    authorship: parse_text_enum(row.get::<_, String>(0)?)?,
                    evidence: row.get(1)?,
                    classifier_version: nonnegative_i64_to_u64(row.get(2)?)? as u32,
                })
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(());
    };
    let merged = merged_message_provenance(&stored, incoming);
    if merged == stored {
        return Ok(());
    }
    conn.execute(
        "UPDATE events SET message_authorship = ?1, message_authorship_evidence = ?2, message_authorship_classifier_version = ?3 WHERE id = ?4",
        params![
            merged.authorship.as_str(),
            merged.evidence,
            i64::from(merged.classifier_version),
            event_id.to_string(),
        ],
    )?;
    Ok(())
}

fn merged_message_provenance(
    stored: &MessageProvenance,
    incoming: &MessageProvenance,
) -> MessageProvenance {
    match incoming.classifier_version.cmp(&stored.classifier_version) {
        std::cmp::Ordering::Greater => incoming.clone(),
        std::cmp::Ordering::Less => stored.clone(),
        std::cmp::Ordering::Equal => {
            if provenance_order_key(incoming) > provenance_order_key(stored) {
                incoming.clone()
            } else {
                stored.clone()
            }
        }
    }
}

fn provenance_order_key(provenance: &MessageProvenance) -> (u8, &str) {
    let conservative_rank = match provenance.authorship {
        MessageAuthorship::Human => 0,
        MessageAuthorship::Automated => 1,
        MessageAuthorship::Unknown => 2,
    };
    (conservative_rank, provenance.evidence.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn equal_version_conflicts_are_order_independent_and_conservative() {
        let human = provenance(MessageAuthorship::Human, "prompt_log", 1);
        let automated = provenance(MessageAuthorship::Automated, "synthetic", 1);
        assert_eq!(merged_message_provenance(&human, &automated), automated);
        assert_eq!(merged_message_provenance(&automated, &human), automated);
    }
}
