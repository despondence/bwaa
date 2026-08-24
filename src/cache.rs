// src/cache.rs
use dashmap::DashMap;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use twilight_model::id::Id;
use twilight_model::id::marker::ChannelMarker;

use crate::googleapis::google::ai::generativelanguage::v1beta::part::Data;
use crate::googleapis::google::ai::generativelanguage::v1beta::{Content, Part};

const MAX_HISTORY_PER_CHANNEL: usize = 12;

#[derive(Debug, Clone, Serialize)]
pub struct CachedMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone)]
pub struct ChannelCache {
    store: Arc<DashMap<Id<ChannelMarker>, VecDeque<Content>>>,
}

impl ChannelCache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }

    /// Pushes a turn to history, ensuring only text parts are preserved
    pub fn push(&self, channel_id: Id<ChannelMarker>, mut content: Content) {
        // Strip any non-text parts (like FunctionCall/FunctionResponse) before saving
        content
            .parts
            .retain(|p| matches!(p.data, Some(Data::Text(_))));

        if content.parts.is_empty() {
            return;
        }

        let mut queue = self.store.entry(channel_id).or_insert_with(VecDeque::new);
        queue.push_back(content);
        if queue.len() > MAX_HISTORY_PER_CHANNEL {
            queue.pop_front();
        }
    }

    pub fn get_history(&self, channel_id: Id<ChannelMarker>) -> Vec<Content> {
        self.store
            .get(&channel_id)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn to_cached_messages(
        &self,
        channel_id: Id<ChannelMarker>,
        limit: usize,
    ) -> Vec<CachedMessage> {
        self.store
            .get(&channel_id)
            .map(|queue| {
                queue
                    .iter()
                    .rev()
                    .take(limit)
                    .map(|item| {
                        let text = item
                            .parts
                            .iter()
                            .filter_map(|p| match &p.data {
                                Some(Data::Text(t)) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        CachedMessage {
                            role: item.role.clone(),
                            content: text,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
