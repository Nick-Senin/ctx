use std::collections::HashSet;

use ctx_history_core::MessageAuthorship;
use uuid::Uuid;

use crate::connection::{collect_rows, parse_uuid};
use crate::search::projections::EventSearchHit;
use crate::{Result, Store};

impl Store {
    pub fn search_event_hits(&self, query: &str, limit: usize) -> Result<Vec<EventSearchHit>> {
        self.search_event_hits_page(query, limit, 0)
    }

    pub fn search_event_hits_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EventSearchHit>> {
        self.search_event_hits_page_filtered_with_ranking(query, limit, offset, None, false)
    }

    pub fn search_event_hits_page_prefer_conversation(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EventSearchHit>> {
        self.search_event_hits_page_filtered_with_ranking(query, limit, offset, None, true)
    }

    pub fn search_event_hits_page_filtered(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
        message_authorship: Option<MessageAuthorship>,
    ) -> Result<Vec<EventSearchHit>> {
        self.search_event_hits_page_filtered_with_ranking(
            query,
            limit,
            offset,
            message_authorship,
            false,
        )
    }

    pub fn search_event_hits_page_filtered_prefer_conversation(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
        message_authorship: Option<MessageAuthorship>,
    ) -> Result<Vec<EventSearchHit>> {
        self.search_event_hits_page_filtered_with_ranking(
            query,
            limit,
            offset,
            message_authorship,
            true,
        )
    }

    pub fn event_ids_by_message_authorship(
        &self,
        message_authorship: MessageAuthorship,
    ) -> Result<HashSet<Uuid>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id FROM events WHERE deleted_at_ms IS NULL AND message_authorship = ?1",
        )?;
        let rows = stmt.query_map([message_authorship.as_str()], |row| {
            parse_uuid(row.get::<_, String>(0)?)
        })?;
        Ok(collect_rows(rows)?.into_iter().collect())
    }
}
