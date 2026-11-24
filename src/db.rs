//! SQLite 持久化：供应商、模型、偏好、生图话题 / 消息 / 图片记录、聊天助手 / 对话 / 消息。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::title::TitleSource;

pub const PREF_IMAGE_DIR: &str = "image_dir";
pub const PREF_TITLE_PROVIDER: &str = "title_provider_id";
pub const PREF_TITLE_MODEL: &str = "title_model_id";
pub const PREF_TITLE_PROMPT: &str = "title_prompt";
pub const PREF_UI_FONT: &str = "ui_font";
pub const PREF_UI_FONT_SIZE: &str = "ui_font_size";
pub const DEFAULT_IMAGE_DIR: &str = "~/.images/";
pub const DEFAULT_UI_FONT_SIZE: u8 = 13;
pub const UI_FONT_SIZE_MIN: u8 = 12;
pub const UI_FONT_SIZE_MAX: u8 = 18;

/// 供应商协议类型：决定模型列表、对话与标题请求的格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderKind {
    /// OpenAI 兼容：Bearer 鉴权，对话走 `/responses`。
    #[default]
    OpenAI,
    /// Anthropic：`x-api-key` 鉴权，对话走 `/messages`。
    Anthropic,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::OpenAI, ProviderKind::Anthropic];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAI => "openai",
            ProviderKind::Anthropic => "anthropic",
        }
    }

    pub fn parse(s: &str) -> ProviderKind {
        if s == "anthropic" { ProviderKind::Anthropic } else { ProviderKind::OpenAI }
    }

    /// 界面显示名。
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::OpenAI => "OpenAI",
            ProviderKind::Anthropic => "Claude",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub enabled: bool,
    pub kind: ProviderKind,
}

/// 聊天助手：名称 + 系统提示词；`is_default` 的助手不可删除。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assistant {
    pub id: i64,
    pub name: String,
    pub system_prompt: String,
    pub is_default: bool,
}

/// 聊天对话，归属某个助手。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: i64,
    pub assistant_id: i64,
    pub title: String,
    pub title_source: TitleSource,
    pub title_error: Option<String>,
    pub last_provider_id: Option<i64>,
    pub last_model_id: Option<String>,
    pub updated_at: String,
}

/// 聊天消息；助手消息的 `content` 为回复正文（Markdown）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub role: Role,
    pub content: String,
    pub model_id: Option<String>,
    pub request_json: Option<String>,
    pub status: Status,
    pub http_status: Option<u16>,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub image_dir: String,
    pub title_provider_id: Option<i64>,
    pub title_model_id: String,
    pub title_prompt: String,
    /// 界面字体族名；`None` 为“自动”（系统字体）。
    pub ui_font: Option<String>,
    /// 界面正文字号，12–18。
    pub ui_font_size: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub id: i64,
    pub title: String,
    pub title_source: TitleSource,
    pub title_error: Option<String>,
    pub last_provider_id: Option<i64>,
    pub last_model_id: Option<String>,
    pub last_size: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Success,
    Error,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRecord {
    pub local_path: String,
    pub format: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source_field: String,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: i64,
    pub topic_id: i64,
    pub parent_id: Option<i64>,
    pub role: Role,
    pub content: String,
    pub provider_id: Option<i64>,
    pub model_id: Option<String>,
    pub size: Option<String>,
    pub request_json: Option<String>,
    pub status: Status,
    pub http_status: Option<u16>,
    pub response_body: Option<String>,
    pub error: Option<String>,
    pub failures: Vec<String>,
    pub images: Vec<ImageRecord>,
    pub created_at: String,
}

/// 用户消息中记录的发送内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentRequest {
    pub provider_id: i64,
    pub model_id: String,
    pub size: String,
    pub request_json: String,
}

/// 助手消息的最终结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantResult {
    pub status: Status,
    pub http_status: Option<u16>,
    pub response_body: Option<String>,
    pub error: Option<String>,
    pub failures: Vec<String>,
    pub images: Vec<ImageRecord>,
}

/// 供应商名称必填，去掉首尾空白。
fn required_name(name: &str) -> DbResult<&str> {
    let name = name.trim();
    if name.is_empty() { Err("请填写供应商名称".into()) } else { Ok(name) }
}

pub struct Db {
    conn: Connection,
}

pub type DbResult<T> = Result<T, String>;

fn e(err: rusqlite::Error) -> String {
    format!("数据库错误：{err}")
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

const SCHEMA_VERSION: i64 = 2;
const DEFAULT_ASSISTANT_NAME: &str = "default";

const SCHEMA_V1: &str = r#"
CREATE TABLE providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    api_key TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    sort INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE provider_models (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(provider_id, model_id)
);
CREATE TABLE preferences (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE topics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    title_source TEXT NOT NULL DEFAULT 'default',
    title_error TEXT,
    last_provider_id INTEGER,
    last_model_id TEXT,
    last_size TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    parent_id INTEGER,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    provider_id INTEGER,
    model_id TEXT,
    size TEXT,
    request_json TEXT,
    status TEXT NOT NULL,
    http_status INTEGER,
    response_body TEXT,
    error TEXT,
    failures TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_messages_topic ON messages(topic_id, id);
CREATE TABLE message_images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    local_path TEXT NOT NULL,
    format TEXT NOT NULL,
    width INTEGER,
    height INTEGER,
    source TEXT NOT NULL,
    origin TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_images_message ON message_images(message_id);
"#;

/// v2：供应商类型；聊天助手、对话、消息。
const SCHEMA_V2: &str = r#"
ALTER TABLE providers ADD COLUMN kind TEXT NOT NULL DEFAULT 'openai';
CREATE TABLE assistants (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    system_prompt TEXT NOT NULL DEFAULT '',
    is_default INTEGER NOT NULL DEFAULT 0,
    sort INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE conversations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    assistant_id INTEGER NOT NULL REFERENCES assistants(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    title_source TEXT NOT NULL DEFAULT 'default',
    title_error TEXT,
    last_provider_id INTEGER,
    last_model_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_conversations_assistant ON conversations(assistant_id, updated_at);
CREATE TABLE chat_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    provider_id INTEGER,
    model_id TEXT,
    request_json TEXT,
    status TEXT NOT NULL,
    http_status INTEGER,
    error TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_chat_messages_conversation ON chat_messages(conversation_id, id);
"#;

fn role_str(r: Role) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn parse_role(s: &str) -> Role {
    if s == "user" { Role::User } else { Role::Assistant }
}

fn status_str(s: Status) -> &'static str {
    match s {
        Status::Pending => "pending",
        Status::Success => "success",
        Status::Error => "error",
        Status::Interrupted => "interrupted",
    }
}

fn parse_status(s: &str) -> Status {
    match s {
        "pending" => Status::Pending,
        "success" => Status::Success,
        "interrupted" => Status::Interrupted,
        _ => Status::Error,
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> DbResult<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("无法设置数据库文件权限 {}：{err}", path.display()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> DbResult<()> {
    Ok(())
}

fn try_open(path: &Path) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    // 损坏或非 SQLite 文件在第一次真正读取时报错
    let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            Some(check),
        ));
    }
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(&format!("BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"))?;
    }
    if version < 2 {
        let ts = now();
        conn.execute_batch(&format!(
            "BEGIN; {SCHEMA_V2}
             INSERT INTO assistants (name, system_prompt, is_default, sort, created_at, updated_at)
             VALUES ('{DEFAULT_ASSISTANT_NAME}', '', 1, 0, '{ts}', '{ts}');
             PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
        ))?;
    }
    Ok(())
}

impl Db {
    /// 打开数据库；文件损坏时改名备份并新建空库，返回备份路径。
    pub fn open(path: &Path) -> DbResult<(Db, Option<PathBuf>)> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("无法创建数据目录 {}：{err}", parent.display()))?;
        }
        let existed = path.exists();
        let (conn, backup) = match try_open(path) {
            Ok(conn) => (conn, None),
            Err(err) if existed => {
                let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let mut name = path.as_os_str().to_owned();
                name.push(format!(".bak.{stamp}"));
                let backup = PathBuf::from(name);
                std::fs::rename(path, &backup).map_err(|re| {
                    format!("数据库无法打开（{err}），且备份失败 {}：{re}", backup.display())
                })?;
                (try_open(path).map_err(e)?, Some(backup))
            }
            Err(err) => return Err(e(err)),
        };
        restrict_permissions(path)?;
        Ok((Db { conn }, backup))
    }

    pub fn add_provider(&self, name: &str, base_url: &str, api_key: &str, kind: ProviderKind) -> DbResult<i64> {
        let name = required_name(name)?;
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO providers (name, base_url, api_key, enabled, kind, sort, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4, COALESCE((SELECT MAX(sort) + 1 FROM providers), 0), ?5, ?5)",
                (name, base_url.trim(), api_key.trim(), kind.as_str(), &ts),
            )
            .map_err(e)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_provider(&self, p: &Provider) -> DbResult<()> {
        let name = required_name(&p.name)?;
        self.conn
            .execute(
                "UPDATE providers SET name = ?1, base_url = ?2, api_key = ?3, enabled = ?4, kind = ?5, updated_at = ?6
                 WHERE id = ?7",
                (name, &p.base_url, &p.api_key, p.enabled, p.kind.as_str(), now(), p.id),
            )
            .map_err(e)?;
        Ok(())
    }

    /// 删除供应商及其模型；若被标题设置引用则清空该设置。
    pub fn delete_provider(&self, id: i64) -> DbResult<()> {
        let tx = self.conn.unchecked_transaction().map_err(e)?;
        tx.execute("DELETE FROM providers WHERE id = ?1", [id]).map_err(e)?;
        let referenced: Option<String> = tx
            .query_row("SELECT value FROM preferences WHERE key = ?1", [PREF_TITLE_PROVIDER], |r| r.get(0))
            .optional()
            .map_err(e)?;
        if referenced.as_deref() == Some(id.to_string().as_str()) {
            tx.execute(
                "DELETE FROM preferences WHERE key IN (?1, ?2)",
                (PREF_TITLE_PROVIDER, PREF_TITLE_MODEL),
            )
            .map_err(e)?;
        }
        tx.commit().map_err(e)
    }

    pub fn list_providers(&self) -> DbResult<Vec<Provider>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, base_url, api_key, enabled, kind FROM providers ORDER BY sort, id")
            .map_err(e)?;
        stmt.query_map([], |r| {
            Ok(Provider {
                id: r.get(0)?,
                name: r.get(1)?,
                base_url: r.get(2)?,
                api_key: r.get(3)?,
                enabled: r.get(4)?,
                kind: ProviderKind::parse(&r.get::<_, String>(5)?),
            })
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)
    }

    /// 用 `models` 整体替换该供应商的保留模型。
    pub fn set_provider_models(&self, provider_id: i64, models: &[String]) -> DbResult<()> {
        let tx = self.conn.unchecked_transaction().map_err(e)?;
        tx.execute("DELETE FROM provider_models WHERE provider_id = ?1", [provider_id]).map_err(e)?;
        let ts = now();
        for m in models {
            tx.execute(
                "INSERT OR IGNORE INTO provider_models (provider_id, model_id, created_at) VALUES (?1, ?2, ?3)",
                (provider_id, m, &ts),
            )
            .map_err(e)?;
        }
        tx.commit().map_err(e)
    }

    pub fn provider_models(&self, provider_id: i64) -> DbResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT model_id FROM provider_models WHERE provider_id = ?1 ORDER BY id")
            .map_err(e)?;
        stmt.query_map([provider_id], |r| r.get(0)).map_err(e)?.collect::<Result<_, _>>().map_err(e)
    }

    fn get_pref(&self, key: &str) -> DbResult<Option<String>> {
        self.conn
            .query_row("SELECT value FROM preferences WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(e)
    }

    fn set_pref(&self, key: &str, value: Option<&str>) -> DbResult<()> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO preferences (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (key, v),
            ),
            None => self.conn.execute("DELETE FROM preferences WHERE key = ?1", [key]),
        }
        .map_err(e)?;
        Ok(())
    }

    pub fn load_settings(&self) -> DbResult<Settings> {
        Ok(Settings {
            image_dir: self.get_pref(PREF_IMAGE_DIR)?.unwrap_or_else(|| DEFAULT_IMAGE_DIR.into()),
            title_provider_id: self.get_pref(PREF_TITLE_PROVIDER)?.and_then(|v| v.parse().ok()),
            title_model_id: self.get_pref(PREF_TITLE_MODEL)?.unwrap_or_default(),
            title_prompt: self
                .get_pref(PREF_TITLE_PROMPT)?
                .unwrap_or_else(|| crate::title::DEFAULT_TITLE_PROMPT.into()),
            ui_font: self.get_pref(PREF_UI_FONT)?.filter(|v| !v.is_empty()),
            ui_font_size: self
                .get_pref(PREF_UI_FONT_SIZE)?
                .and_then(|v| v.parse::<u8>().ok())
                .map_or(DEFAULT_UI_FONT_SIZE, clamp_font_size),
        })
    }

    pub fn save_settings(&self, s: &Settings) -> DbResult<()> {
        let tx = self.conn.unchecked_transaction().map_err(e)?;
        self.set_pref(PREF_IMAGE_DIR, Some(&s.image_dir))?;
        self.set_pref(PREF_TITLE_PROVIDER, s.title_provider_id.map(|v| v.to_string()).as_deref())?;
        self.set_pref(PREF_TITLE_MODEL, Some(&s.title_model_id))?;
        self.set_pref(PREF_TITLE_PROMPT, Some(&s.title_prompt))?;
        self.set_pref(PREF_UI_FONT, s.ui_font.as_deref())?;
        self.set_pref(PREF_UI_FONT_SIZE, Some(&clamp_font_size(s.ui_font_size).to_string()))?;
        tx.commit().map_err(e)
    }

    pub fn create_topic(&self) -> DbResult<Topic> {
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO topics (title, title_source, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                (crate::title::DEFAULT_TOPIC_TITLE, TitleSource::Default.as_str(), &ts),
            )
            .map_err(e)?;
        let id = self.conn.last_insert_rowid();
        self.get_topic(id)?.ok_or_else(|| "新建话题后读取失败".to_string())
    }

    const TOPIC_COLS: &'static str = "id, title, title_source, title_error, last_provider_id, last_model_id, last_size, created_at, updated_at";

    fn row_topic(r: &rusqlite::Row) -> rusqlite::Result<Topic> {
        Ok(Topic {
            id: r.get(0)?,
            title: r.get(1)?,
            title_source: TitleSource::parse(&r.get::<_, String>(2)?),
            title_error: r.get(3)?,
            last_provider_id: r.get(4)?,
            last_model_id: r.get(5)?,
            last_size: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        })
    }

    pub fn list_topics(&self) -> DbResult<Vec<Topic>> {
        let sql = format!("SELECT {} FROM topics ORDER BY updated_at DESC, id DESC", Self::TOPIC_COLS);
        let mut stmt = self.conn.prepare(&sql).map_err(e)?;
        stmt.query_map([], Self::row_topic).map_err(e)?.collect::<Result<_, _>>().map_err(e)
    }

    pub fn get_topic(&self, id: i64) -> DbResult<Option<Topic>> {
        let sql = format!("SELECT {} FROM topics WHERE id = ?1", Self::TOPIC_COLS);
        self.conn.query_row(&sql, [id], Self::row_topic).optional().map_err(e)
    }

    pub fn set_topic_title(
        &self,
        id: i64,
        title: &str,
        source: TitleSource,
        error: Option<&str>,
    ) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE topics SET title = ?1, title_source = ?2, title_error = ?3 WHERE id = ?4",
                (title, source.as_str(), error, id),
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn set_topic_selection(&self, id: i64, provider_id: i64, model_id: &str, size: &str) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE topics SET last_provider_id = ?1, last_model_id = ?2, last_size = ?3 WHERE id = ?4",
                (provider_id, model_id, size, id),
            )
            .map_err(e)?;
        Ok(())
    }

    /// 删除话题及其消息记录；不删除图片文件。
    pub fn delete_topic(&self, id: i64) -> DbResult<()> {
        self.conn.execute("DELETE FROM topics WHERE id = ?1", [id]).map_err(e)?;
        Ok(())
    }

    fn touch_topic(&self, id: i64, ts: &str) -> DbResult<()> {
        self.conn.execute("UPDATE topics SET updated_at = ?1 WHERE id = ?2", (ts, id)).map_err(e)?;
        Ok(())
    }

    pub fn add_user_message(&self, topic_id: i64, content: &str, sent: &SentRequest) -> DbResult<i64> {
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO messages (topic_id, role, content, provider_id, model_id, size, request_json, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    topic_id,
                    role_str(Role::User),
                    content,
                    sent.provider_id,
                    &sent.model_id,
                    &sent.size,
                    &sent.request_json,
                    status_str(Status::Success),
                    &ts,
                ),
            )
            .map_err(e)?;
        let id = self.conn.last_insert_rowid();
        self.touch_topic(topic_id, &ts)?;
        Ok(id)
    }

    pub fn add_pending_assistant(&self, topic_id: i64, parent_id: i64) -> DbResult<i64> {
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO messages (topic_id, parent_id, role, content, status, created_at)
                 VALUES (?1, ?2, ?3, '', ?4, ?5)",
                (topic_id, parent_id, role_str(Role::Assistant), status_str(Status::Pending), &ts),
            )
            .map_err(e)?;
        let id = self.conn.last_insert_rowid();
        self.touch_topic(topic_id, &ts)?;
        Ok(id)
    }

    pub fn finish_assistant(&self, id: i64, r: &AssistantResult) -> DbResult<()> {
        let tx = self.conn.unchecked_transaction().map_err(e)?;
        let failures = serde_json::to_string(&r.failures).map_err(|err| err.to_string())?;
        tx.execute(
            "UPDATE messages SET status = ?1, http_status = ?2, response_body = ?3, error = ?4, failures = ?5
             WHERE id = ?6",
            (status_str(r.status), r.http_status, &r.response_body, &r.error, failures, id),
        )
        .map_err(e)?;
        let ts = now();
        for img in &r.images {
            tx.execute(
                "INSERT INTO message_images (message_id, local_path, format, width, height, source, origin, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (id, &img.local_path, &img.format, img.width, img.height, &img.source_field, &img.origin, &ts),
            )
            .map_err(e)?;
        }
        tx.commit().map_err(e)
    }

    pub fn list_messages(&self, topic_id: i64) -> DbResult<Vec<Message>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, topic_id, parent_id, role, content, provider_id, model_id, size, request_json,
                        status, http_status, response_body, error, failures, created_at
                 FROM messages WHERE topic_id = ?1 ORDER BY id",
            )
            .map_err(e)?;
        let mut messages: Vec<Message> = stmt
            .query_map([topic_id], |r| {
                let failures: Option<String> = r.get(13)?;
                Ok(Message {
                    id: r.get(0)?,
                    topic_id: r.get(1)?,
                    parent_id: r.get(2)?,
                    role: parse_role(&r.get::<_, String>(3)?),
                    content: r.get(4)?,
                    provider_id: r.get(5)?,
                    model_id: r.get(6)?,
                    size: r.get(7)?,
                    request_json: r.get(8)?,
                    status: parse_status(&r.get::<_, String>(9)?),
                    http_status: r.get(10)?,
                    response_body: r.get(11)?,
                    error: r.get(12)?,
                    failures: failures.and_then(|f| serde_json::from_str(&f).ok()).unwrap_or_default(),
                    images: Vec::new(),
                    created_at: r.get(14)?,
                })
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;

        let mut img_stmt = self
            .conn
            .prepare(
                "SELECT i.message_id, i.local_path, i.format, i.width, i.height, i.source, i.origin
                 FROM message_images i JOIN messages m ON m.id = i.message_id
                 WHERE m.topic_id = ?1 ORDER BY i.id",
            )
            .map_err(e)?;
        let rows = img_stmt
            .query_map([topic_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    ImageRecord {
                        local_path: r.get(1)?,
                        format: r.get(2)?,
                        width: r.get(3)?,
                        height: r.get(4)?,
                        source_field: r.get(5)?,
                        origin: r.get(6)?,
                    },
                ))
            })
            .map_err(e)?;
        for row in rows {
            let (mid, img) = row.map_err(e)?;
            if let Some(m) = messages.iter_mut().find(|m| m.id == mid) {
                m.images.push(img);
            }
        }
        Ok(messages)
    }

    /// 启动时把遗留的 pending 消息（生图与聊天）改为 interrupted，返回条数。
    pub fn mark_pending_interrupted(&self) -> DbResult<usize> {
        let mut n = 0;
        for table in ["messages", "chat_messages"] {
            n += self
                .conn
                .execute(
                    &format!("UPDATE {table} SET status = ?1, error = '应用退出，请求已中断' WHERE status = ?2"),
                    (status_str(Status::Interrupted), status_str(Status::Pending)),
                )
                .map_err(e)?;
        }
        Ok(n)
    }

    // ---------- 聊天助手 ----------

    pub fn list_assistants(&self) -> DbResult<Vec<Assistant>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, system_prompt, is_default FROM assistants ORDER BY is_default DESC, sort, id")
            .map_err(e)?;
        stmt.query_map([], |r| {
            Ok(Assistant { id: r.get(0)?, name: r.get(1)?, system_prompt: r.get(2)?, is_default: r.get(3)? })
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)
    }

    pub fn add_assistant(&self, name: &str, system_prompt: &str) -> DbResult<i64> {
        let name = required_assistant_name(name)?;
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO assistants (name, system_prompt, is_default, sort, created_at, updated_at)
                 VALUES (?1, ?2, 0, COALESCE((SELECT MAX(sort) + 1 FROM assistants), 0), ?3, ?3)",
                (name, system_prompt, &ts),
            )
            .map_err(e)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_assistant(&self, a: &Assistant) -> DbResult<()> {
        let name = required_assistant_name(&a.name)?;
        self.conn
            .execute(
                "UPDATE assistants SET name = ?1, system_prompt = ?2, updated_at = ?3 WHERE id = ?4",
                (name, &a.system_prompt, now(), a.id),
            )
            .map_err(e)?;
        Ok(())
    }

    /// 删除助手及其全部对话与消息；默认助手不可删除。
    pub fn delete_assistant(&self, id: i64) -> DbResult<()> {
        let is_default: Option<bool> = self
            .conn
            .query_row("SELECT is_default FROM assistants WHERE id = ?1", [id], |r| r.get(0))
            .optional()
            .map_err(e)?;
        if is_default == Some(true) {
            return Err("默认助手不能删除".into());
        }
        self.conn.execute("DELETE FROM assistants WHERE id = ?1", [id]).map_err(e)?;
        Ok(())
    }

    // ---------- 聊天对话 ----------

    const CONV_COLS: &'static str =
        "id, assistant_id, title, title_source, title_error, last_provider_id, last_model_id, updated_at";

    fn row_conversation(r: &rusqlite::Row) -> rusqlite::Result<Conversation> {
        Ok(Conversation {
            id: r.get(0)?,
            assistant_id: r.get(1)?,
            title: r.get(2)?,
            title_source: TitleSource::parse(&r.get::<_, String>(3)?),
            title_error: r.get(4)?,
            last_provider_id: r.get(5)?,
            last_model_id: r.get(6)?,
            updated_at: r.get(7)?,
        })
    }

    pub fn create_conversation(&self, assistant_id: i64) -> DbResult<Conversation> {
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO conversations (assistant_id, title, title_source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                (assistant_id, crate::title::DEFAULT_CONVERSATION_TITLE, TitleSource::Default.as_str(), &ts),
            )
            .map_err(e)?;
        let id = self.conn.last_insert_rowid();
        self.get_conversation(id)?.ok_or_else(|| "新建对话后读取失败".to_string())
    }

    /// 该助手的对话，最近更新在前。
    pub fn list_conversations(&self, assistant_id: i64) -> DbResult<Vec<Conversation>> {
        let sql = format!(
            "SELECT {} FROM conversations WHERE assistant_id = ?1 ORDER BY updated_at DESC, id DESC",
            Self::CONV_COLS
        );
        let mut stmt = self.conn.prepare(&sql).map_err(e)?;
        stmt.query_map([assistant_id], Self::row_conversation).map_err(e)?.collect::<Result<_, _>>().map_err(e)
    }

    pub fn get_conversation(&self, id: i64) -> DbResult<Option<Conversation>> {
        let sql = format!("SELECT {} FROM conversations WHERE id = ?1", Self::CONV_COLS);
        self.conn.query_row(&sql, [id], Self::row_conversation).optional().map_err(e)
    }

    pub fn set_conversation_title(
        &self,
        id: i64,
        title: &str,
        source: TitleSource,
        error: Option<&str>,
    ) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE conversations SET title = ?1, title_source = ?2, title_error = ?3 WHERE id = ?4",
                (title, source.as_str(), error, id),
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn set_conversation_model(&self, id: i64, provider_id: i64, model_id: &str) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE conversations SET last_provider_id = ?1, last_model_id = ?2 WHERE id = ?3",
                (provider_id, model_id, id),
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn delete_conversation(&self, id: i64) -> DbResult<()> {
        self.conn.execute("DELETE FROM conversations WHERE id = ?1", [id]).map_err(e)?;
        Ok(())
    }

    /// 是否还没有任何消息（用于复用空对话）。
    pub fn conversation_is_empty(&self, id: i64) -> DbResult<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chat_messages WHERE conversation_id = ?1", [id], |r| r.get(0))
            .map_err(e)?;
        Ok(n == 0)
    }

    fn touch_conversation(&self, id: i64, ts: &str) -> DbResult<()> {
        self.conn.execute("UPDATE conversations SET updated_at = ?1 WHERE id = ?2", (ts, id)).map_err(e)?;
        Ok(())
    }

    // ---------- 聊天消息 ----------

    /// 记录用户消息与本次发送的请求体（不含 Key）。
    pub fn add_chat_user(
        &self,
        conversation_id: i64,
        content: &str,
        provider_id: i64,
        model_id: &str,
        request_json: &str,
    ) -> DbResult<i64> {
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO chat_messages (conversation_id, role, content, provider_id, model_id, request_json, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (
                    conversation_id,
                    role_str(Role::User),
                    content,
                    provider_id,
                    model_id,
                    request_json,
                    status_str(Status::Success),
                    &ts,
                ),
            )
            .map_err(e)?;
        let id = self.conn.last_insert_rowid();
        self.touch_conversation(conversation_id, &ts)?;
        Ok(id)
    }

    pub fn add_chat_pending(&self, conversation_id: i64) -> DbResult<i64> {
        let ts = now();
        self.conn
            .execute(
                "INSERT INTO chat_messages (conversation_id, role, content, status, created_at) VALUES (?1, ?2, '', ?3, ?4)",
                (conversation_id, role_str(Role::Assistant), status_str(Status::Pending), &ts),
            )
            .map_err(e)?;
        let id = self.conn.last_insert_rowid();
        self.touch_conversation(conversation_id, &ts)?;
        Ok(id)
    }

    pub fn finish_chat(
        &self,
        id: i64,
        status: Status,
        content: &str,
        http_status: Option<u16>,
        error: Option<&str>,
    ) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE chat_messages SET status = ?1, content = ?2, http_status = ?3, error = ?4 WHERE id = ?5",
                (status_str(status), content, http_status, error, id),
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn list_chat_messages(&self, conversation_id: i64) -> DbResult<Vec<ChatMessage>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, conversation_id, role, content, model_id, request_json, status, http_status, error, created_at
                 FROM chat_messages WHERE conversation_id = ?1 ORDER BY id",
            )
            .map_err(e)?;
        stmt.query_map([conversation_id], |r| {
            Ok(ChatMessage {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                role: parse_role(&r.get::<_, String>(2)?),
                content: r.get(3)?,
                model_id: r.get(4)?,
                request_json: r.get(5)?,
                status: parse_status(&r.get::<_, String>(6)?),
                http_status: r.get(7)?,
                error: r.get(8)?,
                created_at: r.get(9)?,
            })
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)
    }
}

/// 助手名称必填，去掉首尾空白。
fn required_assistant_name(name: &str) -> DbResult<&str> {
    let name = name.trim();
    if name.is_empty() { Err("请填写助手名称".into()) } else { Ok(name) }
}

/// 字号限制在 12–18。
pub fn clamp_font_size(size: u8) -> u8 {
    size.clamp(UI_FONT_SIZE_MIN, UI_FONT_SIZE_MAX)
}
