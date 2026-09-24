//! HTTP surface: conversations, messages (streamed), models and memories.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use futures_util::{Stream, StreamExt};
use serde::Deserialize;

use crate::config::Config;
use crate::db::{Store, StoreError};
use crate::model::{ChatTurn, Conversation, Memory, Message, ModelInfo, SendMessage, valid_kind};
use crate::prompts;
use crate::upstream::{StreamEvent, Upstream, UpstreamError};
use crate::{assets, memory};

pub struct AppState {
    pub store: Arc<Store>,
    pub cfg: Config,
    pub upstream: Arc<Upstream>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(get_config))
        .route("/api/models", get(list_models))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/api/conversations/{id}",
            get(get_conversation)
                .patch(update_conversation)
                .delete(delete_conversation),
        )
        .route(
            "/api/conversations/{id}/messages",
            get(list_messages).post(send_message),
        )
        .route("/api/conversations/{id}/remember", post(remember_now))
        .route("/api/memories", get(list_memories).post(create_memory))
        .route(
            "/api/memories/{id}",
            patch(update_memory).delete(delete_memory),
        )
        .fallback(assets::serve)
        .with_state(state)
}

// ---------- errors ----------

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        ApiError(status, message.into())
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ApiError::new(StatusCode::NOT_FOUND, "not found"),
            other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        }
    }
}

impl From<UpstreamError> for ApiError {
    fn from(e: UpstreamError) -> Self {
        let status = match e {
            UpstreamError::NoKey => StatusCode::PRECONDITION_REQUIRED,
            UpstreamError::HttpStatus(_, _) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::BAD_GATEWAY,
        };
        ApiError::new(status, e.to_string())
    }
}

impl std::fmt::Debug for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.0, self.1)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

/// Runs a blocking store call off the async runtime.
async fn db<T, F>(store: &Arc<Store>, f: F) -> Result<T, ApiError>
where
    F: FnOnce(&Store) -> Result<T, StoreError> + Send + 'static,
    T: Send + 'static,
{
    let store = store.clone();
    tokio::task::spawn_blocking(move || f(&store))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(ApiError::from)
}

// ---------- meta ----------

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn get_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut value = state.cfg.public();
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "baseUrl".into(),
            serde_json::json!(state.upstream.base_url()),
        );
        obj.insert(
            "hasKey".into(),
            serde_json::json!(!state.upstream.lacks_key()),
        );
    }
    Json(value)
}

/// The model list for the picker: configured names first, otherwise whatever
/// the upstream reports. Never fails the page: an unreachable provider gives
/// the default model instead of an error.
async fn list_models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut models: Vec<ModelInfo> = if state.cfg.models.is_empty() {
        state.upstream.list_models().await.unwrap_or_default()
    } else {
        state
            .cfg
            .models
            .iter()
            .map(|id| ModelInfo {
                id: id.clone(),
                label: id.clone(),
            })
            .collect()
    };
    let has_default = models.iter().any(|m| m.id == state.cfg.default_model);
    if !has_default && !state.cfg.default_model.is_empty() {
        models.insert(
            0,
            ModelInfo {
                id: state.cfg.default_model.clone(),
                label: state.cfg.default_model.clone(),
            },
        );
    }
    Json(models)
}

// ---------- conversations ----------

async fn list_conversations(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
    Ok(Json(db(&state.store, |s| s.list_conversations()).await?))
}

#[derive(Deserialize)]
struct NewConversation {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

async fn create_conversation(
    State(state): State<Arc<AppState>>,
    body: Option<Json<NewConversation>>,
) -> Result<(StatusCode, Json<Conversation>), ApiError> {
    let body = body.map(|Json(b)| b).unwrap_or(NewConversation {
        title: None,
        model: None,
        system_prompt: None,
    });
    let model = body
        .model
        .unwrap_or_else(|| state.cfg.default_model.clone());
    let system_prompt = body.system_prompt.unwrap_or_default();
    let created = db(&state.store, move |s| {
        s.create_conversation(&model, &system_prompt)
    })
    .await?;
    if let Some(title) = body.title.filter(|t| !t.trim().is_empty()) {
        let id = created.id;
        let title = title.trim().to_string();
        let updated = db(&state.store, move |s| {
            s.update_conversation(id, Some(&title), None, None)
        })
        .await?;
        return Ok((StatusCode::CREATED, Json(updated)));
    }
    Ok((StatusCode::CREATED, Json(created)))
}

async fn get_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Conversation>, ApiError> {
    Ok(Json(
        db(&state.store, move |s| s.get_conversation(id)).await?,
    ))
}

#[derive(Deserialize)]
struct ConversationPatch {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

async fn update_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<ConversationPatch>,
) -> Result<Json<Conversation>, ApiError> {
    let title = body.title.map(|t| t.trim().to_string());
    let updated = db(&state.store, move |s| {
        s.update_conversation(
            id,
            title.as_deref(),
            body.model.as_deref(),
            body.system_prompt.as_deref(),
        )
    })
    .await?;
    Ok(Json(updated))
}

async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    db(&state.store, move |s| s.delete_conversation(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_messages(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<Message>>, ApiError> {
    // 404 rather than an empty list, so the UI can tell "new chat" from "gone".
    db(&state.store, move |s| s.get_conversation(id)).await?;
    Ok(Json(db(&state.store, move |s| s.list_messages(id)).await?))
}

// ---------- memories ----------

async fn list_memories(State(state): State<Arc<AppState>>) -> Result<Json<Vec<Memory>>, ApiError> {
    Ok(Json(db(&state.store, |s| s.list_memories()).await?))
}

#[derive(Deserialize)]
struct NewMemory {
    text: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    pinned: Option<bool>,
}

async fn create_memory(
    State(state): State<Arc<AppState>>,
    Json(body): Json<NewMemory>,
) -> Result<(StatusCode, Json<Memory>), ApiError> {
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "a memory needs some text",
        ));
    }
    let kind = match body.kind.as_deref() {
        Some(kind) if valid_kind(kind) => kind.to_string(),
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "unknown memory kind",
            ));
        }
        None => "fact".to_string(),
    };
    let pinned = body.pinned.unwrap_or(false);
    let created = db(&state.store, move |s| s.insert_memory(&text, &kind, "user")).await?;
    if pinned {
        let id = created.id;
        let updated = db(&state.store, move |s| {
            s.update_memory(id, None, None, Some(true), None)
        })
        .await?;
        return Ok((StatusCode::CREATED, Json(updated)));
    }
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize)]
struct MemoryPatch {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    enabled: Option<bool>,
}

async fn update_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<MemoryPatch>,
) -> Result<Json<Memory>, ApiError> {
    if let Some(kind) = body.kind.as_deref()
        && !valid_kind(kind)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unknown memory kind",
        ));
    }
    let text = body.text.map(|t| t.trim().to_string());
    let kind = body.kind;
    let updated = db(&state.store, move |s| {
        s.update_memory(
            id,
            text.as_deref(),
            kind.as_deref(),
            body.pinned,
            body.enabled,
        )
    })
    .await?;
    Ok(Json(updated))
}

async fn delete_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    db(&state.store, move |s| s.delete_memory(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- the reply stream ----------

fn sse(event: &str, data: serde_json::Value) -> Result<Event, Infallible> {
    Ok(Event::default().event(event).data(data.to_string()))
}

/// Sends a message and streams the reply back.
///
/// The shape of the stream is fixed and small:
///   `start`    { messageId, model, memories: [...] }   before any text
///   `thinking` { text }                                reasoning chunks, if any
///   `delta`    { text }                                repeatedly
///   `done`     { messageId }                           the reply is complete
///   `memory`   { added: [...] }                        notes mined afterwards
///   `error`    { message }                             anything went wrong
#[allow(clippy::too_many_lines)]
async fn send_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<SendMessage>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "say something first",
        ));
    }

    let conversation = db(&state.store, move |s| s.get_conversation(id)).await?;
    let model = body
        .model
        .filter(|m| !m.trim().is_empty())
        .or_else(|| Some(conversation.model.clone()).filter(|m| !m.trim().is_empty()))
        .unwrap_or_else(|| state.cfg.default_model.clone());
    if conversation.model.is_empty() {
        let model = model.clone();
        db(&state.store, move |s| {
            s.update_conversation(id, None, Some(&model), None)
        })
        .await?;
    }

    // Remember the user's turn first, so the transcript survives a failed reply.
    let user_text = content.clone();
    let user_message_id = db(&state.store, move |s| {
        s.insert_message(id, "user", &user_text, "", &[])
    })
    .await?;

    // Name the conversation after its first question.
    if conversation.message_count == 0 {
        let title = title_from(&content);
        db(&state.store, move |s| {
            s.update_conversation(id, Some(&title), None, None)
        })
        .await?;
    }

    // Choose the memories that inform this reply, and note which they were.
    let memories = if state.cfg.memory_enabled {
        let all = db(&state.store, |s| s.list_memories()).await?;
        memory::select_for_prompt(&all, &content, state.cfg.memory_budget, chrono::Utc::now())
    } else {
        Vec::new()
    };
    let memory_ids: Vec<i64> = memories.iter().map(|m| m.id).collect();
    if !memory_ids.is_empty() {
        let ids = memory_ids.clone();
        db(&state.store, move |s| s.mark_memories_used(&ids)).await?;
    }

    let persona = if conversation.system_prompt.trim().is_empty() {
        prompts::persona(
            state
                .cfg
                .system_prompt
                .as_deref()
                .unwrap_or(prompts::DEFAULT_PERSONA),
            &state.cfg.person(),
        )
    } else {
        conversation.system_prompt.clone()
    };
    let system = prompts::system_prompt(&persona, &memories);

    let history = db(&state.store, move |s| s.list_messages(id)).await?;
    let mut turns = vec![ChatTurn {
        role: "system".into(),
        content: system,
    }];
    turns.extend(replay(&history, user_message_id, &state.cfg));

    let model_for_row = model.clone();
    let ids_for_row = memory_ids.clone();
    let placeholder = db(&state.store, move |s| {
        s.insert_message(id, "assistant", "", &model_for_row, &ids_for_row)
    })
    .await?;

    // The response outlives this function, so everything the stream touches is
    // moved into it: nothing below may be borrowed by the stream.
    let store_for_reply = state.store.clone();
    let store_for_memory = state.store.clone();
    let upstream_for_reply = state.upstream.clone();
    let upstream_for_memory = state.upstream.clone();
    let model_for_memory = model.clone();
    let memory_model_cfg = state.cfg.memory_model.clone();
    let default_model = state.cfg.default_model.clone();
    let memory_every = state.cfg.memory_every.max(1) as i64;
    let auto_memory = state.cfg.memory_enabled;
    let conversation_id = id;
    let turns_for_reply = turns;
    let used_ids = memory_ids.clone();
    let used_memories = memories.clone();

    let stream = async_stream::stream! {
        let mut reply = upstream_for_reply.stream(&model, &turns_for_reply).boxed();
        let mut buffered = String::new();
        let mut reasoning = String::new();

        yield sse("start", serde_json::json!({
            "messageId": placeholder,
            "model": model,
            "memories": used_memories,
        }));

        while let Some(item) = reply.next().await {
            match item {
                Ok(StreamEvent::Delta(text)) => {
                    buffered.push_str(&text);
                    yield sse("delta", serde_json::json!({ "text": text }));
                }
                Ok(StreamEvent::Thinking(text)) => {
                    reasoning.push_str(&text);
                    yield sse("thinking", serde_json::json!({ "text": text }));
                }
                Ok(StreamEvent::Error(message)) => {
                    yield sse("error", serde_json::json!({ "message": message }));
                    break;
                }
                Ok(StreamEvent::Done) => break,
                Err(e) => {
                    yield sse("error", serde_json::json!({ "message": e.to_string() }));
                    break;
                }
            }
        }

        // Store whatever arrived, even if the reader left early or the provider
        // broke off: a partial reply still beats losing the turn.
        let text = buffered;
        let ids = used_ids;
        let thinking = reasoning;
        let store = store_for_reply.clone();
        if let Err(e) = tokio::task::spawn_blocking(move || {
            store
                .update_message(placeholder, &text, &thinking, &ids)
                .and_then(|()| store.touch_conversation(conversation_id))
        })
        .await
        .unwrap_or(Ok(()))
        {
            eprintln!("could not store the reply: {e}");
        }
        yield sse("done", serde_json::json!({ "messageId": placeholder, "model": model }));

        // Mining runs after the reply, on the same stream, so the page can say
        // what was learned without polling for it.
        if auto_memory {
            // Due when enough user turns have landed since the last pass,
            // counting this one.
            let marker = {
                let store = store_for_memory.clone();
                tokio::task::spawn_blocking(move || store.last_memory_marker(conversation_id))
                    .await
                    .unwrap_or(Ok(0))
                    .unwrap_or(0)
            };
            let turns_since = {
                let store = store_for_memory.clone();
                tokio::task::spawn_blocking(move || {
                    store.count_user_messages_after(conversation_id, marker)
                })
                .await
                .unwrap_or(Ok(0))
                .unwrap_or(0)
            };

            if turns_since >= memory_every {
                let model = if model_for_memory.trim().is_empty() {
                    default_model
                } else {
                    model_for_memory
                };
                match extract_and_store(
                    &store_for_memory,
                    &upstream_for_memory,
                    conversation_id,
                    &memory_model_cfg,
                    &model,
                )
                .await
                {
                    Ok(added) if !added.is_empty() => {
                        yield sse("memory", serde_json::json!({ "added": added }));
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("memory extraction skipped: {e}"),
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

#[derive(Deserialize)]
struct RememberNow {
    #[serde(default)]
    message_id: Option<i64>,
}

/// Mines the conversation on demand. The button next to a reply uses this, so
/// the user can promote something without waiting for the automatic pass.
async fn remember_now(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<RememberNow>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    db(&state.store, move |s| s.get_conversation(id)).await?;
    let marker = body.and_then(|Json(b)| b.message_id).unwrap_or(0);
    let model = state.cfg.default_model.clone();
    let added = extract_and_store(
        &state.store,
        &state.upstream,
        id,
        &state.cfg.memory_model,
        &model,
    )
    .await
    .map_err(ApiError::from)?;
    if marker > 0 {
        let store = state.store.clone();
        let _ = tokio::task::spawn_blocking(move || store.set_memory_marker(id, marker)).await;
    }
    Ok(Json(serde_json::json!({ "added": added })))
}

/// Runs one extraction pass and stores whatever is new. Returns the notes that
/// were actually added, which is what the UI reports.
async fn extract_and_store(
    store: &Arc<Store>,
    upstream: &Arc<Upstream>,
    conversation_id: i64,
    memory_model: &Option<String>,
    fallback_model: &str,
) -> Result<Vec<Memory>, UpstreamError> {
    let model = memory_model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| fallback_model.to_string());

    let history = db(store, move |s| s.list_messages(conversation_id))
        .await
        .map_err(|_| UpstreamError::Protocol("could not read the conversation".into()))?;
    let turns: Vec<ChatTurn> = history
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| ChatTurn {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();
    let transcript = prompts::transcript_slice(&turns, 4000);
    if transcript.trim().is_empty() {
        return Ok(Vec::new());
    }

    let known = db(store, |s| s.list_memories())
        .await
        .map_err(|_| UpstreamError::Protocol("could not read memory".into()))?;
    let prompt = prompts::extraction_turns(&known, &transcript);
    let raw = upstream.complete(&model, &prompt).await?;
    let proposed = memory::parse_extraction(&raw);

    let mut added: Vec<Memory> = Vec::new();
    for item in proposed {
        let text = item.text.clone();
        let kind = item.kind.clone();
        if let Ok(memory) = db(store, move |s| s.insert_memory(&text, &kind, "auto")).await {
            // insert_memory returns the existing note for a duplicate; only
            // report ones that are genuinely new.
            if !known.iter().any(|k| k.id == memory.id) {
                added.push(memory);
            }
        }
    }

    let newest = history.iter().map(|m| m.id).max().unwrap_or(0);
    if newest > 0 {
        let _ = tokio::task::spawn_blocking({
            let store = store.clone();
            move || store.set_memory_marker(conversation_id, newest)
        })
        .await;
    }
    Ok(added)
}

/// The history handed to the model: the most recent turns, oldest first,
/// clipped to a character budget so long chats stay affordable.
fn replay(history: &[Message], up_to: i64, cfg: &Config) -> Vec<ChatTurn> {
    let mut turns: Vec<ChatTurn> = Vec::new();
    let mut used = 0usize;
    for message in history
        .iter()
        .filter(|m| m.id <= up_to && (m.role == "user" || m.role == "assistant"))
        .rev()
        .take(cfg.history_messages)
    {
        let cost = message.content.chars().count();
        if used + cost > cfg.history_budget && !turns.is_empty() {
            break;
        }
        used += cost;
        turns.push(ChatTurn {
            role: message.role.clone(),
            content: message.content.clone(),
        });
    }
    turns.reverse();
    turns
}

fn title_from(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").trim();
    let mut title: String = first_line.chars().take(60).collect();
    if first_line.chars().count() > 60 {
        title.push('…');
    }
    if title.is_empty() {
        "New chat".to_string()
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            addr: "127.0.0.1:0".into(),
            db_path: std::path::PathBuf::from(":memory:"),
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key: Some("test".into()),
            default_model: "test-model".into(),
            models: vec![],
            memory_enabled: true,
            memory_every: 2,
            memory_budget: 1200,
            memory_model: None,
            system_prompt: None,
            history_messages: 24,
            history_budget: 2400,
            user_name: "Drew".into(),
            timeout_secs: 30,
        }
    }

    fn message(id: i64, role: &str, content: &str) -> Message {
        Message {
            id,
            conversation_id: 1,
            role: role.into(),
            content: content.into(),
            reasoning: String::new(),
            model: String::new(),
            memory_ids: vec![],
            created_at: String::new(),
        }
    }

    #[test]
    fn replay_keeps_the_newest_turns_and_drops_nothing_in_range() {
        let history = vec![
            message(1, "user", "first"),
            message(2, "assistant", "reply one"),
            message(3, "user", "second"),
        ];
        let turns = replay(&history, 3, &config());
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].content, "first");
        assert_eq!(turns[2].content, "second");
    }

    #[test]
    fn replay_respects_the_budget_by_dropping_the_oldest() {
        let mut cfg = config();
        cfg.history_budget = 12;
        let history = vec![
            message(1, "user", "a very old message"),
            message(2, "assistant", "a very old reply"),
            message(3, "user", "new"),
        ];
        let turns = replay(&history, 3, &cfg);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].content, "new");
    }

    #[test]
    fn titles_come_from_the_first_question() {
        assert_eq!(
            title_from("How do I learn Rust?\nmore text"),
            "How do I learn Rust?"
        );
        assert_eq!(title_from("   "), "New chat");
        let long = title_from(&"word ".repeat(40));
        assert!(long.ends_with('…'));
        assert_eq!(long.chars().count(), 61);
    }
}
