//! SQLite persistence for Hearth. Storage lane (mimo).
//!
//! Frozen lifecycle and uncertainty invariants — callers may rely on these:
//!
//! * Incognito conversations are never persisted. `save_conversation` performs no
//!   writes for them and reports success; `record_audit` is the caller's duty to
//!   skip (lib.rs does) and this layer keeps no hidden copies.
//! * Messages persist their exact `status`/`thinking`/`tools`/`stats` payloads and
//!   their order. Rows still marked `streaming` after a crash are recovered to
//!   `stopped` when the database is opened, so no partial answer is ever lost.
//! * Audit rows are purged together with their conversation (`delete_conversation`):
//!   tool arguments and output can contain private transcript data, so deletion must
//!   not leave hidden copies. Outside deletion, retention is bounded to the latest
//!   1000 rows.
//! * Corrupt payloads surface as errors that name the offending row. The database
//!   is never discarded or rewritten behind the user's back.
//! * Search covers message `content` and conversation titles only. `thinking` text
//!   is never indexed. A blank query returns every conversation id. Genuine
//!   SQLite failures propagate; only an FTS syntax error degrades to no matches.

use crate::models::{Conversation, Memory, Message, Settings, Stats, ToolStep};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 1;
const MAX_MESSAGES: usize = 5_000;
const MAX_TEXT_BYTES: usize = 1_000_000;
const MAX_JSON_BYTES: usize = 1_000_000;
const MAX_TITLE_BYTES: usize = 1_000;
const MAX_ID_BYTES: usize = 128;
const MAX_SHORT_BYTES: usize = 500;
const MAX_MEMORIES_USED: usize = 100;
const AUDIT_KEEP: usize = 1_000;
const SEARCH_MAX_CHARS: usize = 200;
const SEARCH_TERM_MAX_CHARS: usize = 100;

const MESSAGE_STATUSES: [&str; 4] = ["complete", "streaming", "stopped", "error"];
const TOOL_STATUSES: [&str; 5] = ["pending", "running", "complete", "denied", "error"];
const MEMORY_CATEGORIES: [&str; 5] = ["identity", "preference", "project", "instruction", "other"];
const MEMORY_STATUSES: [&str; 3] = ["active", "candidate", "archived"];

const SCHEMA_SQL: &str = r#"
CREATE TABLE conversations (
  id            TEXT PRIMARY KEY,
  title         TEXT NOT NULL,
  mode          TEXT NOT NULL CHECK (mode IN ('chat','agent')),
  model_id      TEXT NOT NULL,
  backend_id    TEXT NOT NULL,
  pinned        INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0,1)),
  incognito     INTEGER NOT NULL DEFAULT 0 CHECK (incognito = 0),
  memory_enabled INTEGER NOT NULL DEFAULT 1 CHECK (memory_enabled IN (0,1)),
  thinking      INTEGER NOT NULL DEFAULT 0 CHECK (thinking IN (0,1)),
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE TABLE messages (
  seq                INTEGER PRIMARY KEY,
  id                 TEXT NOT NULL UNIQUE,
  conversation_id    TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  position           INTEGER NOT NULL,
  role               TEXT NOT NULL CHECK (role IN ('user','assistant')),
  content            TEXT NOT NULL,
  thinking           TEXT NOT NULL DEFAULT '',
  status             TEXT NOT NULL CHECK (status IN ('complete','streaming','stopped','error')),
  stats_json         TEXT,
  tools_json         TEXT NOT NULL DEFAULT '[]',
  memories_used_json TEXT NOT NULL DEFAULT '[]',
  created_at         INTEGER NOT NULL,
  UNIQUE (conversation_id, position)
);
CREATE INDEX messages_conversation ON messages(conversation_id);
CREATE VIRTUAL TABLE messages_fts USING fts5(
  content, content='messages', content_rowid='seq', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, content) VALUES (new.seq, new.content);
END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.seq, old.content);
END;
CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.seq, old.content);
  INSERT INTO messages_fts(rowid, content) VALUES (new.seq, new.content);
END;
CREATE TABLE memories (
  seq        INTEGER PRIMARY KEY,
  id         TEXT NOT NULL UNIQUE,
  text       TEXT NOT NULL,
  category   TEXT NOT NULL CHECK (category IN ('identity','preference','project','instruction','other')),
  status     TEXT NOT NULL CHECK (status IN ('active','candidate','archived')),
  pinned     INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0,1)),
  enabled    INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE VIRTUAL TABLE memories_fts USING fts5(
  text, content='memories', content_rowid='seq', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts(rowid, text) VALUES (new.seq, new.text);
END;
CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.seq, old.text);
END;
CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.seq, old.text);
  INSERT INTO memories_fts(rowid, text) VALUES (new.seq, new.text);
END;
CREATE TABLE audit (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id TEXT NOT NULL,
  tool_step_json  TEXT NOT NULL,
  ts              INTEGER NOT NULL
);
CREATE INDEX audit_conversation ON audit(conversation_id);
CREATE TABLE settings (
  id   INTEGER PRIMARY KEY CHECK (id = 1),
  json TEXT NOT NULL
);
"#;

pub struct Store {
    conn: Mutex<Connection>,
}

struct RawMessage {
    id: String,
    role: String,
    content: String,
    thinking: String,
    status: String,
    stats_json: Option<String>,
    tools_json: String,
    memories_used_json: String,
    created_at: i64,
}

impl RawMessage {
    fn into_message(self) -> Result<Message, String> {
        let stats: Option<Stats> = match self.stats_json {
            None => None,
            Some(json) => Some(serde_json::from_str(&json).map_err(|e| {
                format!("Message {} has a corrupt stats payload: {e}", self.id)
            })?),
        };
        let tools: Vec<ToolStep> = serde_json::from_str(&self.tools_json).map_err(|e| {
            format!("Message {} has a corrupt tool payload: {e}", self.id)
        })?;
        let memories_used: Vec<String> = serde_json::from_str(&self.memories_used_json)
            .map_err(|e| format!("Message {} has a corrupt memory payload: {e}", self.id))?;
        Ok(Message {
            id: self.id,
            role: self.role,
            content: self.content,
            thinking: self.thinking,
            created_at: self.created_at,
            status: self.status,
            stats,
            tools,
            memories_used,
        })
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn check_id(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("Identifier must not be empty.".into());
    }
    if value.len() > MAX_ID_BYTES {
        return Err(format!("Identifier exceeds {MAX_ID_BYTES} bytes."));
    }
    if value.contains('\0') {
        return Err("Identifier must not contain NUL.".into());
    }
    Ok(())
}

fn check_text(value: &str, max: usize, what: &str) -> Result<(), String> {
    if value.len() > max {
        return Err(format!("{what} exceeds {max} bytes."));
    }
    if value.contains('\0') {
        return Err(format!("{what} must not contain NUL."));
    }
    Ok(())
}

fn check_enum(value: &str, allowed: &[&str], what: &str) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{what} must be one of: {}.",
            allowed.join(", ")
        ))
    }
}

fn check_ts(value: i64, what: &str) -> Result<(), String> {
    if value < 0 {
        Err(format!("{what} must not be negative."))
    } else {
        Ok(())
    }
}

fn json_field<T: serde::Serialize>(value: &T, what: &str) -> Result<String, String> {
    let json = serde_json::to_string(value).map_err(|e| format!("{what} could not be encoded: {e}"))?;
    if json.len() > MAX_JSON_BYTES {
        return Err(format!("{what} payload exceeds {MAX_JSON_BYTES} bytes."));
    }
    Ok(json)
}

fn validate_message(message: &Message) -> Result<(), String> {
    check_id(&message.id)?;
    check_enum(&message.role, &["user", "assistant"], "Message role")?;
    check_text(&message.content, MAX_TEXT_BYTES, "Message content")?;
    check_text(&message.thinking, MAX_TEXT_BYTES, "Message thinking")?;
    check_enum(&message.status, &MESSAGE_STATUSES, "Message status")?;
    check_ts(message.created_at, "Message timestamp")?;
    if message.memories_used.len() > MAX_MEMORIES_USED {
        return Err(format!("Message references more than {MAX_MEMORIES_USED} memories."));
    }
    for used in &message.memories_used {
        check_id(used)?;
    }
    for tool in &message.tools {
        check_id(&tool.id)?;
        if tool.name.is_empty() || tool.name.len() > MAX_ID_BYTES {
            return Err("Tool name must be between 1 and 128 bytes.".into());
        }
        check_enum(&tool.status, &TOOL_STATUSES, "Tool status")?;
        check_text(&tool.output, MAX_TEXT_BYTES, "Tool output")?;
        check_text(&tool.preview, MAX_TEXT_BYTES, "Tool preview")?;
        json_field(&tool.args, "Tool arguments")?;
    }
    if let Some(stats) = &message.stats {
        // NaN serialises as JSON null and then fails the next read. Reject it here.
        if !stats.tokens_per_second.is_finite() || stats.tokens_per_second < 0.0 {
            return Err(
                "Message stats tokens_per_second must be finite and nonnegative.".into(),
            );
        }
        json_field(stats, "Message stats")?;
    }
    Ok(())
}

fn validate_conversation(conversation: &Conversation) -> Result<(), String> {
    check_id(&conversation.id)?;
    check_text(&conversation.title, MAX_TITLE_BYTES, "Conversation title")?;
    check_enum(&conversation.mode, &["chat", "agent"], "Conversation mode")?;
    check_text(&conversation.model_id, MAX_SHORT_BYTES, "Model id")?;
    check_text(&conversation.backend_id, MAX_SHORT_BYTES, "Backend id")?;
    check_ts(conversation.created_at, "Conversation timestamp")?;
    check_ts(conversation.updated_at, "Conversation timestamp")?;
    if conversation.messages.len() > MAX_MESSAGES {
        return Err(format!("Conversation exceeds {MAX_MESSAGES} messages."));
    }
    for message in &conversation.messages {
        validate_message(message)?;
    }
    Ok(())
}

fn validate_memory(memory: &Memory) -> Result<(), String> {
    check_id(&memory.id)?;
    if memory.text.trim().is_empty() {
        return Err("Memory text must not be empty.".into());
    }
    check_text(&memory.text, MAX_TEXT_BYTES, "Memory text")?;
    check_enum(&memory.category, &MEMORY_CATEGORIES, "Memory category")?;
    check_enum(&memory.status, &MEMORY_STATUSES, "Memory status")?;
    check_ts(memory.created_at, "Memory timestamp")?;
    check_ts(memory.updated_at, "Memory timestamp")?;
    Ok(())
}

fn validate_settings(settings: &Settings) -> Result<(), String> {
    check_enum(&settings.theme, &["system", "light", "dark"], "Theme")?;
    let accent = settings.accent.as_bytes();
    let hex_ok = accent.len() == 7
        && accent[0] == b'#'
        && accent[1..].iter().all(|b| b.is_ascii_hexdigit());
    if !hex_ok {
        return Err("Accent must be a #rrggbb hex colour.".into());
    }
    if !(512..=131_072).contains(&settings.context_window) {
        return Err("Context window must be between 512 and 131072.".into());
    }
    if settings.max_output_tokens < 16 || settings.max_output_tokens >= settings.context_window {
        return Err("Output reservation must be at least 16 and below the context window.".into());
    }
    if !(0.0..=2.0).contains(&settings.temperature) || !settings.temperature.is_finite() {
        return Err("Temperature must be between 0 and 2.".into());
    }
    check_text(&settings.system_prompt, 50_000, "System prompt")?;
    if !(1..=20).contains(&settings.max_steps) {
        return Err("Max steps must be between 1 and 20.".into());
    }
    check_text(&settings.workspace, 1_000, "Workspace path")?;
    check_enum(&settings.send_key, &["enter", "cmd_enter"], "Send key")?;
    check_id(&settings.default_backend_id)?;
    check_text(&settings.default_model_id, MAX_SHORT_BYTES, "Default model id")?;
    if settings.backends.len() > 8 {
        return Err("At most eight backends are supported.".into());
    }
    let mut seen: Vec<&str> = Vec::new();
    for backend in &settings.backends {
        check_id(&backend.id)?;
        if seen.contains(&backend.id.as_str()) {
            return Err(format!("Duplicate backend id: {}.", backend.id));
        }
        seen.push(&backend.id);
        if backend.name.is_empty() || backend.name.len() > 100 {
            return Err("Backend name must be between 1 and 100 bytes.".into());
        }
        if backend.kind.is_empty() || backend.kind.len() > 32 {
            return Err("Backend kind must be between 1 and 32 bytes.".into());
        }
        if backend.url.is_empty() {
            return Err("Backend URL must not be empty.".into());
        }
        check_text(&backend.url, 2_000, "Backend URL")?;
    }
    Ok(())
}

/// An FTS parse failure is a no-match. A missing table, a locked database, or a
/// bad row is a real error and must not look like an empty search.
fn is_malformed_fts(error: &rusqlite::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("fts5:") || message.contains("syntax error")
}

/// Quote each whitespace-separated term so malformed FTS syntax is impossible
/// and the query degrades to plain word matching.
fn fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .map(|term| {
            let mut term = term.chars().take(SEARCH_TERM_MAX_CHARS).collect::<String>();
            term = term.replace('"', "\"\"");
            format!("\"{term}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn like_pattern(raw: &str) -> String {
    let escaped = raw
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

impl Store {
    fn guard(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.conn
            .lock()
            .map_err(|_| "The database lock was poisoned.".to_string())
    }

    /// Open (creating if needed) the Hearth database, run migrations and recover
    /// any incomplete streaming rows. FTS5 is required; failure is a startup error.
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create the data folder: {e}"))?;
            }
        }
        let mut conn =
            Connection::open(path).map_err(|e| format!("Could not open the Hearth database: {e}"))?;
        conn.busy_timeout(Duration::from_millis(5_000))
            .map_err(|e| format!("Could not configure the database: {e}"))?;
        // WAL keeps the UI from blocking on writes; if a filesystem refuses it the
        // journal pragma still returns a usable mode, so only SQL failures are fatal.
        conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Could not enable WAL journal mode: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| format!("Could not enable foreign keys: {e}"))?;
        migrate(&mut conn)?;
        conn.execute(
            "UPDATE messages SET status = 'stopped' WHERE status = 'streaming'",
            [],
        )
        .map_err(|e| format!("Could not recover interrupted messages: {e}"))?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    /// All conversations, newest activity first, with messages in saved order.
    pub fn conversations(&self) -> Result<Vec<Conversation>, String> {
        let conn = self.guard()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, title, mode, model_id, backend_id, pinned, memory_enabled, thinking, \
                 created_at, updated_at FROM conversations ORDER BY updated_at DESC, created_at DESC, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut conversations = Vec::new();
        for row in rows {
            let (id, title, mode, model_id, backend_id, pinned, memory_enabled, thinking, created_at, updated_at) =
                row.map_err(|e| e.to_string())?;
            let messages = messages_for(&conn, &id)?;
            conversations.push(Conversation {
                id,
                title,
                mode,
                model_id,
                backend_id,
                pinned: pinned != 0,
                incognito: false,
                memory_enabled: memory_enabled != 0,
                thinking: thinking != 0,
                created_at,
                updated_at,
                messages,
            });
        }
        Ok(conversations)
    }

    /// Every memory row, including candidates — the review inbox needs them.
    /// Prompt injection is filtered in `context::build`, not here.
    pub fn memories(&self) -> Result<Vec<Memory>, String> {
        let conn = self.guard()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, text, category, status, pinned, enabled, created_at, updated_at \
                 FROM memories ORDER BY pinned DESC, updated_at DESC, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Memory {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    category: row.get(2)?,
                    status: row.get(3)?,
                    pinned: row.get::<_, i64>(4)? != 0,
                    enabled: row.get::<_, i64>(5)? != 0,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut memories = Vec::new();
        for memory in rows {
            memories.push(memory.map_err(|e| e.to_string())?);
        }
        Ok(memories)
    }

    /// Stored settings. A corrupt row errors by name and leaves the database intact.
    pub fn settings(&self) -> Result<Settings, String> {
        let conn = self.guard()?;
        let stored: Option<String> = conn
            .query_row("SELECT json FROM settings WHERE id = 1", [], |row| row.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(json) = stored else {
            return Ok(Settings::default());
        };
        let mut settings: Settings = serde_json::from_str(&json).map_err(|e| {
            format!("Stored settings are corrupt ({e}). Save settings again to replace them.")
        })?;
        // Read-time normalisation only. The stored row is left untouched so a
        // later valid save is what repairs it.
        settings.network_tools = false;
        settings.auto_extract = false;
        validate_settings(&settings).map_err(|e| {
            format!("Stored settings are invalid ({e}). Save settings again to replace them.")
        })?;
        Ok(settings)
    }

    /// Atomic whole-conversation save: upsert metadata, replace messages in order.
    /// Incognito conversations are refused silently — nothing is written.
    pub fn save_conversation(&self, conversation: &Conversation) -> Result<(), String> {
        if conversation.incognito {
            return Ok(());
        }
        validate_conversation(conversation)?;
        let mut guard = self.guard()?;
        let tx = guard
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO conversations \
             (id, title, mode, model_id, backend_id, pinned, incognito, memory_enabled, thinking, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?10) \
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, mode = excluded.mode, \
             model_id = excluded.model_id, backend_id = excluded.backend_id, pinned = excluded.pinned, \
             memory_enabled = excluded.memory_enabled, thinking = excluded.thinking, updated_at = excluded.updated_at",
            params![
                conversation.id,
                conversation.title,
                conversation.mode,
                conversation.model_id,
                conversation.backend_id,
                conversation.pinned as i64,
                conversation.memory_enabled as i64,
                conversation.thinking as i64,
                conversation.created_at,
                conversation.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            params![conversation.id],
        )
        .map_err(|e| e.to_string())?;
        for (position, message) in conversation.messages.iter().enumerate() {
            validate_message(message)?;
            let stats_json = match &message.stats {
                Some(stats) => Some(json_field(stats, "Message stats")?),
                None => None,
            };
            let tools_json = json_field(&message.tools, "Tool steps")?;
            let memories_used_json = json_field(&message.memories_used, "Memories used")?;
            tx.execute(
                "INSERT INTO messages \
                 (id, conversation_id, position, role, content, thinking, status, stats_json, tools_json, memories_used_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    message.id,
                    conversation.id,
                    position as i64,
                    message.role,
                    message.content,
                    message.thinking,
                    message.status,
                    stats_json,
                    tools_json,
                    memories_used_json,
                    message.created_at
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// Delete a conversation, its messages and its audit rows. Idempotent.
    /// Audit is purged explicitly (not left to FK cascade) because tool arguments
    /// and output can contain private transcript data — deletion leaves no hidden copy.
    pub fn delete_conversation(&self, conversation_id: &str) -> Result<(), String> {
        check_id(conversation_id)?;
        let mut guard = self.guard()?;
        let tx = guard
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM conversations WHERE id = ?1",
            params![conversation_id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM audit WHERE conversation_id = ?1",
            params![conversation_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn save_memory(&self, memory: &Memory) -> Result<(), String> {
        validate_memory(memory)?;
        let conn = self.guard()?;
        conn.execute(
            "INSERT INTO memories (id, text, category, status, pinned, enabled, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(id) DO UPDATE SET text = excluded.text, category = excluded.category, \
             status = excluded.status, pinned = excluded.pinned, enabled = excluded.enabled, \
             updated_at = excluded.updated_at",
            params![
                memory.id,
                memory.text,
                memory.category,
                memory.status,
                memory.pinned as i64,
                memory.enabled as i64,
                memory.created_at,
                memory.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_memory(&self, memory_id: &str) -> Result<(), String> {
        check_id(memory_id)?;
        let conn = self.guard()?;
        conn.execute("DELETE FROM memories WHERE id = ?1", params![memory_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Persist settings after validation. Unsupported features (`network_tools`,
    /// `auto_extract`) are normalised to false before storage, never trusted.
    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        let mut settings = settings.clone();
        settings.network_tools = false;
        settings.auto_extract = false;
        validate_settings(&settings)?;
        let json = json_field(&settings, "Settings")?;
        let conn = self.guard()?;
        conn.execute(
            "INSERT INTO settings (id, json) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET json = excluded.json",
            params![json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Conversation ids whose message content matches the query (FTS words) or
    /// whose title contains it (case-insensitive), newest activity first.
    /// A blank query returns every id in that order. An FTS syntax error
    /// degrades to no matches; any other SQLite or row failure propagates.
    pub fn search_conversations(&self, query: &str) -> Result<Vec<String>, String> {
        let raw: String = query.trim().chars().take(SEARCH_MAX_CHARS).collect();
        if raw.is_empty() {
            return self.all_conversation_ids();
        }
        let fts = fts_query(&raw);
        if fts.is_empty() {
            return self.all_conversation_ids();
        }
        let conn = self.guard()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT c.id FROM conversations c WHERE c.id IN ( \
                   SELECT m.conversation_id FROM messages_fts \
                   JOIN messages m ON m.seq = messages_fts.rowid \
                   WHERE messages_fts MATCH ?1 \
                   UNION \
                   SELECT id FROM conversations WHERE title LIKE ?2 ESCAPE '\\' \
                 ) ORDER BY c.updated_at DESC, c.created_at DESC, c.id",
            )
            .map_err(|e| e.to_string())?;
        let rows = match stmt
            .query_map(params![fts, like_pattern(&raw)], |row| row.get::<_, String>(0))
        {
            Ok(rows) => rows,
            Err(error) if is_malformed_fts(&error) => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        let mut ids = Vec::new();
        for id in rows {
            match id {
                Ok(id) => ids.push(id),
                Err(error) if is_malformed_fts(&error) => return Ok(Vec::new()),
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(ids)
    }

    fn all_conversation_ids(&self) -> Result<Vec<String>, String> {
        let conn = self.guard()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id FROM conversations ORDER BY updated_at DESC, created_at DESC, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        for id in rows {
            ids.push(id.map_err(|e| e.to_string())?);
        }
        Ok(ids)
    }

    /// Record one tool step in the audit log, pruning to the latest 1000 rows.
    /// Callers must skip incognito conversations (lib.rs does).
    pub fn record_audit(&self, conversation_id: &str, step: &ToolStep) -> Result<(), String> {
        check_id(conversation_id)?;
        let json = json_field(step, "Tool step")?;
        let mut guard = self.guard()?;
        let tx = guard
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO audit (conversation_id, tool_step_json, ts) VALUES (?1, ?2, ?3)",
            params![conversation_id, json, now_ms()],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM audit WHERE id NOT IN (SELECT id FROM audit ORDER BY id DESC LIMIT ?1)",
            params![AUDIT_KEEP as i64],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Audit trail for a conversation, oldest first. Purged with the conversation.
    pub fn audit(&self, conversation_id: &str) -> Result<Vec<ToolStep>, String> {
        check_id(conversation_id)?;
        let conn = self.guard()?;
        let mut stmt = conn
            .prepare_cached("SELECT tool_step_json FROM audit WHERE conversation_id = ?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![conversation_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut steps = Vec::new();
        for json in rows {
            let json = json.map_err(|e| e.to_string())?;
            steps.push(
                serde_json::from_str(&json)
                    .map_err(|e| format!("Audit record is corrupt: {e}"))?,
            );
        }
        Ok(steps)
    }
}

fn messages_for(conn: &Connection, conversation_id: &str) -> Result<Vec<Message>, String> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, role, content, thinking, status, stats_json, tools_json, memories_used_json, created_at \
             FROM messages WHERE conversation_id = ?1 ORDER BY position",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], |row| {
            Ok(RawMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                thinking: row.get(3)?,
                status: row.get(4)?,
                stats_json: row.get(5)?,
                tools_json: row.get(6)?,
                memories_used_json: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    for raw in rows {
        messages.push(raw.map_err(|e| e.to_string())?.into_message()?);
    }
    Ok(messages)
}

fn migrate(conn: &mut Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if version > SCHEMA_VERSION {
        return Err(format!(
            "This database was written by a newer Hearth (schema {version}, supported {SCHEMA_VERSION})."
        ));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute_batch(SCHEMA_SQL).map_err(|e| {
        format!("Could not create the Hearth schema (is FTS5 available?): {e}")
    })?;
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EndpointConfig;
    use rusqlite::params;

    fn message(id: &str, content: &str, status: &str) -> Message {
        Message {
            id: id.into(),
            role: "user".into(),
            content: content.into(),
            thinking: String::new(),
            created_at: 1_000,
            status: status.into(),
            stats: None,
            tools: vec![],
            memories_used: vec![],
        }
    }

    fn conversation(id: &str) -> Conversation {
        Conversation {
            id: id.into(),
            title: "Test chat".into(),
            mode: "chat".into(),
            model_id: "test/model".into(),
            backend_id: "lmstudio".into(),
            pinned: false,
            incognito: false,
            memory_enabled: true,
            thinking: true,
            created_at: 1_000,
            updated_at: 2_000,
            messages: vec![],
        }
    }

    fn open_store(dir: &tempfile::TempDir) -> Store {
        Store::open(&dir.path().join("test.db")).unwrap()
    }

    #[test]
    fn reopens_with_wal_and_recovers_interrupted_streams() {
        let dir = tempfile::tempdir().unwrap();
        let mut conversation = conversation("c1");
        conversation.messages = vec![
            message("m1", "finished answer", "complete"),
            message("m2", "partial answer", "streaming"),
        ];
        {
            let store = open_store(&dir);
            store.save_conversation(&conversation).unwrap();
            let journal: String = store
                .guard()
                .unwrap()
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(journal, "wal");
        }
        // Reopen: the interrupted stream is kept but marked stopped.
        let store = open_store(&dir);
        let restored = store.conversations().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].messages.len(), 2);
        assert_eq!(restored[0].messages[0].status, "complete");
        assert_eq!(restored[0].messages[1].status, "stopped");
        assert_eq!(restored[0].messages[1].content, "partial answer");
        assert!(restored[0].thinking);
    }

    #[test]
    fn persists_exact_payloads_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c1");
        let mut assistant = message("m2", "answer", "complete");
        assistant.role = "assistant".into();
        assistant.thinking = "hmm".into();
        assistant.stats = Some(Stats {
            prompt_tokens: 10,
            completion_tokens: 5,
            ttft_ms: 40,
            duration_ms: 900,
            tokens_per_second: 5.5,
            estimated: true,
            backend_id: "lmstudio".into(),
            model_id: "test/model".into(),
        });
        assistant.tools = vec![ToolStep {
            id: "t1".into(),
            name: "calculator".into(),
            args: serde_json::json!({"expression": "2+2"}),
            output: "4".into(),
            status: "complete".into(),
            duration_ms: 3,
            preview: "2+2".into(),
        }];
        assistant.memories_used = vec!["mem1".into()];
        conversation.messages = vec![message("m1", "hello moonflower", "complete"), assistant];
        store.save_conversation(&conversation).unwrap();

        let restored = store.conversations().unwrap();
        assert_eq!(restored[0].messages[0].id, "m1");
        assert_eq!(restored[0].messages[1].id, "m2");
        let round = &restored[0].messages[1];
        assert_eq!(round.thinking, "hmm");
        assert_eq!(round.stats.as_ref().unwrap().tokens_per_second, 5.5);
        assert!(round.stats.as_ref().unwrap().estimated);
        assert_eq!(round.tools[0].args["expression"], "2+2");
        assert_eq!(round.memories_used, vec!["mem1".to_string()]);
    }

    #[test]
    fn search_is_case_insensitive_and_follows_edits_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c1");
        conversation.messages = vec![message("m1", "the MOONFLOWER blooms", "complete")];
        store.save_conversation(&conversation).unwrap();
        assert_eq!(store.search_conversations("moonflower").unwrap(), vec!["c1"]);

        // Edit: the old term vanishes, the new one is found.
        conversation.messages[0].content = "the orchid wilts".into();
        conversation.updated_at = 3_000;
        store.save_conversation(&conversation).unwrap();
        assert!(store.search_conversations("moonflower").unwrap().is_empty());
        assert_eq!(store.search_conversations("orchid").unwrap(), vec!["c1"]);

        // Delete the conversation: its text is gone from the index.
        store.delete_conversation("c1").unwrap();
        assert!(store.search_conversations("orchid").unwrap().is_empty());
    }

    #[test]
    fn search_matches_titles_and_ignores_thinking_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c1");
        conversation.title = "Renamed Chat".into();
        let mut assistant = message("m1", "visible", "complete");
        assistant.role = "assistant".into();
        assistant.thinking = "secretmoonflower".into();
        conversation.messages = vec![assistant];
        store.save_conversation(&conversation).unwrap();
        assert_eq!(store.search_conversations("renamed").unwrap(), vec!["c1"]);
        // Thinking text must never be searchable.
        assert!(store.search_conversations("secretmoonflower").unwrap().is_empty());
    }

    #[test]
    fn blank_search_returns_every_id_and_bad_syntax_degrades() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut older = conversation("c-old");
        older.updated_at = 1_000;
        older.messages = vec![message("m1", "hello", "complete")];
        let mut newer = conversation("c-new");
        newer.updated_at = 5_000;
        newer.created_at = 1_500;
        newer.messages = vec![message("m2", "other", "complete")];
        store.save_conversation(&older).unwrap();
        store.save_conversation(&newer).unwrap();
        assert_eq!(
            store.search_conversations("").unwrap(),
            vec!["c-new".to_string(), "c-old".to_string()]
        );
        assert_eq!(
            store.search_conversations("   ").unwrap(),
            vec!["c-new".to_string(), "c-old".to_string()]
        );
        for query in ["\"", "\"\"\"\"", "AND OR NOT(", "^"] {
            assert!(
                store.search_conversations(query).unwrap().is_empty(),
                "query {query:?} should degrade to empty"
            );
        }
        // A 10KB paste is truncated, not an error.
        let long = "word ".repeat(2_000);
        let _ = store.search_conversations(&long).unwrap();
    }

    #[test]
    fn search_propagates_real_sqlite_failures() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c1");
        conversation.messages = vec![message("m1", "hello", "complete")];
        store.save_conversation(&conversation).unwrap();
        store
            .guard()
            .unwrap()
            .execute_batch("DROP TABLE messages_fts;")
            .unwrap();
        let error = store.search_conversations("hello").unwrap_err();
        assert!(!error.is_empty(), "a missing index must not look like no matches: {error}");
        assert_eq!(store.search_conversations("").unwrap(), vec!["c1".to_string()]);
    }

    #[test]
    fn invalid_conversation_is_rejected_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut base = conversation("c1");
        base.messages = vec![message("m1", "valid", "complete")];
        store.save_conversation(&base).unwrap();

        let mut bad = base.clone();
        bad.messages.push(message("m2", "x", "streamin")); // bad status
        assert!(store.save_conversation(&bad).is_err());
        // The earlier good save is intact — no partial replacement happened.
        let restored = store.conversations().unwrap();
        assert_eq!(restored[0].messages.len(), 1);
        assert_eq!(restored[0].messages[0].id, "m1");

        let mut too_many = conversation("c2");
        too_many.messages = (0..MAX_MESSAGES + 1)
            .map(|i| message(&format!("x{i}"), "x", "complete"))
            .collect();
        assert!(store.save_conversation(&too_many).is_err());

        let mut huge = conversation("c3");
        huge.messages = vec![message("m1", &"x".repeat(MAX_TEXT_BYTES + 1), "complete")];
        assert!(store.save_conversation(&huge).is_err());
    }

    #[test]
    fn incognito_conversations_are_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c-secret");
        conversation.incognito = true;
        conversation.messages = vec![message("m1", "moonflower secret", "complete")];
        store.save_conversation(&conversation).unwrap();
        assert!(store.conversations().unwrap().is_empty());
        assert!(store.search_conversations("moonflower").unwrap().is_empty());
    }

    #[test]
    fn memory_round_trip_preserves_candidate_and_flag_states() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let memory = Memory {
            id: "mem1".into(),
            text: "Prefers concise answers".into(),
            category: "preference".into(),
            status: "candidate".into(),
            pinned: true,
            enabled: false,
            created_at: 5,
            updated_at: 6,
        };
        store.save_memory(&memory).unwrap();
        let memories = store.memories().unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].status, "candidate");
        assert!(memories[0].pinned);
        assert!(!memories[0].enabled);

        assert!(store.save_memory(&Memory {
            status: "unknown".into(),
            ..memory.clone()
        })
        .is_err());
        store.delete_memory("mem1").unwrap();
        assert!(store.memories().unwrap().is_empty());
    }

    #[test]
    fn settings_validate_normalise_and_repair_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let settings = Settings {
            network_tools: true,
            auto_extract: true,
            ..Default::default()
        };
        store.save_settings(&settings).unwrap();
        let loaded = store.settings().unwrap();
        assert!(!loaded.network_tools, "unsupported features are never stored on");
        assert!(!loaded.auto_extract);

        let bad = Settings {
            temperature: 3.0,
            ..Default::default()
        };
        assert!(store.save_settings(&bad).is_err());
        let bad = Settings {
            theme: "neon".into(),
            ..Default::default()
        };
        assert!(store.save_settings(&bad).is_err());
        let bad = Settings {
            context_window: 64,
            ..Default::default()
        };
        assert!(store.save_settings(&bad).is_err());
        let bad = Settings {
            accent: "orange".into(),
            ..Default::default()
        };
        assert!(store.save_settings(&bad).is_err());
        let mut bad = Settings::default();
        bad.backends.push(EndpointConfig {
            id: "lmstudio".into(),
            name: "dup".into(),
            kind: "lmstudio".into(),
            url: "http://127.0.0.1:1".into(),
            enabled: true,
        });
        assert!(store.save_settings(&bad).is_err());

        // Corrupt the stored payload by hand: error by name, database intact.
        store
            .guard()
            .unwrap()
            .execute("UPDATE settings SET json = '{not json' WHERE id = 1", [])
            .unwrap();
        assert!(store.settings().is_err());
        let mut conversation = conversation("c1");
        conversation.messages = vec![message("m1", "still works", "complete")];
        store.save_conversation(&conversation).unwrap();
        assert_eq!(store.conversations().unwrap().len(), 1);
        // A valid save repairs the row.
        store.save_settings(&Settings::default()).unwrap();
        assert!(store.settings().is_ok());
    }

    #[test]
    fn corrupt_message_payload_names_the_row_and_keeps_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c1");
        conversation.messages = vec![message("m1", "hello", "complete")];
        store.save_conversation(&conversation).unwrap();
        store
            .guard()
            .unwrap()
            .execute("UPDATE messages SET tools_json = 'nope' WHERE id = 'm1'", [])
            .unwrap();
        let error = store.conversations().unwrap_err();
        assert!(error.contains("m1"), "error should name the row: {error}");
        // Search (FTS) still works; the database was not discarded.
        assert_eq!(store.search_conversations("hello").unwrap(), vec!["c1"]);
    }

    #[test]
    fn audit_is_bounded_and_purged_with_its_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c1");
        conversation.messages = vec![message("m1", "hi", "complete")];
        store.save_conversation(&conversation).unwrap();
        for i in 0..(AUDIT_KEEP + 5) {
            let step = ToolStep {
                id: format!("t{i}"),
                name: "calculator".into(),
                args: serde_json::json!({"expression": "1+1"}),
                output: "2".into(),
                status: "complete".into(),
                duration_ms: 1,
                preview: "1+1".into(),
            };
            store.record_audit("c1", &step).unwrap();
        }
        let count: i64 = store
            .guard()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM audit", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count as usize, AUDIT_KEEP);

        store.delete_conversation("c1").unwrap();
        assert!(
            store.audit("c1").unwrap().is_empty(),
            "deletion must not leave hidden tool transcripts behind"
        );
    }

    #[test]
    fn deletes_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        store.delete_conversation("ghost").unwrap();
        store.delete_memory("ghost").unwrap();
    }

    #[test]
    fn duplicate_message_id_rolls_back_the_save() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut base = conversation("c1");
        base.messages = vec![message("m1", "kept", "complete")];
        store.save_conversation(&base).unwrap();

        let mut bad = base.clone();
        bad.updated_at = 9_000;
        bad.messages = vec![
            message("m-dup", "first", "complete"),
            message("m-dup", "second", "complete"),
        ];
        assert!(store.save_conversation(&bad).is_err());
        let restored = store.conversations().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].updated_at, 2_000);
        assert_eq!(restored[0].messages.len(), 1);
        assert_eq!(restored[0].messages[0].content, "kept");
    }

    #[test]
    fn stats_rate_must_be_finite_and_nonnegative() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let mut conversation = conversation("c1");
        let mut assistant = message("m1", "kept", "complete");
        assistant.stats = Some(Stats {
            prompt_tokens: 1,
            completion_tokens: 1,
            ttft_ms: 1,
            duration_ms: 1,
            tokens_per_second: 1.0,
            estimated: false,
            backend_id: "lmstudio".into(),
            model_id: "m".into(),
        });
        conversation.messages = vec![assistant];
        store.save_conversation(&conversation).unwrap();

        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1] {
            let mut bad = conversation.clone();
            bad.messages[0].stats.as_mut().unwrap().tokens_per_second = rate;
            assert!(
                store.save_conversation(&bad).is_err(),
                "rate {rate} should be rejected"
            );
        }
        let restored = store.conversations().unwrap();
        assert_eq!(restored[0].messages[0].content, "kept");
        assert_eq!(
            restored[0].messages[0].stats.as_ref().unwrap().tokens_per_second,
            1.0
        );
    }

    #[test]
    fn settings_load_normalises_flags_and_keeps_invalid_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        store.save_settings(&Settings::default()).unwrap();
        let json: String = store
            .guard()
            .unwrap()
            .query_row("SELECT json FROM settings WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["networkTools"] = serde_json::json!(true);
        value["autoExtract"] = serde_json::json!(true);
        store
            .guard()
            .unwrap()
            .execute(
                "UPDATE settings SET json = ?1 WHERE id = 1",
                params![value.to_string()],
            )
            .unwrap();
        let loaded = store.settings().unwrap();
        assert!(!loaded.network_tools);
        assert!(!loaded.auto_extract);
        let stored: String = store
            .guard()
            .unwrap()
            .query_row("SELECT json FROM settings WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert!(
            stored.contains("\"networkTools\":true"),
            "load normalises without rewriting the row: {stored}"
        );

        value["theme"] = serde_json::json!("neon");
        store
            .guard()
            .unwrap()
            .execute(
                "UPDATE settings SET json = ?1 WHERE id = 1",
                params![value.to_string()],
            )
            .unwrap();
        let error = store.settings().unwrap_err();
        assert!(
            error.contains("Theme") || error.contains("invalid"),
            "{error}"
        );
        let still: String = store
            .guard()
            .unwrap()
            .query_row("SELECT json FROM settings WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert!(still.contains("neon"), "invalid settings stay on disk");
        let mut conversation = conversation("c1");
        conversation.messages = vec![message("m1", "still works", "complete")];
        store.save_conversation(&conversation).unwrap();
        assert_eq!(store.conversations().unwrap().len(), 1);
    }

    fn memory_ids_matching(store: &Store, term: &str) -> Vec<String> {
        let conn = store.guard().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT m.id FROM memories_fts \
                 JOIN memories m ON m.seq = memories_fts.rowid \
                 WHERE memories_fts MATCH ?1",
            )
            .unwrap();
        let query = format!("\"{term}\"");
        let rows = stmt
            .query_map(params![query], |row| row.get::<_, String>(0))
            .unwrap();
        rows.map(|row| row.unwrap()).collect()
    }

    #[test]
    fn memory_fts_follows_edits_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let memory = Memory {
            id: "mem1".into(),
            text: "moonflower tea".into(),
            category: "preference".into(),
            status: "active".into(),
            pinned: false,
            enabled: true,
            created_at: 5,
            updated_at: 6,
        };
        store.save_memory(&memory).unwrap();
        assert_eq!(memory_ids_matching(&store, "moonflower"), vec!["mem1"]);

        let mut edited = memory.clone();
        edited.text = "orchid tea".into();
        edited.updated_at = 7;
        store.save_memory(&edited).unwrap();
        assert!(memory_ids_matching(&store, "moonflower").is_empty());
        assert_eq!(memory_ids_matching(&store, "orchid"), vec!["mem1"]);

        store.delete_memory("mem1").unwrap();
        assert!(memory_ids_matching(&store, "orchid").is_empty());
    }
}
