//! The OpenAI-compatible client: listing models and streaming replies.
//!
//! Any endpoint that speaks the standard Chat Completions protocol works,
//! which covers OpenAI, OpenRouter, Groq, Ollama, and most local servers.

use std::time::Duration;

use futures_util::StreamExt;

use crate::config::Config;
use crate::model::{ChatTurn, ModelInfo};

pub struct Upstream {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

/// One element pulled from a reply stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A chunk of assistant text.
    Delta(String),
    /// The provider signalled a clean end.
    Done,
    /// The provider reported an error inside the stream.
    Error(String),
}

#[derive(Debug)]
pub enum UpstreamError {
    Http(reqwest::Error),
    /// The provider answered with a non-2xx status, with whatever it said.
    HttpStatus(u16, String),
    /// The response was not the shape the protocol promises.
    Protocol(String),
    NoKey,
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpstreamError::Http(e) => write!(f, "could not reach the model server: {e}"),
            UpstreamError::HttpStatus(code, detail) if detail.is_empty() => {
                write!(f, "the model server answered {code}")
            }
            UpstreamError::HttpStatus(code, detail) => {
                write!(f, "the model server answered {code}: {detail}")
            }
            UpstreamError::Protocol(msg) => {
                write!(f, "unexpected answer from the model server: {msg}")
            }
            UpstreamError::NoKey => write!(
                f,
                "no API key is set. Start with SELF_API_KEY or OPENAI_API_KEY, or point SELF_BASE_URL at a local server such as Ollama"
            ),
        }
    }
}

impl std::error::Error for UpstreamError {}

impl Upstream {
    pub fn new(cfg: &Config) -> Upstream {
        // A read timeout guards against a stalled connection without killing a
        // long but steady stream, which a whole-request timeout would.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(cfg.timeout_secs.max(15)))
            .build()
            .expect("build HTTP client");
        Upstream {
            http,
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            api_key: cfg.api_key.clone(),
        }
    }

    /// True when no key is configured. Local servers do not need one, so this
    /// is a hint for the UI rather than a hard failure.
    pub fn lacks_key(&self) -> bool {
        self.api_key.is_none()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn authorize(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => req.header("Authorization", format!("Bearer {key}")),
            None => req,
        }
    }

    fn is_ollama(&self) -> bool {
        self.base_url.contains("11434") || self.base_url.contains("/ollama")
    }

    /// The models this server offers, best effort: an unreachable or
    /// key-less `/models` returns an empty list and the UI falls back to the
    /// configured default rather than blocking.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, UpstreamError> {
        let res = self
            .authorize(self.http.get(self.endpoint("models")))
            .send()
            .await
            .map_err(UpstreamError::Http)?;
        let status = res.status();
        let body = res.text().await.map_err(UpstreamError::Http)?;
        if !status.is_success() {
            return Err(UpstreamError::HttpStatus(status.as_u16(), snippet(&body)));
        }
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|_| UpstreamError::Protocol("invalid JSON from /models".into()))?;

        let mut out: Vec<ModelInfo> = Vec::new();
        for item in json
            .get("data")
            .and_then(|data| data.as_array())
            .into_iter()
            .flatten()
        {
            let Some(id) = item.get("id").and_then(|id| id.as_str()) else {
                continue;
            };
            if id.trim().is_empty() {
                continue;
            }
            let id = if self.is_ollama() {
                id.split(':').next().unwrap_or(id).to_string()
            } else {
                id.to_string()
            };
            if out.iter().any(|model| model.id == id) {
                continue;
            }
            out.push(ModelInfo {
                label: label_for(&id),
                id,
            });
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Streams a reply as deltas, using exactly the standard Chat Completions
    /// body so any compatible server accepts it.
    pub fn stream(
        &self,
        model: &str,
        messages: &[ChatTurn],
    ) -> impl futures_util::Stream<Item = Result<StreamEvent, UpstreamError>> + '_ {
        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });
        let req = self.authorize(
            self.http
                .post(self.endpoint("chat/completions"))
                .json(&body),
        );
        let lacks_key = self.lacks_key();
        let base = self.base_url.clone();

        async_stream::stream! {
            if lacks_key && !base.contains("localhost") && !base.contains("127.0.0.1") {
                yield Err(UpstreamError::NoKey);
                return;
            }
            let res = match req.send().await {
                Ok(res) => res,
                Err(e) => { yield Err(UpstreamError::Http(e)); return; }
            };
            let status = res.status();
            if !status.is_success() {
                let detail = res.text().await.map(|body| snippet(&body)).unwrap_or_default();
                yield Err(UpstreamError::HttpStatus(status.as_u16(), detail));
                return;
            }

            let mut stream = res.bytes_stream();
            let mut buffer: Vec<u8> = Vec::new();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(chunk) => buffer.extend_from_slice(&chunk),
                    Err(e) => { yield Err(UpstreamError::Http(e)); return; }
                }
                while let Some(newline) = buffer.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=newline).collect();
                    let line = String::from_utf8_lossy(&line);
                    match parse_sse_line(line.trim_end_matches(['\r', '\n'])) {
                        Some(StreamEvent::Done) => { yield Ok(StreamEvent::Done); return; }
                        Some(event) => yield Ok(event),
                        None => {}
                    }
                }
            }
        }
    }

    /// A single round trip with no streaming, used for memory extraction where
    /// the whole reply is needed at once.
    pub async fn complete(
        &self,
        model: &str,
        messages: &[ChatTurn],
    ) -> Result<String, UpstreamError> {
        let body = serde_json::json!({ "model": model, "messages": messages, "stream": false });
        let res = self
            .authorize(
                self.http
                    .post(self.endpoint("chat/completions"))
                    .json(&body),
            )
            .send()
            .await
            .map_err(UpstreamError::Http)?;
        let status = res.status();
        let text = res.text().await.map_err(UpstreamError::Http)?;
        if !status.is_success() {
            return Err(UpstreamError::HttpStatus(status.as_u16(), snippet(&text)));
        }
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| UpstreamError::Protocol("invalid JSON from chat/completions".into()))?;
        Ok(content_of(&json).unwrap_or_default())
    }
}

/// Reads assistant text out of either the streaming shape (`delta`) or the
/// non-streaming one (`message`).
fn content_of(json: &serde_json::Value) -> Option<String> {
    json.get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("delta").or_else(|| choice.get("message")))
        .and_then(|part| part.get("content"))
        .and_then(|content| content.as_str())
        .map(str::to_string)
}

/// Turns one SSE line into an event. Pure and testable: a line is the unit the
/// protocol works in, so anything reading a stream buffers up to a newline.
pub fn parse_sse_line(line: &str) -> Option<StreamEvent> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return None; // blank separator or comment (heartbeat)
    }
    let (field, value) = match line.split_once(':') {
        Some((field, value)) => (field.trim(), value.trim_start()),
        None => (line, ""),
    };
    if field != "data" {
        return None;
    }
    if value == "[DONE]" {
        return Some(StreamEvent::Done);
    }
    let json: serde_json::Value = serde_json::from_str(value).ok()?;
    if let Some(err) = json.get("error") {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("the model server reported an error")
            .to_string();
        return Some(StreamEvent::Error(message));
    }
    match content_of(&json) {
        Some(text) if !text.is_empty() => Some(StreamEvent::Delta(text)),
        _ => None,
    }
}

fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    let cut: String = trimmed.chars().take(300).collect();
    if trimmed.chars().count() > 300 {
        format!("{cut}…")
    } else {
        cut
    }
}

fn label_for(id: &str) -> String {
    let lower = id.to_lowercase();
    if let Some(rest) = lower.strip_prefix("claude-") {
        format!("Claude {rest}")
    } else if let Some(rest) = lower.strip_prefix("gemini-") {
        format!("Gemini {rest}")
    } else if let Some(rest) = lower.strip_prefix("meta-llama/") {
        rest.replace('-', " ")
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_lines_parse_into_events() {
        assert_eq!(parse_sse_line(""), None);
        assert_eq!(parse_sse_line(": keep-alive"), None);
        assert_eq!(
            parse_sse_line("event: delta"),
            None,
            "only data fields carry text"
        );
        assert_eq!(parse_sse_line("data: [DONE]"), Some(StreamEvent::Done));

        let delta = parse_sse_line(r#"data: {"choices":[{"delta":{"content":"Hi "}}]}"#);
        assert_eq!(delta, Some(StreamEvent::Delta("Hi ".into())));

        let err = parse_sse_line(r#"data: {"error":{"message":"boom"}}"#);
        assert_eq!(err, Some(StreamEvent::Error("boom".into())));

        // Some providers echo the non-streaming shape instead of a delta.
        let msg = parse_sse_line(r#"data: {"choices":[{"message":{"content":"yo"}}]}"#);
        assert_eq!(msg, Some(StreamEvent::Delta("yo".into())));
    }

    #[test]
    fn sse_ignores_usage_and_empty_deltas() {
        assert_eq!(
            parse_sse_line(r#"data: {"usage":{"total_tokens":3}}"#),
            None
        );
        assert_eq!(
            parse_sse_line(r#"data: {"choices":[{"delta":{"content":null}}]}"#),
            None
        );
        assert_eq!(parse_sse_line("data: not json"), None);
    }

    #[test]
    fn snippets_are_truncated_for_error_messages() {
        assert_eq!(snippet("  short  "), "short");
        let long = snippet(&"x".repeat(400));
        assert!(long.ends_with('…'));
        assert_eq!(long.chars().count(), 301);
    }

    #[test]
    fn labels_read_well() {
        assert_eq!(label_for("gpt-4o-mini"), "gpt-4o-mini");
        assert_eq!(label_for("claude-sonnet-4-5"), "Claude sonnet-4-5");
        assert_eq!(label_for("gemini-2.0-flash"), "Gemini 2.0-flash");
    }
}
