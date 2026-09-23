//! End-to-end checks that drive the real router with tower::oneshot and point
//! the upstream client at a fake model server served from a plain OS thread.
//!
//! No client sockets are involved here, and the fake server lives on its own
//! thread, which keeps these tests hermetic and quick instead of racing the
//! network stack.

#![cfg(test)]

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{AppState, router};
use crate::config::Config;
use crate::db::Store;
use crate::upstream::Upstream;

// ---------------------------------------------------------------------------
// A fake model server on a plain thread: it understands exactly the two
// requests the app makes (GET /models, POST /chat/completions) and speaks
// minimal HTTP/1.1.

struct App {
    _dir: std::path::PathBuf,
    router: Router,
    stub_seen: Arc<Mutex<Vec<Value>>>,
}

fn spawn_stub(extraction_notes: &'static str) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming().take(200) {
            let Ok(mut stream) = stream else { continue };
            let (path, body) = read_request(&stream);
            let body_value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            if !body_value.is_null() {
                s.lock().unwrap().push(body_value.clone());
            }

            let response = match path.as_str() {
                "/v1/models" => {
                    http_ok_json(&json!({ "data": [{ "id": "stub-model" }] }).to_string())
                }
                "/v1/chat/completions" => {
                    if body_value
                        .get("stream")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        let mut out = String::new();
                        for chunk in ["Hello", " from", " the stub."] {
                            out.push_str(&format!(
                                "data: {}\n\n",
                                json!({ "choices": [{ "delta": { "content": chunk } }] })
                            ));
                        }
                        out.push_str("data: [DONE]\n\n");
                        sse_ok(&out)
                    } else {
                        http_ok_json(
                            &json!({ "choices": [{ "message": { "content": extraction_notes } }] })
                                .to_string(),
                        )
                    }
                }
                _ => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string(),
            };
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    (format!("http://{addr}/v1"), seen)
}

fn read_request(stream: &std::net::TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    let _ = reader.read_line(&mut request_line);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        headers.push_str(&line);
    }
    let len: usize = headers
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim().eq_ignore_ascii_case("content-length")).then(|| v.trim().parse().unwrap_or(0))
        })
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    let _ = reader.read_exact(&mut body);
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    (path, String::from_utf8_lossy(&body).to_string())
}

fn http_ok_json(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn sse_ok(body: &str) -> String {
    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n".to_string()
        + body
}

// ---------------------------------------------------------------------------

fn start_app(notes: &'static str, memory_every: usize) -> App {
    let (base_url, stub_seen) = spawn_stub(notes);
    let dir = std::env::temp_dir().join(format!(
        "self-ai-gui-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let cfg = Config {
        addr: "127.0.0.1:0".into(),
        db_path: dir.join("self.db"),
        base_url,
        api_key: Some("stub-key".into()),
        default_model: "stub-model".into(),
        models: vec![],
        memory_enabled: true,
        memory_every,
        memory_budget: 1200,
        memory_model: None,
        system_prompt: None,
        history_messages: 24,
        history_budget: 4000,
        user_name: "Drew".into(),
        timeout_secs: 10,
    };
    let state = Arc::new(AppState {
        store: Arc::new(Store::open(&cfg.db_path).unwrap()),
        upstream: Arc::new(Upstream::new(&cfg)),
        cfg,
    });
    App {
        router: router(state),
        _dir: dir,
        stub_seen,
    }
}

async fn call(app: &App, method: &str, path: &str, body: Option<Value>) -> (StatusCode, String) {
    let builder = Request::builder()
        .method(Method::from_bytes(method.as_bytes()).unwrap())
        .uri(path);
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn call_json(
    app: &App,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, text) = call(app, method, path, body).await;
    let parsed = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, parsed)
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_reply_is_streamed_stored_and_told_what_it_remembers() {
    let app = start_app("[]", 99);

    let (status, memory) = call_json(
        &app,
        "POST",
        "/api/memories",
        Some(json!({ "text": "Prefers tea over coffee.", "kind": "preference", "pinned": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let memory_id = memory["id"].as_i64().unwrap();

    let (_, conversation) = call_json(&app, "POST", "/api/conversations", Some(json!({}))).await;
    let id = conversation["id"].as_i64().unwrap();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/conversations/{id}/messages"),
        Some(json!({ "content": "What should I drink while I work?" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("event: start"), "{body}");
    assert!(body.contains("event: delta"), "{body}");
    assert!(body.contains("Hello"), "{body}");
    assert!(body.contains("event: done"), "{body}");

    // The prompt reached the fake server with the note injected as MEMORY.
    let sent = app.stub_seen.lock().unwrap().clone();
    let request = sent
        .iter()
        .find(|r| r["stream"] == true)
        .expect("the streaming call happened");
    let system = request["messages"][0]["content"].as_str().unwrap();
    assert_eq!(request["messages"][0]["role"], "system");
    assert!(
        system.contains("## MEMORY"),
        "memory block missing:\n{system}"
    );
    assert!(
        system.contains("Prefers tea over coffee."),
        "the note was not injected:\n{system}"
    );
    assert!(system.contains("Drew"), "the persona should name the user");
    assert_eq!(request["model"], "stub-model");

    let (_, messages) = call_json(
        &app,
        "GET",
        &format!("/api/conversations/{id}/messages"),
        None,
    )
    .await;
    let messages = messages.as_array().unwrap();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "Hello from the stub.");
    assert_eq!(messages[1]["memory_ids"], json!([memory_id]));

    let (_, conversation) = call_json(&app, "GET", &format!("/api/conversations/{id}"), None).await;
    assert_eq!(conversation["title"], "What should I drink while I work?");
    assert_eq!(conversation["message_count"], 2);
}

#[tokio::test]
async fn the_extraction_pass_saves_what_it_learns() {
    let notes = r#"[{"text": "Works on a Rust dashboard.", "kind": "project"}]"#;
    let app = start_app(notes, 1);

    let (_, conversation) = call_json(&app, "POST", "/api/conversations", Some(json!({}))).await;
    let id = conversation["id"].as_i64().unwrap();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/conversations/{id}/messages"),
        Some(json!({ "content": "I am building a Rust dashboard this month." })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("event: done"), "{body}");
    assert!(
        body.contains("event: memory"),
        "the page should be told what was learned:\n{body}"
    );
    assert!(body.contains("Works on a Rust dashboard."));

    let (_, memories) = call_json(&app, "GET", "/api/memories", None).await;
    let memories = memories.as_array().unwrap();
    assert_eq!(memories.len(), 1, "{memories:?}");
    assert_eq!(memories[0]["text"], "Works on a Rust dashboard.");
    assert_eq!(memories[0]["source"], "auto");

    // The extraction used a non-streaming call.
    let sent = app.stub_seen.lock().unwrap().clone();
    let extraction = sent
        .iter()
        .find(|r| r["stream"] == false)
        .expect("the extraction call happened");
    assert!(
        extraction["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("JSON")
    );

    // Running it again must not pile up a duplicate.
    let (_, again) = call_json(
        &app,
        "POST",
        &format!("/api/conversations/{id}/remember"),
        Some(json!({})),
    )
    .await;
    assert_eq!(
        again["added"].as_array().unwrap().len(),
        0,
        "duplicates must collapse"
    );
    let (_, memories) = call_json(&app, "GET", "/api/memories", None).await;
    assert_eq!(memories.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn memory_can_be_edited_pinned_and_muted() {
    let app = start_app("[]", 99);

    let (status, created) = call_json(
        &app,
        "POST",
        "/api/memories",
        Some(json!({ "text": "Lives in Tbilisi." })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["kind"], "fact");
    assert_eq!(created["enabled"], true);

    let (_, patched) = call_json(
        &app,
        "PATCH",
        &format!("/api/memories/{id}"),
        Some(json!({ "text": "Lives in Tbilisi, Georgia.", "pinned": true, "enabled": false })),
    )
    .await;
    assert_eq!(patched["text"], "Lives in Tbilisi, Georgia.");
    assert_eq!(patched["pinned"], true);
    assert_eq!(patched["enabled"], false);

    // A muted note is never injected, even though it is pinned.
    let (_, conversation) = call_json(&app, "POST", "/api/conversations", Some(json!({}))).await;
    let conversation_id = conversation["id"].as_i64().unwrap();
    let (_, body) = call(
        &app,
        "POST",
        &format!("/api/conversations/{conversation_id}/messages"),
        Some(json!({ "content": "Where do I live?" })),
    )
    .await;
    assert!(body.contains("event: start"), "{body}");
    let sent = app.stub_seen.lock().unwrap().clone();
    let system = sent.iter().find(|r| r["stream"] == true).unwrap()["messages"][0]["content"]
        .as_str()
        .unwrap();
    assert!(
        !system.contains("Tbilisi"),
        "a muted note leaked into the prompt:\n{system}"
    );

    let (status, _) = call(&app, "DELETE", &format!("/api/memories/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, memories) = call_json(&app, "GET", "/api/memories", None).await;
    assert!(memories.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn bad_input_is_refused_with_a_usable_message() {
    let app = start_app("[]", 99);

    let (status, body) = call(
        &app,
        "POST",
        "/api/conversations/1/messages",
        Some(json!({ "content": "hi" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (_, conversation) = call_json(&app, "POST", "/api/conversations", Some(json!({}))).await;
    let id = conversation["id"].as_i64().unwrap();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/conversations/{id}/messages"),
        Some(json!({ "content": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("say something"), "{body}");

    let (status, body) = call(&app, "POST", "/api/memories", Some(json!({ "text": "" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("some text"), "{body}");
}

#[tokio::test]
async fn the_api_reports_config_and_models() {
    let app = start_app("[]", 99);

    let (_, models) = call_json(&app, "GET", "/api/models", None).await;
    let ids: Vec<&str> = models
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"stub-model"), "{ids:?}");

    let (_, config) = call_json(&app, "GET", "/api/config", None).await;
    assert_eq!(config["defaultModel"], "stub-model");
    assert_eq!(config["hasKey"], true);
    assert_eq!(config["memoryEnabled"], true);
    assert!(
        config["baseUrl"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1")
    );
}
