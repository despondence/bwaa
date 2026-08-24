// src/sandbox.rs
use crate::cache::{CachedMessage, ChannelCache};
use deno_core::{
    JsRuntime, OpState, PollEventLoopOptions, RuntimeOptions, extension, op2, scope, serde_v8, v8,
};
use deno_error::JsError;
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use twilight_http::Client as HttpClient;
use twilight_model::gateway::payload::incoming::MessageCreate;
use twilight_model::id::Id;
use twilight_model::id::marker::ChannelMarker;

#[derive(Debug, thiserror::Error, JsError)]
#[class(generic)]
#[error("{0}")]
pub struct SandboxError(pub String);

pub struct BotContext {
    pub http: Arc<HttpClient>,
    pub cache: ChannelCache,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<twilight_model::id::marker::MessageMarker>,
}

#[op2]
#[serde]
pub fn op_get_channel_messages(
    state: Rc<RefCell<OpState>>,
    #[serde] limit: usize,
) -> Result<Vec<CachedMessage>, SandboxError> {
    let op_state = state.borrow();
    let ctx = op_state.borrow::<BotContext>();
    Ok(ctx.cache.to_cached_messages(ctx.channel_id, limit))
}

#[op2]
#[string]
pub async fn op_send_reply(
    state: Rc<RefCell<OpState>>,
    #[string] content: String,
) -> Result<String, SandboxError> {
    let (http, channel_id, message_id) = {
        let op_state = state.borrow();
        let ctx = op_state.borrow::<BotContext>();
        (Arc::clone(&ctx.http), ctx.channel_id, ctx.message_id)
    };

    http.create_message(channel_id)
        .content(&content)
        .reply(message_id)
        .await
        .map_err(|e| SandboxError(e.to_string()))?;

    Ok("Message dispatched".into())
}

extension!(bot_sandbox, ops = [op_get_channel_messages, op_send_reply]);

const JS_BOOTSTRAP: &str = r#"
((globalThis) => {
    const { ops } = Deno.core;

    globalThis.__console_logs = [];
    globalThis.console = {
        log: (...args) => {
            globalThis.__console_logs.push(
                args.map(a => (typeof a === 'object' ? JSON.stringify(a) : String(a))).join(' ')
            );
        },
        error: (...args) => {
            globalThis.__console_logs.push(
                "[ERROR] " + args.map(a => (typeof a === 'object' ? JSON.stringify(a) : String(a))).join(' ')
            );
        },
        warn: (...args) => {
            globalThis.__console_logs.push(
                "[WARN] " + args.map(a => (typeof a === 'object' ? JSON.stringify(a) : String(a))).join(' ')
            );
        }
    };

    class Channel {
        constructor(id) {
            this.id = id;
        }

        getHistory(limit = 10) {
            return ops.op_get_channel_messages(limit);
        }
    }

    class Message {
        constructor(data) {
            this.id = data.id;
            this.content = data.content;
            this.author = data.author;
            this.channel = new Channel(data.channelId);
        }

        async reply(text) {
            return await ops.op_send_reply(text);
        }
    }

    globalThis.Message = Message;
    globalThis.Channel = Channel;
})(globalThis);
"#;

pub fn run_js_eval_sync(
    http: Arc<HttpClient>,
    cache: ChannelCache,
    msg: MessageCreate,
    js_code: String,
    timeout_duration: Duration,
) -> anyhow::Result<JsonValue> {
    let local_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    local_rt.block_on(async move {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![bot_sandbox::init()],
            ..Default::default()
        });

        {
            let state = runtime.op_state();
            state.borrow_mut().put(BotContext {
                http,
                cache,
                channel_id: msg.channel_id,
                message_id: msg.id,
            });
        }

        runtime.execute_script("bootstrap.js", JS_BOOTSTRAP)?;

        // 1. Safely hydrate context using standard JSON serialization for zero syntax errors
        let hydration_script = format!(
            "globalThis.message = new Message({{ id: {}, content: {}, author: {}, channelId: {} }});",
            serde_json::to_string(&msg.id.to_string())?,
            serde_json::to_string(&msg.content)?,
            serde_json::to_string(&msg.author.name)?,
            serde_json::to_string(&msg.channel_id.to_string())?
        );
        runtime.execute_script("hydrate.js", hydration_script)?;

        // 2. Wrap using eval() so expressions automatically yield their return value
        let escaped_code = serde_json::to_string(&js_code)?;
        let wrapped_script = format!(
            r#"(async () => {{
                globalThis.__console_logs = [];
                let __ret = await (async () => {{
                    return eval({escaped_code});
                }})();
                return {{
                    return_value: __ret !== undefined ? __ret : null,
                    logs: globalThis.__console_logs || []
                }};
            }})()"#
        );

        let promise_val = runtime.execute_script("user_code.js", wrapped_script)?;

        // 3. Resolve promise with event loop pump
        let resolve_fut = runtime.resolve(promise_val);
        let resolved_val = tokio::time::timeout(
            timeout_duration,
            runtime.with_event_loop_promise(resolve_fut, PollEventLoopOptions::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Execution timed out"))?
        .map_err(|e| anyhow::anyhow!("Runtime error: {}", e))?;

        // 4. Extract into serde_json::Value
        scope!(scope, &mut runtime);
        let local_val = v8::Local::new(scope, resolved_val);

        let json_value: JsonValue = serde_v8::from_v8(scope, local_val)
            .unwrap_or(JsonValue::Null);

        Ok(json_value)
    })
}
