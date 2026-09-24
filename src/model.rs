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
    pub attachments: Vec<Attachment>,
    pub memory_ids: Vec<i64>,
    pub created_at: String,
}

/// An image attached to a message. `data` is the raw base64, without the
/// `data:` prefix, so the wire shape is small and the UI can decide how to
/// display it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub mime: String,
    pub data: String,
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
    /// Whether the model can be sent images. Drives the attach control: a
    /// model that cannot see never offers to take a picture.
    pub vision: bool,
}

/// One turn sent to the upstream chat-completions API.
///
/// Plain text serializes to the ordinary `{"role","content":"…"}` shape; a
/// turn carrying images switches `content` to the multimodal parts array, so
/// nothing else has to know about the difference.
#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
    /// Data URLs (`data:image/png;base64,…`) to attach to this turn.
    pub images: Vec<String>,
}

impl ChatTurn {
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        ChatTurn {
            role: role.to_string(),
            content: content.into(),
            images: Vec::new(),
        }
    }
}

impl Serialize for ChatTurn {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut turn = serializer.serialize_struct("ChatTurn", 2)?;
        turn.serialize_field("role", &self.role)?;
        if self.images.is_empty() {
            turn.serialize_field("content", &self.content)?;
        } else {
            let mut parts = Vec::with_capacity(self.images.len() + 1);
            parts.push(serde_json::json!({ "type": "text", "text": self.content }));
            for url in &self.images {
                parts.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": url },
                }));
            }
            turn.serialize_field("content", &parts)?;
        }
        turn.end()
    }
}

/// Body shape the frontend sends to start a reply.
#[derive(Debug, Deserialize)]
pub struct SendMessage {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_turn_serializes_as_a_plain_string() {
        let turn = ChatTurn::text("system", "hello");
        let json = serde_json::to_value(&turn).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "role": "system", "content": "hello" })
        );
    }

    #[test]
    fn a_turn_with_images_serializes_as_parts() {
        let mut turn = ChatTurn::text("user", "what is this?");
        turn.images = vec!["data:image/png;base64,AAAA".into()];
        let json = serde_json::to_value(&turn).unwrap();
        assert_eq!(json["content"][0]["type"], "text");
        assert_eq!(json["content"][0]["text"], "what is this?");
        assert_eq!(json["content"][1]["type"], "image_url");
        assert_eq!(
            json["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
    }
}
