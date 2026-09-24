//! Serde shapes shared by the API and the store.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub model: String,
    pub system_prompt: String,
    pub message_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub role: String, // user | assistant
    pub content: String,
    /// The model's reasoning trace for this turn, when it exposed one.
    pub reasoning: String,
    pub model: String,
    pub memory_ids: Vec<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub text: String,
    pub kind: String,   // fact | preference | project | person | routine
    pub source: String, // user | auto | seed
    pub pinned: bool,
    pub enabled: bool,
    pub created_at: String,
    pub last_used_at: String,
    pub use_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
}

/// One turn sent to the upstream chat-completions API.
#[derive(Debug, Clone, Serialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

/// Body shape the frontend sends to start a reply. `remember` asks that the
/// conversation be mined for durable notes afterwards.
#[derive(Debug, Deserialize)]
pub struct SendMessage {
    pub content: String,
    #[serde(default)]
    pub model: Option<String>,
}

/// A note that an extraction pass proposes, before deduplication.
#[derive(Debug, Clone, Deserialize)]
pub struct ProposedMemory {
    pub text: String,
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "fact".to_string()
}

pub const KINDS: [&str; 5] = ["fact", "preference", "project", "person", "routine"];

pub fn valid_kind(kind: &str) -> bool {
    KINDS.contains(&kind)
}
