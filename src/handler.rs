use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex as AsyncMutex;
use twilight_http::Client as HttpClient;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::gateway::payload::incoming::MessageCreate;

use crate::cache::ChannelCache;
use crate::gemini::Gemini;
use crate::googleapis::google::ai::generativelanguage::v1beta::part::Data;
use crate::googleapis::google::ai::generativelanguage::v1beta::{
    Content, GenerateContentRequest, GenerationConfig, Part,
};
use crate::memory::MemoryDb;

const MODEL_NAME: &str = "models/gemini-3.1-flash-lite";

#[derive(Deserialize)]
pub struct BotAction {
    pub should_reply: Option<bool>,
    pub reason: Option<String>,
    pub reply: Option<String>,
    pub reaction: Option<String>,
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

    // Recall SQLite memory for current prompt and inject into the user's latest turn
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

    let action: BotAction = serde_json::from_str(raw_text).unwrap_or(BotAction {
        should_reply: Some(true),
        reason: Some("Failed JSON parse fallback".into()),
        reply: Some(raw_text.to_string()),
        reaction: None,
    });

    tracing::info!(
        should_reply = action.should_reply.unwrap_or(false),
        reason = %action.reason.as_deref().unwrap_or("none"),
        "Model autonomous decision evaluated"
    );

    // Dispatch reaction side-effect
    if let Some(emoji) = &action.reaction {
        let http = Arc::clone(&http);
        let channel_id = msg.channel_id;
        let message_id = msg.id;
        let emoji_clone = emoji.clone();
        tokio::spawn(async move {
            let reaction_type = RequestReactionType::Unicode { name: &emoji_clone };
            if let Err(e) = http
                .create_reaction(channel_id, message_id, &reaction_type)
                .await
            {
                tracing::error!(error = ?e, emoji = %emoji_clone, "Failed to apply Discord reaction");
            }
        });
    }

    // Dispatch reply text if decision was positive
    if action.should_reply.unwrap_or(true) {
        if let Some(reply_text) = &action.reply {
            let trimmed = reply_text.trim();
            if !trimmed.is_empty() {
                cache.push(
                    msg.channel_id,
                    Content {
                        role: "model".into(),
                        parts: vec![Part {
                            data: Some(Data::Text(trimmed.to_string())),
                            ..Default::default()
                        }],
                    },
                );

                for chunk in trimmed.as_bytes().chunks(1900) {
                    let chunk_str = std::str::from_utf8(chunk)?;
                    http.create_message(msg.channel_id)
                        .content(chunk_str)
                        .reply(msg.id)
                        .await?;
                }
            }
        }
    } else {
        tracing::debug!("bwaa decided to stay quiet");
    }

    Ok(())
}
