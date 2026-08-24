use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use twilight_http::Client as HttpClient;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::gateway::payload::incoming::MessageCreate;
use twilight_model::id::Id;
use twilight_model::id::marker::{EmojiMarker, MessageMarker};

use crate::cache::ChannelCache;
use crate::gemini::Gemini;
use crate::googleapis::google::ai::generativelanguage::v1beta::part::Data;
use crate::googleapis::google::ai::generativelanguage::v1beta::{
    Content, GenerateContentRequest, GenerationConfig, Part,
};
use crate::memory::MemoryDb;

const MODEL_NAME: &str = "models/gemini-3.1-flash-lite";

/// Granular, flexible actions Gemini can choose to execute.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Add a Unicode or custom reaction to a specific message.
    AddReaction {
        #[serde(default)]
        message_id: Option<String>,
        /// Can be a unicode emoji ("👍") or a custom emoji name ("pepe")
        emoji_name: String,
        /// Required only if using a custom guild emoji
        emoji_id: Option<String>,
    },
    /// Send a message to the channel (either as a reply or raw message).
    SendMessage {
        content: String,
        #[serde(default)]
        reply_to_message_id: Option<String>,
    },
    /// Execute JavaScript inside the Deno sandbox runtime.
    ExecuteJs { code: String },
    /// Explicitly decide to do nothing.
    DoNothing,
}

#[derive(Debug, Deserialize)]
pub struct DecisionResponse {
    pub reason: String,
    pub actions: Vec<Action>,
}

pub async fn handle_chat_turn(
    http: Arc<HttpClient>,
    gemini: Arc<AsyncMutex<Gemini>>,
    db: MemoryDb,
    cache: ChannelCache,
    system_instruction: String,
    msg: MessageCreate,
    prompt: String,
    mut history: Vec<Content>,
) -> anyhow::Result<()> {
    tracing::debug!(
        author = %msg.author.name,
        author_id = %msg.author.id,
        channel_id = %msg.channel_id,
        "Starting autonomous turn execution"
    );

    let _ = http.create_typing_trigger(msg.channel_id).await;

    // Recall SQLite memory for current prompt and inject into the user's turn
    let recalled = db.recall_memories(&prompt)?;
    if !recalled.is_empty() {
        if let Some(last_turn) = history.last_mut() {
            if let Some(Part {
                data: Some(Data::Text(text)),
                ..
            }) = last_turn.parts.first_mut()
            {
                *text = format!(
                    "=== RECALLED MEMORIES ===\n{}\n\n{}",
                    recalled.join("\n"),
                    text
                );
            }
        }
    }

    let req = GenerateContentRequest {
        model: MODEL_NAME.to_string(),
        contents: history,
        system_instruction: Some(Content {
            role: "system".into(),
            parts: vec![Part {
                data: Some(Data::Text(system_instruction)),
                ..Default::default()
            }],
        }),
        generation_config: Some(GenerationConfig {
            response_mime_type: "application/json".into(),
            ..Default::default()
        }),
        ..Default::default()
    };

    tracing::info!(
        model = MODEL_NAME,
        "Sending GenerateContentRequest to Gemini gRPC..."
    );
    let start_time = Instant::now();

    let response_result = {
        let mut client = gemini.lock().await;
        client.generate_content(req.clone()).await
    };

    let response = match response_result {
        Ok(res) => res,
        Err(err) if err.to_string().contains("Invalid argument") => {
            tracing::warn!("Gemini rejected inline payload. Retrying with text-only parts...");
            let mut sanitized_contents = req.contents.clone();
            for content in &mut sanitized_contents {
                content
                    .parts
                    .retain(|p| matches!(p.data, Some(Data::Text(_))));
            }

            let fallback_req = GenerateContentRequest {
                contents: sanitized_contents,
                ..req
            };
            let mut client = gemini.lock().await;
            client.generate_content(fallback_req).await?
        }
        Err(e) => return Err(e.into()),
    };

    tracing::info!(
        elapsed_ms = start_time.elapsed().as_millis(),
        "Received gRPC response"
    );

    let raw_text = response
        .candidates
        .first()
        .and_then(|c| c.content.as_ref())
        .and_then(|c| c.parts.first())
        .and_then(|p| p.data.as_ref())
        .and_then(|d| match d {
            Data::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .unwrap_or("{}");

    tracing::debug!(raw_json = %raw_text, "Unwrapped raw response JSON");

    let decision: DecisionResponse = serde_json::from_str(raw_text).unwrap_or_else(|e| {
        tracing::warn!(error = ?e, "Failed to parse structured actions; using raw text fallback");
        DecisionResponse {
            reason: "Fallback parse".into(),
            actions: vec![Action::SendMessage {
                content: raw_text.to_string(),
                reply_to_message_id: Some(msg.id.to_string()),
            }],
        }
    });

    tracing::info!(
        reason = %decision.reason,
        action_count = decision.actions.len(),
        "Model decision processing"
    );

    // Group reactions by target message ID to preserve strict order per message
    let mut sequential_reactions: HashMap<Id<MessageMarker>, Vec<(String, Option<String>)>> =
        HashMap::new();

    for action in decision.actions {
        match action {
            Action::AddReaction {
                message_id,
                emoji_name,
                emoji_id,
            } => {
                let target_msg_id = message_id
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Id::<MessageMarker>::new)
                    .unwrap_or(msg.id);

                sequential_reactions
                    .entry(target_msg_id)
                    .or_default()
                    .push((emoji_name, emoji_id));
            }

            Action::SendMessage {
                content,
                reply_to_message_id,
            } => {
                let http = Arc::clone(&http);
                let cache = cache.clone();
                let channel_id = msg.channel_id;

                // Spawn message sending in parallel immediately
                tokio::spawn(async move {
                    let trimmed = content.trim();
                    if trimmed.is_empty() {
                        return;
                    }

                    cache.push(
                        channel_id,
                        Content {
                            role: "model".into(),
                            parts: vec![Part {
                                data: Some(Data::Text(trimmed.to_string())),
                                ..Default::default()
                            }],
                        },
                    );

                    let target_reply_id = reply_to_message_id
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(Id::<MessageMarker>::new);

                    for chunk in trimmed.as_bytes().chunks(1900) {
                        if let Ok(chunk_str) = std::str::from_utf8(chunk) {
                            let mut builder = http.create_message(channel_id).content(chunk_str);
                            if let Some(reply_id) = target_reply_id {
                                builder = builder.reply(reply_id);
                            }
                            if let Err(e) = builder.await {
                                tracing::error!(error = ?e, "Failed to send message chunk");
                            }
                        }
                    }
                });
            }

            Action::ExecuteJs { code } => {
                let http = Arc::clone(&http);
                let cache = cache.clone();
                let msg_clone = msg.clone();

                tokio::task::spawn_blocking(move || {
                    if let Err(e) = crate::sandbox::run_js_eval_sync(
                        Arc::clone(&http),
                        cache,
                        msg_clone.clone(),
                        code,
                        Duration::from_secs(5),
                    ) {
                        tracing::error!(error = ?e, "Sandbox execution error");

                        // Post the error back to Discord
                        let err_msg = format!("```js\n[JS Eval Error]: {}\n```", e);
                        let channel_id = msg_clone.channel_id;
                        let msg_id = msg_clone.id;

                        // Spawn a task on the current thread's handle to fire the HTTP reply
                        tokio::runtime::Handle::current().spawn(async move {
                            let _ = http
                                .create_message(channel_id)
                                .content(&err_msg)
                                .reply(msg_id)
                                .await;
                        });
                    }
                });
            }

            Action::DoNothing => {
                tracing::debug!("Model issued explicit DoNothing action");
            }
        }
    }

    // Dispatch reaction sequences per message target concurrently
    for (target_msg_id, emojis) in sequential_reactions {
        let http = Arc::clone(&http);
        let channel_id = msg.channel_id;

        tokio::spawn(async move {
            for (emoji_name, emoji_id) in emojis {
                let reaction_type = if let Some(id_str) = emoji_id {
                    if let Ok(raw_id) = id_str.parse::<u64>() {
                        RequestReactionType::Custom {
                            id: Id::<EmojiMarker>::new(raw_id),
                            name: Some(&emoji_name),
                        }
                    } else {
                        RequestReactionType::Unicode { name: &emoji_name }
                    }
                } else {
                    RequestReactionType::Unicode { name: &emoji_name }
                };

                if let Err(e) = http
                    .create_reaction(channel_id, target_msg_id, &reaction_type)
                    .await
                {
                    tracing::error!(error = ?e, "Failed to dispatch reaction action");
                }

                // 200ms delay ensures Discord places reactions strictly in array order
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
    }

    Ok(())
}
