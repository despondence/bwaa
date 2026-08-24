// src/agent/engine.rs
use futures::future::join_all;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{error, info, warn};
use twilight_model::channel::message::embed::Embed;

use crate::agent::context::ToolContext;
use crate::agent::registry::ToolRegistry;
use crate::agent::tool::{ToolOutput, ToolResult};
use crate::gemini::builder::RequestBuilder;
use crate::gemini::client::GeminiClient;
use crate::gemini::convert::{json_to_prost_struct, prost_struct_to_json};
use crate::googleapis::google::ai::generativelanguage::v1beta::part::Data;
use crate::googleapis::google::ai::generativelanguage::v1beta::{
    Content, FunctionCall, FunctionResponse, Part,
};

const MAX_TURNS: usize = 4;

pub struct AgentEngine {
    client: Arc<AsyncMutex<GeminiClient>>,
    registry: ToolRegistry,
    model: String,
    system_instruction: String,
}

impl AgentEngine {
    pub fn new(
        client: Arc<AsyncMutex<GeminiClient>>,
        registry: ToolRegistry,
        model: impl Into<String>,
        system_instruction: impl Into<String>,
    ) -> Self {
        Self {
            client,
            registry,
            model: model.into(),
            system_instruction: system_instruction.into(),
        }
    }

    pub async fn run(&self, ctx: ToolContext, mut history: Vec<Content>) -> anyhow::Result<()> {
        let _ = ctx.http.create_typing_trigger(ctx.channel_id).await;

        let mut collected_embeds: Vec<Embed> = Vec::new();
        let mut final_text_response: Option<String> = None;

        for step in 0..MAX_TURNS {
            let sanitized_history = sanitize_history(&history);

            let req = RequestBuilder::new(&self.model)
                .system_instruction(&self.system_instruction)
                .history(sanitized_history)
                .tool(self.registry.to_gemini_tool())
                .build();

            let response = {
                let mut client = self.client.lock().await;
                client.generate_content(req).await?
            };

            let candidate = match response.candidates.into_iter().next() {
                Some(c) => c,
                None => {
                    warn!("Gemini returned no candidates");
                    break;
                }
            };

            let model_content = candidate.content.unwrap_or_else(|| Content {
                role: "model".into(),
                parts: vec![],
            });

            let mut function_calls = Vec::new();
            let mut text_parts = Vec::new();

            for part in &model_content.parts {
                match &part.data {
                    Some(Data::FunctionCall(call)) => function_calls.push(call.clone()),
                    Some(Data::Text(txt)) if !txt.trim().is_empty() => text_parts.push(txt.clone()),
                    _ => {}
                }
            }

            // Save model response to history
            history.push(model_content.clone());
            ctx.cache.push(ctx.channel_id, model_content);

            if !text_parts.is_empty() {
                final_text_response = Some(text_parts.join("\n"));
            }

            // If no tools requested, we are done
            if function_calls.is_empty() {
                info!(step, "No further tool calls. Completing agent turn.");
                break;
            }

            info!(
                step,
                call_count = function_calls.len(),
                "Executing batched tool calls"
            );

            // Execute all tools in parallel
            let tool_futures = function_calls.iter().map(|call| {
                let ctx = ctx.clone();
                let registry = self.registry.clone();
                let call = call.clone();

                async move {
                    let tool = registry.get(&call.name);
                    let args = call
                        .args
                        .as_ref()
                        .map(prost_struct_to_json)
                        .unwrap_or_default();

                    let result = match tool {
                        Some(t) => t.execute(&ctx, args).await,
                        None => Err(anyhow::anyhow!("Tool '{}' not found", call.name)),
                    };

                    (call, result)
                }
            });

            let results = join_all(tool_futures).await;

            let mut response_parts = Vec::new();
            let mut has_informational_response = false;

            for (call, output_result) in results {
                let response_json = match output_result {
                    Ok(ToolOutput::Info(ToolResult {
                        model_response,
                        visual_embed,
                    })) => {
                        has_informational_response = true;
                        if let Some(embed) = visual_embed {
                            collected_embeds.push(embed); // Accumulate embed for the final message!
                        }
                        model_response
                    }
                    Ok(ToolOutput::ActionExecuted(msg)) => {
                        serde_json::json!({ "status": "ok", "message": msg })
                    }
                    Ok(ToolOutput::Stop) => {
                        serde_json::json!({ "status": "stopped" })
                    }
                    Err(e) => {
                        error!(tool = %call.name, error = ?e, "Tool execution failed");
                        serde_json::json!({ "status": "error", "error": e.to_string() })
                    }
                };

                response_parts.push(Part {
                    data: Some(Data::FunctionResponse(FunctionResponse {
                        id: call.id,
                        name: call.name,
                        response: Some(json_to_prost_struct(&response_json)),
                        ..Default::default()
                    })),
                    ..Default::default()
                });
            }

            let function_response_content = Content {
                role: "user".into(),
                parts: response_parts,
            };

            history.push(function_response_content.clone());
            ctx.cache.push(ctx.channel_id, function_response_content);

            if !has_informational_response {
                break;
            }
        }

        // =========================================================================
        // ATOMIC FINAL DISPATCH: Send content and all accumulated embeds in 1 message
        // =========================================================================
        self.dispatch_final_response(&ctx, final_text_response, collected_embeds)
            .await?;

        Ok(())
    }

    async fn dispatch_final_response(
        &self,
        ctx: &ToolContext,
        text: Option<String>,
        embeds: Vec<Embed>,
    ) -> anyhow::Result<()> {
        let has_text = text.as_ref().map_or(false, |t| !t.trim().is_empty());
        let has_embeds = !embeds.is_empty();

        if !has_text && !has_embeds {
            return Ok(());
        }

        let channel_id = ctx.channel_id;
        let reply_to = ctx.message.id;

        // If we have text, chunk it if it's over Discord's 2000 char limit
        let content_str = text.unwrap_or_default();
        let chunks: Vec<&str> = if content_str.is_empty() {
            vec![""]
        } else {
            content_str
                .as_bytes()
                .chunks(1900)
                .filter_map(|c| std::str::from_utf8(c).ok())
                .collect()
        };

        // Attach embeds to the first (or only) message chunk
        for (i, chunk) in chunks.iter().enumerate() {
            let mut builder = ctx.http.create_message(channel_id).reply(reply_to);

            if !chunk.is_empty() {
                builder = builder.content(chunk);
            }

            if i == 0 && has_embeds {
                builder = builder.embeds(&embeds);
            }

            builder.await?;
        }

        Ok(())
    }
}

fn sanitize_history(contents: &[Content]) -> Vec<Content> {
    let mut cleaned: Vec<Content> = Vec::new();

    for (i, content) in contents.iter().enumerate() {
        let is_last_turn = i == contents.len() - 1;
        let is_second_to_last = i + 1 == contents.len() - 1;

        let mut parts = Vec::new();

        for part in &content.parts {
            match &part.data {
                Some(Data::Text(t)) if !t.trim().is_empty() => {
                    parts.push(part.clone());
                }
                Some(Data::FunctionCall(call)) => {
                    // Only preserve FunctionCalls if they are part of the active turn
                    if is_second_to_last || is_last_turn {
                        parts.push(part.clone());
                    } else {
                        parts.push(Part {
                            data: Some(Data::Text(format!("[Called tool: {}]", call.name))),
                            ..Default::default()
                        });
                    }
                }
                Some(Data::FunctionResponse(resp)) => {
                    // Only preserve FunctionResponses if they are in the active turn
                    if is_last_turn {
                        parts.push(part.clone());
                    } else {
                        parts.push(Part {
                            data: Some(Data::Text(format!("[Tool {} finished]", resp.name))),
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
        }

        if !parts.is_empty() {
            cleaned.push(Content {
                role: content.role.clone(),
                parts,
            });
        }
    }

    // Merge consecutive turns with the same role (e.g. user + user)
    let mut alternating: Vec<Content> = Vec::new();
    for content in cleaned {
        if let Some(last) = alternating.last_mut() {
            // Do not merge if the turn is an in-flight tool call/response
            let last_has_tool = last.parts.iter().any(|p| {
                matches!(
                    p.data,
                    Some(Data::FunctionCall(_)) | Some(Data::FunctionResponse(_))
                )
            });
            let curr_has_tool = content.parts.iter().any(|p| {
                matches!(
                    p.data,
                    Some(Data::FunctionCall(_)) | Some(Data::FunctionResponse(_))
                )
            });

            if last.role == content.role && !last_has_tool && !curr_has_tool {
                last.parts.extend(content.parts);
                continue;
            }
        }
        alternating.push(content);
    }

    // Ensure history starts with a user message
    if let Some(first) = alternating.first() {
        if first.role == "model" {
            alternating.remove(0);
        }
    }

    alternating
}
