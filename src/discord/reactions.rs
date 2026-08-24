// src/discord/reactions.rs
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use twilight_http::Client as HttpClient;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, EmojiMarker, MessageMarker};

struct QueuedReaction {
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji_name: String,
    emoji_id: Option<String>,
}

#[derive(Clone)]
pub struct ReactionScheduler {
    tx: UnboundedSender<QueuedReaction>,
}

impl ReactionScheduler {
    pub fn new(http: Arc<HttpClient>) -> Self {
        let (tx, rx) = unbounded_channel();
        tokio::spawn(Self::worker_loop(http, rx));
        Self { tx }
    }

    pub fn queue(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji_name: String,
        emoji_id: Option<String>,
    ) {
        let _ = self.tx.send(QueuedReaction {
            channel_id,
            message_id,
            emoji_name,
            emoji_id,
        });
    }

    async fn worker_loop(http: Arc<HttpClient>, mut rx: UnboundedReceiver<QueuedReaction>) {
        while let Some(item) = rx.recv().await {
            let reaction_type = if let Some(id_str) = item.emoji_id {
                if let Ok(raw_id) = id_str.parse::<u64>() {
                    RequestReactionType::Custom {
                        id: Id::<EmojiMarker>::new(raw_id),
                        name: Some(&item.emoji_name),
                    }
                } else {
                    RequestReactionType::Unicode {
                        name: &item.emoji_name,
                    }
                }
            } else {
                RequestReactionType::Unicode {
                    name: &item.emoji_name,
                }
            };

            let _ = http
                .create_reaction(item.channel_id, item.message_id, &reaction_type)
                .await;

            // Maintain strict order & prevent Discord rate limits
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
