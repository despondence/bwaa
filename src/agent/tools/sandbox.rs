// src/agent/tools/sandbox.rs
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use std::time::{Duration, Instant};
use twilight_model::channel::message::embed::{Embed, EmbedField};

use crate::agent::context::ToolContext;
use crate::agent::tool::{Tool, ToolOutput, ToolResult};
use crate::sandbox::run_js_eval_sync;

const BWAA_PINK: u32 = 0xF472B6;

#[derive(Debug)]
pub struct ExecuteJsTool;

#[derive(Deserialize)]
struct ExecuteJsArgs {
    code: String,
}

#[async_trait]
impl Tool for ExecuteJsTool {
    fn name(&self) -> &'static str {
        "execute_js"
    }

    fn description(&self) -> &'static str {
        "Execute dynamic JavaScript inside an isolated Deno runtime with channel context."
    }

    fn parameters_schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "JS code to execute in sandbox."
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: JsonValue) -> anyhow::Result<ToolOutput> {
        let args: ExecuteJsArgs = serde_json::from_value(args)?;
        let http = ctx.http.clone();
        let cache = ctx.cache.clone();
        let msg = ctx.message.clone();
        let code = args.code;

        let start = Instant::now();
        let code_clone = code.clone();

        let eval_result = tokio::task::spawn_blocking(move || {
            run_js_eval_sync(http, cache, msg, code_clone, Duration::from_secs(5))
        })
        .await?;

        let elapsed = start.elapsed().as_millis();

        let (output_field_name, output_text, is_success) = match &eval_result {
            Ok(output) => {
                let logs = output
                    .get("logs")
                    .and_then(|l| l.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();

                let ret_val = output.get("return_value");

                let display_text = if !logs.is_empty() {
                    if let Some(val) = ret_val.filter(|v| !v.is_null()) {
                        format!(
                            "{}\n\n[Returned]: {}",
                            logs,
                            serde_json::to_string_pretty(val).unwrap_or_default()
                        )
                    } else {
                        logs // If only console.log was used, show just the logs!
                    }
                } else if let Some(val) = ret_val {
                    if val.is_null() {
                        "undefined".to_string()
                    } else {
                        serde_json::to_string_pretty(val).unwrap_or_else(|_| val.to_string())
                    }
                } else {
                    "undefined".to_string()
                };

                ("✨ Output", display_text, true)
            }
            Err(e) => ("💥 Error", e.to_string(), false),
        };

        // Construct the embed without sending it
        let embed = Embed {
            title: Some(if is_success {
                "⚡ JS Sandbox Executed".to_string()
            } else {
                "⚠️ JS Execution Failed".to_string()
            }),
            description: Some(format!("```js\n{}\n```", truncate_str(&code, 900))),
            fields: vec![EmbedField {
                name: format!("{output_field_name} ({elapsed}ms)"),
                value: format!("```json\n{}\n```", truncate_str(&output_text, 900)),
                inline: false,
            }],
            color: Some(if is_success { BWAA_PINK } else { 0xEF4444 }),
            author: None,
            footer: None,
            image: None,
            kind: "rich".to_string(),
            provider: None,
            thumbnail: None,
            timestamp: None,
            url: None,
            video: None,
        };

        let response_data = match eval_result {
            Ok(val) => json!({ "status": "success", "result": val }),
            Err(e) => json!({ "status": "error", "error": e.to_string() }),
        };

        // Propagate both the model feedback and visual embed up
        Ok(ToolOutput::Info(ToolResult::info_with_embed(
            response_data,
            embed,
        )))
    }
}

fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.len() > max_chars {
        &s[..max_chars]
    } else {
        s
    }
}
