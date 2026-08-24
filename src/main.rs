// src/main.rs
mod agent;
mod authorisation;
mod cache;
mod config;
mod discord;
mod gemini;
mod googleapis;
mod media;
mod memory;
mod sandbox;

use rand::RngExt;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use twilight_gateway::{Event, EventTypeFlags, Intents, StreamExt};
use twilight_http::Client as HttpClient;

use crate::agent::context::ToolContext;
use crate::agent::engine::AgentEngine;
use crate::agent::registry::ToolRegistry;
use crate::agent::tools::messaging::{AddReactionTool, SendMessageTool};
use crate::agent::tools::sandbox::ExecuteJsTool;
use crate::cache::ChannelCache;
use crate::config::Config;
use crate::discord::reactions::ReactionScheduler;
use crate::gemini::client::GeminiClient;
use crate::googleapis::google::ai::generativelanguage::v1beta::part::Data;
use crate::googleapis::google::ai::generativelanguage::v1beta::{Content, Part};
use crate::media::process_attachments;
use crate::memory::MemoryDb;

const MODEL_NAME: &str = "models/gemini-3.1-flash-lite";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,bwaa=debug".into()),
        )
        .init();

    let cfg = Config::load()?;
    let http = Arc::new(HttpClient::new(cfg.discord_token.clone()));
    let bot_id = http.current_user().await?.model().await?.id;

    let gemini_client = GeminiClient::connect(cfg.gemini_api_key).await?;
    let gemini = Arc::new(AsyncMutex::new(gemini_client));
    let db = MemoryDb::open("bwaa_memory.db")?;
    let cache = ChannelCache::new();
    let reactions = ReactionScheduler::new(http.clone());

    // Register all modular tools
    let registry = ToolRegistry::builder()
        .register(SendMessageTool)
        .register(AddReactionTool)
        .register(ExecuteJsTool)
        .build();

    let engine = Arc::new(AgentEngine::new(
        gemini,
        registry,
        MODEL_NAME,
        cfg.system_instruction,
    ));

    let intents = Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES | Intents::MESSAGE_CONTENT;
    let mut shard =
        twilight_gateway::Shard::new(twilight_gateway::ShardId::ONE, cfg.discord_token, intents);

    tracing::info!("✨ bwaa refactored engine is ready!");

    while let Some(item) = shard.next_event(EventTypeFlags::MESSAGE_CREATE).await {
        let Ok(Event::MessageCreate(msg)) = item else {
            continue;
        };

        if msg.author.bot || msg.author.id == bot_id {
            continue;
        }

        let prompt = clean_prompt(&msg);
        let mut user_text = format!("[Msg ID: {}] {}: {}", msg.id, msg.author.name, prompt);
        let mut parts = process_attachments(&msg, &mut user_text).await;
        parts.insert(
            0,
            Part {
                data: Some(Data::Text(user_text)),
                ..Default::default()
            },
        );

        cache.push(
            msg.channel_id,
            Content {
                role: "user".into(),
                parts,
            },
        );

        let is_mentioned = msg.mentions.iter().any(|u| u.id == bot_id);
        let is_reply = msg
            .referenced_message
            .as_ref()
            .is_some_and(|m| m.author.id == bot_id);
        let random_roll = rand::rng().random_bool(0.15);

        if is_mentioned || is_reply || random_roll {
            let engine = engine.clone();
            let history = cache.get_history(msg.channel_id);
            let ctx = ToolContext {
                http: http.clone(),
                cache: cache.clone(),
                db: db.clone(),
                reactions: reactions.clone(),
                channel_id: msg.channel_id,
                message: *msg,
            };

            tokio::spawn(async move {
                if let Err(e) = engine.run(ctx, history).await {
                    tracing::error!(error = ?e, "Error during agent execution");
                }
            });
        }
    }

    Ok(())
}

fn clean_prompt(msg: &twilight_model::gateway::payload::incoming::MessageCreate) -> String {
    let mut clean = msg.content.clone();
    for mentioned in &msg.mentions {
        clean = clean
            .replace(&format!("<@{}>", mentioned.id), "")
            .replace(&format!("<@!{}>", mentioned.id), "");
    }
    clean.trim().to_string()
}
