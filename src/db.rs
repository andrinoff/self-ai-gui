//! SQLite persistence: conversations, messages and memories.
//!
//! rusqlite is synchronous, so callers run these functions on the blocking
//! pool via the `db()` helper in api.rs. A single connection behind a mutex
//! is plenty for a personal tool.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{Attachment, Conversation, Memory, Message};

pub struct Store {
    conn: Mutex<Connection>,
}

/// Errors the store can produce, mapped to HTTP statuses by the API layer.
#[derive(Debug)]
pub enum StoreError {
    NotFound,
    Sql(rusqlite::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "not found"),
            StoreError::Sql(e) => write!(f, "database error: {e}"),
            StoreError::Io(e) => write!(f, "storage error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                title                 TEXT    NOT NULL,
                model                 TEXT    NOT NULL DEFAULT '',
                system_prompt         TEXT    NOT NULL DEFAULT '',
                last_memory_message_id INTEGER NOT NULL DEFAULT 0,
                created_at            TEXT    NOT NULL,
                updated_at            TEXT    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                role            TEXT    NOT NULL,
                content         TEXT    NOT NULL,
                model           TEXT    NOT NULL DEFAULT '',
                memory_ids      TEXT    NOT NULL DEFAULT '',
                created_at      TEXT    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memories (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                text         TEXT    NOT NULL,
                kind         TEXT    NOT NULL DEFAULT 'fact',
                source       TEXT    NOT NULL DEFAULT 'user',
                pinned       INTEGER NOT NULL DEFAULT 0,
                enabled      INTEGER NOT NULL DEFAULT 1,
                normalized   TEXT    NOT NULL UNIQUE,
                created_at   TEXT    NOT NULL,
                last_used_at TEXT    NOT NULL DEFAULT '',
                use_count    INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id, id);
            CREATE INDEX IF NOT EXISTS idx_memories_enabled ON memories(enabled, pinned);
            "#,
        )?;
        // Columns added after the first release. Each is best effort: on a
        // fresh database the CREATE above already included it and the ALTER
        // fails harmlessly, which is why the result is ignored.
        let _ = conn.execute(
            "ALTER TABLE messages ADD COLUMN reasoning TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE messages ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]'",
            [],
        );
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    // ---------- conversations ----------

    pub fn list_conversations(&self) -> Result<Vec<Conversation>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.title, c.model, c.system_prompt, c.created_at, c.updated_at,
                    (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id)
             FROM conversations c
             ORDER BY c.updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                model: row.get(2)?,
                system_prompt: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                message_count: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_conversation(&self, id: i64) -> Result<Conversation, StoreError> {
        let conn = self.conn.lock().unwrap();
        read_conversation(&conn, id)
    }
}

fn read_conversation(conn: &Connection, id: i64) -> Result<Conversation, StoreError> {
    conn.query_row(
        "SELECT c.id, c.title, c.model, c.system_prompt, c.created_at, c.updated_at,
                    (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id)
             FROM conversations c WHERE c.id = ?",
        [id],
        |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                model: row.get(2)?,
                system_prompt: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                message_count: row.get(6)?,
            })
        },
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

impl Store {
    pub fn create_conversation(
        &self,
        model: &str,
        system_prompt: &str,
    ) -> Result<Conversation, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO conversations (title, model, system_prompt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params!["", model, system_prompt, now()],
        )?;
        let id = conn.last_insert_rowid();
        read_conversation(&conn, id)
    }

    pub fn update_conversation(
        &self,
        id: i64,
        title: Option<&str>,
        model: Option<&str>,
        system_prompt: Option<&str>,
    ) -> Result<Conversation, StoreError> {
        let conn = self.conn.lock().unwrap();
        if let Some(title) = title {
            conn.execute(
                "UPDATE conversations SET title = ? WHERE id = ?",
                params![title, id],
            )?;
        }
        if let Some(model) = model {
            conn.execute(
                "UPDATE conversations SET model = ? WHERE id = ?",
                params![model, id],
            )?;
        }
        if let Some(system_prompt) = system_prompt {
            conn.execute(
                "UPDATE conversations SET system_prompt = ? WHERE id = ?",
                params![system_prompt, id],
            )?;
        }
        read_conversation(&conn, id)
    }

    pub fn touch_conversation(&self, id: i64) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET updated_at = ? WHERE id = ?",
            params![now(), id],
        )?;
        Ok(())
    }

    pub fn delete_conversation(&self, id: i64) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM conversations WHERE id = ?", [id])?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    // ---------- messages ----------

    pub fn list_messages(&self, conversation_id: i64) -> Result<Vec<Message>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, reasoning, model, memory_ids,
                    attachments, created_at
             FROM messages WHERE conversation_id = ? ORDER BY id",
        )?;
        let rows = stmt.query_map([conversation_id], |row| {
            let ids: String = row.get(6)?;
            let attachments: String = row.get(7)?;
            Ok(Message {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                reasoning: row.get(4)?,
                model: row.get(5)?,
                memory_ids: parse_ids(&ids),
                attachments: parse_attachments(&attachments),
                created_at: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Inserts a message and returns its row id.
    pub fn insert_message(
        &self,
        conversation_id: i64,
        role: &str,
        content: &str,
        model: &str,
        memory_ids: &[i64],
        attachments: &[Attachment],
    ) -> Result<i64, StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages
                (conversation_id, role, content, model, memory_ids, attachments, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                conversation_id,
                role,
                content,
                model,
                encode_ids(memory_ids),
                encode_attachments(attachments),
                now()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_message(
        &self,
        id: i64,
        content: &str,
        reasoning: &str,
        memory_ids: &[i64],
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET content = ?, reasoning = ?, memory_ids = ? WHERE id = ?",
            params![content, reasoning, encode_ids(memory_ids), id],
        )?;
        Ok(())
    }

    /// The last message id that memory extraction already looked past.
    pub fn last_memory_marker(&self, conversation_id: i64) -> Result<i64, StoreError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT last_memory_message_id FROM conversations WHERE id = ?",
                [conversation_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub fn set_memory_marker(
        &self,
        conversation_id: i64,
        message_id: i64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET last_memory_message_id = ? WHERE id = ?",
            params![message_id, conversation_id],
        )?;
        Ok(())
    }

    pub fn count_user_messages_after(
        &self,
        conversation_id: i64,
        after_id: i64,
    ) -> Result<i64, StoreError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ? AND id > ? AND role = 'user'",
            params![conversation_id, after_id],
            |row| row.get(0),
        )?)
    }

    // ---------- memories ----------

    pub fn list_memories(&self) -> Result<Vec<Memory>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, text, kind, source, pinned, enabled, created_at, last_used_at, use_count
             FROM memories ORDER BY pinned DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| Ok(row_to_memory(row)?))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_memory(&self, id: i64) -> Result<Memory, StoreError> {
        let conn = self.conn.lock().unwrap();
        read_memory(&conn, id)
    }

    pub fn insert_memory(
        &self,
        text: &str,
        kind: &str,
        source: &str,
    ) -> Result<Memory, StoreError> {
        let normalized = crate::memory::normalize(text);
        let conn = self.conn.lock().unwrap();
        // Prefer the existing note when one already says the same thing, so
        // re-remembering a fact is a no-op rather than a pile of duplicates.
        if let Some(existing) = read_memory_by_normalized(&conn, &normalized)? {
            return Ok(existing);
        }
        conn.execute(
            "INSERT INTO memories (text, kind, source, normalized, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![text.trim(), kind, source, normalized, now()],
        )?;
        let id = conn.last_insert_rowid();
        read_memory(&conn, id)
    }

    pub fn update_memory(
        &self,
        id: i64,
        text: Option<&str>,
        kind: Option<&str>,
        pinned: Option<bool>,
        enabled: Option<bool>,
    ) -> Result<Memory, StoreError> {
        let conn = self.conn.lock().unwrap();
        let existing = read_memory(&conn, id)?;
        let text = text.unwrap_or(&existing.text);
        let kind = kind.unwrap_or(&existing.kind);
        let pinned = pinned.unwrap_or(existing.pinned);
        let enabled = enabled.unwrap_or(existing.enabled);
        let normalized = crate::memory::normalize(text);
        conn.execute(
            "UPDATE memories SET text = ?1, kind = ?2, pinned = ?3, enabled = ?4, normalized = ?5 WHERE id = ?6",
            params![text.trim(), kind, pinned as i64, enabled as i64, normalized, id],
        )?;
        read_memory(&conn, id)
    }

    pub fn delete_memory(&self, id: i64) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM memories WHERE id = ?", [id])?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub fn mark_memories_used(&self, ids: &[i64]) -> Result<(), StoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let placeholders: Vec<String> = (0..ids.len()).map(|_| "?".to_string()).collect();
        let sql = format!(
            "UPDATE memories SET use_count = use_count + 1, last_used_at = ?
             WHERE id IN ({})",
            placeholders.join(",")
        );
        let stamp = now();
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&stamp];
        for id in ids {
            params.push(id);
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
        Ok(())
    }
}

fn read_memory_by_normalized(
    conn: &Connection,
    normalized: &str,
) -> Result<Option<Memory>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT id, text, kind, source, pinned, enabled, created_at, last_used_at, use_count
             FROM memories WHERE normalized = ?",
            [normalized],
            |row| row_to_memory(row),
        )
        .optional()?)
}

fn read_memory(conn: &Connection, id: i64) -> Result<Memory, StoreError> {
    conn.query_row(
        "SELECT id, text, kind, source, pinned, enabled, created_at, last_used_at, use_count
         FROM memories WHERE id = ?",
        [id],
        |row| row_to_memory(row),
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: row.get(0)?,
        text: row.get(1)?,
        kind: row.get(2)?,
        source: row.get(3)?,
        pinned: row.get::<_, i64>(4)? != 0,
        enabled: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        last_used_at: row.get(7)?,
        use_count: row.get(8)?,
    })
}

fn encode_ids(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

fn parse_ids(raw: &str) -> Vec<i64> {
    raw.split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect()
}

fn encode_attachments(attachments: &[Attachment]) -> String {
    serde_json::to_string(attachments).unwrap_or_else(|_| "[]".to_string())
}

/// A malformed row reads as "no attachments" rather than failing the whole
/// transcript: the text of the message is still worth showing.
fn parse_attachments(raw: &str) -> Vec<Attachment> {
    serde_json::from_str(raw).unwrap_or_default()
}
