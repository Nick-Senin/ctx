use rusqlite::Connection;

use super::ddl::table_has_column;
use super::views::create_stable_sql_views;
use crate::{Result, StoreError};

pub(super) fn migrate_to_v47(conn: &Connection) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE;")?;
    let migration = (|| -> Result<()> {
        if !table_has_column(conn, "events", "message_authorship")? {
            conn.execute_batch(
                "ALTER TABLE events ADD COLUMN message_authorship TEXT NOT NULL DEFAULT 'unknown' CHECK (message_authorship IN ('human', 'automated', 'unknown'));",
            )?;
        }
        if !table_has_column(conn, "events", "message_authorship_evidence")? {
            conn.execute_batch(
                "ALTER TABLE events ADD COLUMN message_authorship_evidence TEXT NOT NULL DEFAULT 'ambiguous';",
            )?;
        }
        if !table_has_column(conn, "events", "message_authorship_classifier_version")? {
            conn.execute_batch(
                "ALTER TABLE events ADD COLUMN message_authorship_classifier_version INTEGER NOT NULL DEFAULT 0 CHECK (message_authorship_classifier_version >= 0);",
            )?;
        }
        conn.execute_batch(
            "UPDATE catalog_sessions
             SET indexed_status = 'pending', indexed_event_count = NULL
             WHERE indexed_status = 'indexed'
               AND source_format IN ('codex_session_jsonl', 'codex_session_jsonl_tree', 'codex_history_jsonl', 'claude_projects_jsonl_tree', 'claude_history_jsonl', 'opencode_sqlite');
             UPDATE source_import_files
             SET indexed_status = 'pending'
             WHERE indexed_status = 'indexed'
               AND source_format IN ('codex_session_jsonl', 'codex_session_jsonl_tree', 'codex_history_jsonl', 'claude_projects_jsonl_tree', 'claude_history_jsonl', 'opencode_sqlite');",
        )?;
        create_stable_sql_views(conn)?;
        conn.execute_batch("PRAGMA user_version = 47;")?;
        Ok(())
    })();

    match migration {
        Ok(()) => {
            conn.execute_batch("COMMIT;")?;
            Ok(())
        }
        Err(err) => {
            if let Err(rollback_err) = conn.execute_batch("ROLLBACK;") {
                return Err(StoreError::Sql(rollback_err));
            }
            Err(err)
        }
    }
}
