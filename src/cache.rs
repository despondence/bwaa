use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use twilight_model::id::Id;
use twilight_model::id::marker::ChannelMarker;

use crate::googleapis::google::ai::generativelanguage::v1beta::Content;

const MAX_HISTORY_PER_CHANNEL: usize = 10;

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

    pub fn push(&self, channel_id: Id<ChannelMarker>, content: Content) {
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
}
