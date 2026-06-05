// Anthropic API compatible server routes for LlamaEngine
use super::llama_engine::SamplingParams;
use super::llama_server::{
    build_chat_prompt, validate_prompt_and_tokens, ApiServerState, AppError,
    ChatMessage as LlamaChatMessage,
};
use axum::{
    extract::State,
    response::{sse::Event, IntoResponse, Sse},
    Json,
};
use futures_util::{
    stream::{self, Stream},
    StreamExt,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

// ==================== Request / Response types ====================

#[derive(Debug, Deserialize)]
pub struct MessagesRequest {
    pub model: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub thinking: Option<ThinkingConfig>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct MessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: String,
    pub usage: AnthropicUsage,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "thinking")]
    Thinking { thinking: String, signature: String },
    #[serde(rename = "text")]
    Text { text: String },
}

#[derive(Debug, Serialize)]
pub struct AnthropicUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

// ==================== Handler ====================

pub async fn messages_handler(
    State(state): State<ApiServerState>,
    Json(req): Json<MessagesRequest>,
) -> Result<axum::response::Response, AppError> {
    info!(
        "Anthropic messages request: {} messages, stream={}",
        req.messages.len(),
        req.stream
    );

    if req.stream {
        Ok(messages_stream(state, req).await?.into_response())
    } else {
        Ok(messages_non_stream(state, req).await?.into_response())
    }
}

async fn messages_non_stream(
    state: ApiServerState,
    req: MessagesRequest,
) -> Result<Json<MessagesResponse>, AppError> {
    let prompt = build_anthropic_prompt(&req);
    validate_prompt_and_tokens(&state.security.limits, &prompt, req.max_tokens)?;
    let _generation_permit = state.try_generation_permit()?;
    let engine = state.engine.read().await;
    let max_tokens = req.max_tokens.unwrap_or(1024);
    let mut sampling = SamplingParams::default();
    if let Some(t) = req.temperature {
        sampling.temperature = t;
    }

    let (text, prompt_tokens, completion_tokens) = engine
        .generate_with_cached_model_sampling(&prompt, max_tokens, &sampling)
        .await?;

    let content = if req.thinking.is_some() {
        split_thinking_text(&text)
    } else {
        vec![ContentBlock::Text { text }]
    };

    let response = MessagesResponse {
        id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: req.model.unwrap_or_else(|| "llama.cpp".to_string()),
        stop_reason: "end_turn".to_string(),
        usage: AnthropicUsage {
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
        },
    };

    Ok(Json(response))
}

async fn messages_stream(
    state: ApiServerState,
    req: MessagesRequest,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, AppError> {
    let prompt = build_anthropic_prompt(&req);
    validate_prompt_and_tokens(&state.security.limits, &prompt, req.max_tokens)?;
    let generation_permit = state.try_generation_permit()?;
    let sse_permit = state.try_sse_permit()?;
    let engine = state.engine.read().await;
    let max_tokens = req.max_tokens.unwrap_or(1024);
    let mut sampling = SamplingParams::default();
    if let Some(t) = req.temperature {
        sampling.temperature = t;
    }
    if let Some(ref thinking) = req.thinking {
        if let Some(budget) = thinking.budget_tokens {
            sampling.thinking_budget_tokens = Some(budget);
        }
    }

    let token_stream = engine
        .stream_with_cached_model_sampling(&prompt, max_tokens, &sampling)
        .await?;

    let has_thinking = req.thinking.is_some();
    let splitter = Arc::new(std::sync::Mutex::new(StreamSplitter::new(has_thinking)));
    let block_index = Arc::new(std::sync::Mutex::new(0usize));
    let block_started = Arc::new(std::sync::Mutex::new(false));
    let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let model_name = req.model.unwrap_or_else(|| "llama.cpp".to_string());

    // message_start must be the very first event per SSE spec
    let message_start_event = stream::once(async move {
        Ok::<Event, std::convert::Infallible>(
            Event::default()
                .event("message_start")
                .json_data(serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": message_id,
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "model": model_name,
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": {
                            "input_tokens": prompt.len() / 4,
                            "output_tokens": 0
                        }
                    }
                }))
                .unwrap_or_else(|_| Event::default().event("error").data("json failed")),
        )
    });

    let splitter_body = splitter.clone();
    let block_index_body = block_index.clone();
    let block_started_body = block_started.clone();

    let body_stream = token_stream.flat_map(move |token_res| {
        let mut events: Vec<Result<Event, std::convert::Infallible>> = Vec::new();
        match token_res {
            Ok(token) => {
                for ev in splitter_body.lock().unwrap().push(&token) {
                    append_event(
                        &mut events,
                        &ev,
                        &mut *block_index_body.lock().unwrap(),
                        &mut *block_started_body.lock().unwrap(),
                    );
                }
            }
            Err(e) => {
                error!("Anthropic stream token error: {}", e);
            }
        }
        stream::iter(events)
    });

    let stream = message_start_event.chain(body_stream);

    // Footer with flush and stop events. Keepalive pings must not be chained before
    // this finite footer, otherwise message_stop can never be observed.
    let footer = stream::once(async move {
        let mut events: Vec<Result<Event, std::convert::Infallible>> = Vec::new();
        for ev in splitter.lock().unwrap().flush() {
            append_event(
                &mut events,
                &ev,
                &mut *block_index.lock().unwrap(),
                &mut *block_started.lock().unwrap(),
            );
        }
        append_footer_events(
            &mut events,
            *block_index.lock().unwrap(),
            *block_started.lock().unwrap(),
        );
        stream::iter(events)
    })
    .flatten();

    let permits = Arc::new((generation_permit, sse_permit));
    let stream = stream.chain(footer).map(move |event| {
        let _keep_permits_alive = &permits;
        event
    });
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

// ==================== Helpers ====================

fn build_anthropic_prompt(req: &MessagesRequest) -> String {
    let mut messages: Vec<LlamaChatMessage> = req
        .messages
        .iter()
        .map(|m| LlamaChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    let system_content = if let Some(ref thinking) = req.thinking {
        if thinking.thinking_type == "enabled" {
            let base = req.system.clone().unwrap_or_default();
            let instruction = if let Some(budget) = thinking.budget_tokens {
                format!(
                    "Please reason step by step inside <think> tags before giving your final answer. \
                    Aim to use approximately {} tokens for your thinking, then provide your final answer after \
</think> tag.",
                    budget
                )
            } else {
                "Please reason step by step inside <think> tags before giving your final answer."
                    .to_string()
            };
            if base.is_empty() {
                instruction
            } else {
                format!("{}\n\n{}", base, instruction)
            }
        } else {
            req.system.clone().unwrap_or_default()
        }
    } else {
        req.system.clone().unwrap_or_default()
    };

    if !system_content.is_empty() {
        messages.insert(
            0,
            LlamaChatMessage {
                role: "system".to_string(),
                content: system_content,
            },
        );
    }

    build_chat_prompt(&messages)
}

fn split_thinking_text(text: &str) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    let mut rest = text;

    while let Some(start) = rest.find("<think>") {
        let before = &rest[..start];
        if !before.trim().is_empty() {
            blocks.push(ContentBlock::Text {
                text: before.to_string(),
            });
        }
        rest = &rest[start + "<think>".len()..];
        if let Some(end) = rest.find("</think>") {
            let thinking = &rest[..end];
            blocks.push(ContentBlock::Thinking {
                thinking: thinking.to_string(),
                signature: "".to_string(),
            });
            rest = &rest[end + "</think>".len()..];
        } else {
            blocks.push(ContentBlock::Thinking {
                thinking: rest.to_string(),
                signature: "".to_string(),
            });
            rest = "";
            break;
        }
    }

    if !rest.trim().is_empty() {
        blocks.push(ContentBlock::Text {
            text: rest.to_string(),
        });
    }

    if blocks.is_empty() {
        blocks.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    blocks
}

fn sse_event(name: &str, payload: serde_json::Value) -> Result<Event, std::convert::Infallible> {
    Ok(Event::default()
        .event(name)
        .json_data(payload)
        .unwrap_or_else(|_| {
            Event::default()
                .event("error")
                .data("json serialization failed")
        }))
}

fn append_footer_events(
    events: &mut Vec<Result<Event, std::convert::Infallible>>,
    final_index: usize,
    block_started: bool,
) {
    if block_started {
        events.push(sse_event(
            "content_block_stop",
            serde_json::json!({
                "type": "content_block_stop",
                "index": final_index
            }),
        ));
    }
    events.push(sse_event(
        "message_delta",
        serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
                "stop_sequence": null
            },
            "usage": {
                "output_tokens": 0
            }
        }),
    ));
    events.push(sse_event(
        "message_stop",
        serde_json::json!({"type": "message_stop"}),
    ));
}

#[cfg(test)]
fn footer_event_names(block_started: bool) -> Vec<&'static str> {
    let mut names = Vec::new();
    if block_started {
        names.push("content_block_stop");
    }
    names.push("message_delta");
    names.push("message_stop");
    names
}

fn append_event(
    events: &mut Vec<Result<Event, std::convert::Infallible>>,
    ev: &SplitEvent,
    block_index: &mut usize,
    block_started: &mut bool,
) {
    match ev {
        SplitEvent::Text(text) => {
            if !*block_started {
                events.push(sse_event(
                    "content_block_start",
                    serde_json::json!({
                        "type": "content_block_start",
                        "index": *block_index,
                        "content_block": {"type": "text"}
                    }),
                ));
                *block_started = true;
            }
            events.push(sse_event(
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta",
                    "index": *block_index,
                    "delta": {"type": "text_delta", "text": text}
                }),
            ));
        }
        SplitEvent::StartThinking => {
            if *block_started {
                events.push(sse_event(
                    "content_block_stop",
                    serde_json::json!({
                        "type": "content_block_stop",
                        "index": *block_index
                    }),
                ));
                *block_index += 1;
            }
            events.push(sse_event(
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start",
                    "index": *block_index,
                    "content_block": {"type": "thinking"}
                }),
            ));
            *block_started = true;
        }
        SplitEvent::Thinking(text) => {
            events.push(sse_event(
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta",
                    "index": *block_index,
                    "delta": {"type": "thinking_delta", "thinking": text}
                }),
            ));
        }
        SplitEvent::EndThinking => {
            events.push(sse_event(
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop",
                    "index": *block_index
                }),
            ));
            *block_index += 1;
            *block_started = false;
        }
    }
}

// ==================== Stream splitting for thinking tags ====================

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockKind {
    Text,
    Thinking,
}

#[derive(Debug)]
enum SplitEvent {
    Text(String),
    StartThinking,
    Thinking(String),
    EndThinking,
}

struct StreamSplitter {
    state: BlockKind,
    carry: String,
    enabled: bool,
}

impl StreamSplitter {
    fn new(enabled: bool) -> Self {
        Self {
            state: BlockKind::Text,
            carry: String::new(),
            enabled,
        }
    }

    fn push(&mut self, text: &str) -> Vec<SplitEvent> {
        if !self.enabled {
            return vec![SplitEvent::Text(text.to_string())];
        }

        let mut combined = String::new();
        if !self.carry.is_empty() {
            combined.push_str(&self.carry);
            self.carry.clear();
        }
        combined.push_str(text);

        let mut events = Vec::new();
        let mut pos = 0;

        while pos < combined.len() {
            let rest = &combined[pos..];
            match self.state {
                BlockKind::Text => {
                    if let Some(idx) = rest.find("<think>") {
                        if idx > 0 {
                            events.push(SplitEvent::Text(rest[..idx].to_string()));
                        }
                        events.push(SplitEvent::StartThinking);
                        self.state = BlockKind::Thinking;
                        pos += idx + "<think>".len();
                    } else {
                        let (safe, carry) = Self::split_tail(rest, "<think>");
                        if !safe.is_empty() {
                            events.push(SplitEvent::Text(safe.to_string()));
                        }
                        self.carry = carry.to_string();
                        break;
                    }
                }
                BlockKind::Thinking => {
                    if let Some(idx) = rest.find("</think>") {
                        if idx > 0 {
                            events.push(SplitEvent::Thinking(rest[..idx].to_string()));
                        }
                        events.push(SplitEvent::EndThinking);
                        self.state = BlockKind::Text;
                        pos += idx + "</think>".len();
                    } else {
                        let (safe, carry) = Self::split_tail(rest, "</think>");
                        if !safe.is_empty() {
                            events.push(SplitEvent::Thinking(safe.to_string()));
                        }
                        self.carry = carry.to_string();
                        break;
                    }
                }
            }
        }

        events
    }

    fn flush(&mut self) -> Vec<SplitEvent> {
        if !self.enabled {
            if self.carry.is_empty() {
                return Vec::new();
            }
            let text = self.carry.clone();
            self.carry.clear();
            return vec![SplitEvent::Text(text)];
        }

        let mut events = Vec::new();
        if !self.carry.is_empty() {
            match self.state {
                BlockKind::Text => events.push(SplitEvent::Text(self.carry.clone())),
                BlockKind::Thinking => events.push(SplitEvent::Thinking(self.carry.clone())),
            }
            self.carry.clear();
        }
        if self.state == BlockKind::Thinking {
            events.push(SplitEvent::EndThinking);
        }
        events
    }

    fn split_tail<'a>(text: &'a str, marker: &str) -> (&'a str, &'a str) {
        let max_scan = marker.len().saturating_sub(1).min(text.len());
        let mut carry_len = 0usize;
        for l in 1..=max_scan {
            let start = text.len().saturating_sub(l);
            if !text.is_char_boundary(start) {
                continue;
            }
            let suf = &text[start..];
            if marker.starts_with(suf) {
                carry_len = l;
            }
        }
        if carry_len == 0 {
            return (text, "");
        }
        let split = text.len() - carry_len;
        if !text.is_char_boundary(split) {
            return (text, "");
        }
        (&text[..split], &text[split..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_splitter_flushes_unclosed_thinking_before_footer() {
        let mut splitter = StreamSplitter::new(true);
        let events = splitter.push("hello <think>private reasoning");
        assert!(events
            .iter()
            .any(|ev| matches!(ev, SplitEvent::StartThinking)));
        let flushed = splitter.flush();
        assert!(matches!(flushed.last(), Some(SplitEvent::EndThinking)));
    }

    #[test]
    fn footer_events_always_end_with_message_stop() {
        assert_eq!(
            footer_event_names(false),
            vec!["message_delta", "message_stop"]
        );
        assert_eq!(
            footer_event_names(true),
            vec!["content_block_stop", "message_delta", "message_stop"]
        );
    }
}
