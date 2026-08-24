// src/agent/tool.rs
use async_trait::async_trait;
use serde_json::Value as JsonValue;
use std::fmt::Debug;
use twilight_model::channel::message::embed::Embed;

use crate::agent::context::ToolContext;

#[derive(Debug, Clone)]
pub struct ToolResult {
    /// JSON payload sent back to Gemini in FunctionResponse
    pub model_response: JsonValue,
    /// Optional rich Discord embed to attach to the final response message
    pub visual_embed: Option<Embed>,
}

impl ToolResult {
    pub fn info(data: JsonValue) -> Self {
        Self {
            model_response: data,
            visual_embed: None,
        }
    }

    pub fn info_with_embed(data: JsonValue, embed: Embed) -> Self {
        Self {
            model_response: data,
            visual_embed: Some(embed),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ToolOutput {
    /// Informational response + optional visual embed
    Info(ToolResult),
    /// Side-effect action completed (e.g. queued reaction)
    ActionExecuted(&'static str),
    /// Explicit turn termination
    Stop,
}

#[async_trait]
pub trait Tool: Send + Sync + Debug {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters_schema(&self) -> JsonValue;
    async fn execute(&self, ctx: &ToolContext, args: JsonValue) -> anyhow::Result<ToolOutput>;
}
