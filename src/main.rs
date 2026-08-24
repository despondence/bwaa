mod authorisation;
mod cache;
mod config;
mod gemini;
mod googleapis;
mod handler;
mod media;
mod memory;
mod sandbox;

use rand::RngExt;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use twilight_gateway::{Event, EventTypeFlags, Intents, StreamExt};
use twilight_http::Client as HttpClient;

use cache::ChannelCache;
use config::Config;
use gemini::Gemini;
use googleapis::google::ai::generativelanguage::v1beta::part::Data;
use googleapis::google::ai::generativelanguage::v1beta::{Content, Part};
use handler::handle_chat_turn;
use media::process_attachments;
use memory::MemoryDb;

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

    let gemini_client = Gemini::connect(cfg.gemini_api_key).await?;
    let gemini = Arc::new(AsyncMutex::new(gemini_client));
    let db = MemoryDb::open("bwaa_memory.db")?;
    let cache = ChannelCache::new();

    let intents = Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES | Intents::MESSAGE_CONTENT;
    let mut shard =
        twilight_gateway::Shard::new(twilight_gateway::ShardId::ONE, cfg.discord_token, intents);

    tracing::info!("bwaa is running...");

    while let Some(item) = shard.next_event(EventTypeFlags::MESSAGE_CREATE).await {
        let Ok(Event::MessageCreate(msg)) = item else {
            continue;
        };

        if msg.author.bot || msg.author.id == bot_id {
            continue;
        }

        // 1. Clean mentions out of prompt text
        let prompt = clean_prompt(&msg);

        // 2. Format incoming message with Message ID context so model can target actions
        let mut user_text = format!("[Msg ID: {}] {}: {}", msg.id, msg.author.name, prompt);
        let mut parts = process_attachments(&msg, &mut user_text).await;
        parts.insert(
            0,
            Part {
                data: Some(Data::Text(user_text)),
                ..Default::default()
            },
        );

        // 3. Cache current user turn unconditionally
        cache.push(
            msg.channel_id,
            Content {
                role: "user".into(),
                parts,
            },
        );

        // 4. Trigger evaluation logic: Direct mentions/replies OR 15% random chance
        let is_mentioned = msg.mentions.iter().any(|u| u.id == bot_id);
        let is_reply = msg
            .referenced_message
            .as_ref()
            .map_or(false, |m| m.author.id == bot_id);

        let random_roll = rand::rng().random_bool(0.15);

        if is_mentioned || is_reply || random_roll {
            tracing::info!(
                author = %msg.author.name,
                mentioned = is_mentioned,
                reply = is_reply,
                random_chatter = random_roll,
                "Firing autonomous turn execution"
            );

            let http = Arc::clone(&http);
            let gemini = Arc::clone(&gemini);
            let db = db.clone();
            let cache = cache.clone();
            let sys_prompt = cfg.system_instruction.clone();
            let history = cache.get_history(msg.channel_id);

            tokio::spawn(async move {
                if let Err(e) =
                    handle_chat_turn(http, gemini, db, cache, sys_prompt, *msg, prompt, history)
                        .await
                {
                    tracing::error!(error = ?e, "Error executing turn");
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
