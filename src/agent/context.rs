// src/agent/context.rs
use std::sync::Arc;
use twilight_http::Client as HttpClient;
use twilight_model::gateway::payload::incoming::MessageCreate;
use twilight_model::id::Id;
use twilight_model::id::marker::ChannelMarker;

use crate::cache::ChannelCache;
use crate::discord::reactions::ReactionScheduler;
use crate::memory::MemoryDb;

#[derive(Clone)]
pub struct ToolContext {
    pub http: Arc<HttpClient>,
    pub cache: ChannelCache,
    pub db: MemoryDb,
    pub reactions: ReactionScheduler,
    pub channel_id: Id<ChannelMarker>,
    pub message: MessageCreate,
}
