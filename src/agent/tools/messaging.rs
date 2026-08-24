// src/agent/tools/messaging.rs
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use twilight_model::id::Id;
use twilight_model::id::marker::MessageMarker;

use crate::agent::context::ToolContext;
use crate::agent::tool::{Tool, ToolOutput};

#[derive(Debug)]
pub struct SendMessageTool;

#[derive(Deserialize)]
struct SendMessageArgs {
    content: String,
    reply_to_message_id: Option<String>,
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &'static str {
        "send_message"
    }

    fn description(&self) -> &'static str {
        "Send a message or reply directly to the active Discord channel."
    }

    fn parameters_schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Text content to send. May include URLs or Discord mentions."
                },
                "reply_to_message_id": {
                    "type": "string",
                    "description": "Optional Message ID to reply to directly."
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: JsonValue) -> anyhow::Result<ToolOutput> {
        let args: SendMessageArgs = serde_json::from_value(args)?;
        let trimmed = args.content.trim();
        if trimmed.is_empty() {
            return Ok(ToolOutput::ActionExecuted("Empty message skipped"));
        }

        let target_reply_id = args
            .reply_to_message_id
            .and_then(|s| s.parse::<u64>().ok())
            .map(Id::<MessageMarker>::new);

        for chunk in trimmed.as_bytes().chunks(1900) {
            if let Ok(chunk_str) = std::str::from_utf8(chunk) {
                let mut builder = ctx.http.create_message(ctx.channel_id).content(chunk_str);
                if let Some(reply_id) = target_reply_id {
                    builder = builder.reply(reply_id);
                }
                builder.await?;
            }
        }

        Ok(ToolOutput::ActionExecuted("Message sent"))
    }
}

#[derive(Debug)]
pub struct AddReactionTool;

#[derive(Deserialize)]
struct ReactionArgs {
    emoji_name: String,
    emoji_id: Option<String>,
    message_id: Option<String>,
}

#[async_trait]
impl Tool for AddReactionTool {
    fn name(&self) -> &'static str {
        "add_reaction"
    }

    fn description(&self) -> &'static str {
        "React to a message with a unicode or custom emoji."
    }

    fn parameters_schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "emoji_name": { "type": "string", "description": "Unicode emoji (e.g. 💀) or custom emoji name" },
                "emoji_id": { "type": "string", "description": "Snowflake ID if custom server emoji" },
                "message_id": { "type": "string", "description": "Target message ID (defaults to trigger message)" }
            },
            "required": ["emoji_name"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: JsonValue) -> anyhow::Result<ToolOutput> {
        let args: ReactionArgs = serde_json::from_value(args)?;
        let target_msg_id = args
            .message_id
            .and_then(|s| s.parse::<u64>().ok())
            .map(Id::<MessageMarker>::new)
            .unwrap_or(ctx.message.id);

        ctx.reactions.queue(
            ctx.channel_id,
            target_msg_id,
            args.emoji_name,
            args.emoji_id,
        );

        Ok(ToolOutput::ActionExecuted("Reaction queued"))
    }
}
