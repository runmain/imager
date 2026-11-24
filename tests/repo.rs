use imageclient::db::*;
use imageclient::title::{TitleSource, DEFAULT_TITLE_PROMPT};

const KEY: &str = "sk-test-SECRET-123";

fn fresh() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("imageclient.db");
    (dir, path)
}

// AC-001, AC-002, AC-003
#[test]
fn providers_roundtrip_across_reopen() {
    let (_d, path) = fresh();
    {
        let (db, bak) = Db::open(&path).unwrap();
        assert!(bak.is_none());
        let a = db.add_provider("A", "https://a/v1", KEY, ProviderKind::OpenAI).unwrap();
        let b = db.add_provider("B", "https://b/v1/", "k2", ProviderKind::OpenAI).unwrap();
        let mut pb = db.list_providers().unwrap().into_iter().find(|p| p.id == b).unwrap();
        assert!(pb.enabled, "new provider enabled by default");
        pb.enabled = false;
        pb.name = "B2".into();
        db.update_provider(&pb).unwrap();
        db.set_provider_models(a, &["m1".into(), "m2".into()]).unwrap();
        db.set_provider_models(a, &["m2".into()]).unwrap();
    }
    let (db, _) = Db::open(&path).unwrap();
    let ps = db.list_providers().unwrap();
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].name, "A");
    assert_eq!(ps[0].api_key, KEY);
    assert_eq!(ps[1].name, "B2");
    assert!(!ps[1].enabled);
    assert_eq!(db.provider_models(ps[0].id).unwrap(), vec!["m2"]);
}


// AC-001：供应商名称必填
#[test]
fn provider_name_required() {
    let (_d, path) = fresh();
    let (db, _) = Db::open(&path).unwrap();
    assert!(db.add_provider("   ", "https://a", KEY, ProviderKind::OpenAI).unwrap_err().contains("名称"));
    let id = db.add_provider("  A  ", "https://a", KEY, ProviderKind::OpenAI).unwrap();
    let mut p = db.list_providers().unwrap().remove(0);
    assert_eq!(p.name, "A", "name is trimmed");
    p.name = "".into();
    assert!(db.update_provider(&p).unwrap_err().contains("名称"));
    assert_eq!(db.list_providers().unwrap()[0].name, "A");
    assert_eq!(db.list_providers().unwrap()[0].id, id);
}

// AC-012
#[cfg(unix)]
#[test]
fn db_file_is_0600() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, path) = fresh();
    let _ = Db::open(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

// BN-007
#[test]
fn corrupt_db_is_backed_up_not_overwritten() {
    let (_d, path) = fresh();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"this is not sqlite at all, garbage garbage garbage").unwrap();
    let (db, bak) = Db::open(&path).unwrap();
    let bak = bak.expect("backup path returned");
    assert_eq!(std::fs::read(&bak).unwrap(), b"this is not sqlite at all, garbage garbage garbage");
    assert!(db.list_providers().unwrap().is_empty());
}

// AC-004, AC-015
#[test]
fn settings_defaults_and_roundtrip() {
    let (_d, path) = fresh();
    let (db, _) = Db::open(&path).unwrap();
    let s = db.load_settings().unwrap();
    assert_eq!(s.image_dir, DEFAULT_IMAGE_DIR);
    assert_eq!(s.title_provider_id, None);
    assert_eq!(s.title_prompt, DEFAULT_TITLE_PROMPT);
    assert_eq!(s.ui_font, None, "默认字体为“自动”");
    assert_eq!(s.ui_font_size, DEFAULT_UI_FONT_SIZE);
    let p = db.add_provider("T", "https://t", KEY, ProviderKind::OpenAI).unwrap();
    let new = Settings {
        image_dir: "/tmp/x".into(),
        title_provider_id: Some(p),
        title_model_id: "gpt-4o-mini".into(),
        title_prompt: "自定义 {prompt}".into(),
        ui_font: Some("PingFang SC".into()),
        ui_font_size: 16,
    };
    db.save_settings(&new).unwrap();
    drop(db);
    let (db, _) = Db::open(&path).unwrap();
    assert_eq!(db.load_settings().unwrap(), new);

    // 删除被引用的供应商 → 标题供应商设置清空
    db.delete_provider(p).unwrap();
    assert_eq!(db.load_settings().unwrap().title_provider_id, None);

    // BN-013：字号超出 12–18 时钳制；字体可恢复为“自动”
    for (stored, expect) in [(30u8, UI_FONT_SIZE_MAX), (5, UI_FONT_SIZE_MIN)] {
        let s = Settings { ui_font: None, ui_font_size: stored, ..new.clone() };
        db.save_settings(&s).unwrap();
        let loaded = db.load_settings().unwrap();
        assert_eq!(loaded.ui_font_size, expect);
        assert_eq!(loaded.ui_font, None);
    }
}

// AC-005, AC-013, AC-014, BN-001, Q-1
#[test]
fn topics_messages_roundtrip() {
    let (_d, path) = fresh();
    let (db, _) = Db::open(&path).unwrap();
    let pid = db.add_provider("A", "https://a", KEY, ProviderKind::OpenAI).unwrap();
    let t = db.create_topic().unwrap();
    assert_eq!(t.title, "新话题");
    assert_eq!(t.title_source, TitleSource::Default);

    let sent = SentRequest {
        provider_id: pid,
        model_id: "m".into(),
        size: "1024x1024".into(),
        request_json: r#"{"model":"m","prompt":"猫","n":1}"#.into(),
    };
    let u = db.add_user_message(t.id, "猫", &sent).unwrap();
    let a = db.add_pending_assistant(t.id, u).unwrap();
    let a2 = db.add_pending_assistant(t.id, u).unwrap();
    db.finish_assistant(
        a,
        &AssistantResult {
            status: Status::Success,
            http_status: Some(200),
            response_body: Some(r#"{"data":[{"b64_json":"<base64 10 bytes -> /p.png>"}]}"#.into()),
            error: None,
            failures: vec!["下载失败：HTTP 404".into()],
            images: vec![ImageRecord {
                local_path: "/p.png".into(),
                format: "PNG".into(),
                width: Some(2),
                height: Some(3),
                source_field: "b64_json".into(),
                origin: None,
            }],
        },
    )
    .unwrap();
    db.set_topic_title(t.id, "橘猫", TitleSource::Generated, None).unwrap();
    db.set_topic_selection(t.id, pid, "m", "2048x2048").unwrap();
    drop(db);

    // 模拟重启
    let (db, _) = Db::open(&path).unwrap();
    assert_eq!(db.mark_pending_interrupted().unwrap(), 1);
    let topic = db.get_topic(t.id).unwrap().unwrap();
    assert_eq!(topic.title, "橘猫");
    assert_eq!(topic.title_source, TitleSource::Generated);
    assert_eq!(topic.last_size.as_deref(), Some("2048x2048"));

    let msgs = db.list_messages(t.id).unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(msgs[0].content, "猫");
    assert_eq!(msgs[0].request_json.as_deref(), Some(sent.request_json.as_str()));
    assert_eq!(msgs[0].model_id.as_deref(), Some("m"));
    assert_eq!(msgs[1].status, Status::Success);
    assert_eq!(msgs[1].parent_id, Some(u));
    assert_eq!(msgs[1].http_status, Some(200));
    assert_eq!(msgs[1].images.len(), 1);
    assert_eq!(msgs[1].images[0].width, Some(2));
    assert_eq!(msgs[1].failures, vec!["下载失败：HTTP 404"]);
    assert_eq!(msgs[2].id, a2);
    assert_eq!(msgs[2].status, Status::Interrupted);

    // BN-001：Key 只存在于 providers 表
    for m in &msgs {
        assert!(!format!("{m:?}").contains(KEY));
    }

    db.delete_topic(t.id).unwrap();
    assert!(db.get_topic(t.id).unwrap().is_none());
    assert!(db.list_messages(t.id).unwrap().is_empty());
}

#[test]
fn topics_listed_newest_first() {
    let (_d, path) = fresh();
    let (db, _) = Db::open(&path).unwrap();
    let t1 = db.create_topic().unwrap();
    let t2 = db.create_topic().unwrap();
    let ids: Vec<i64> = db.list_topics().unwrap().iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![t2.id, t1.id]);
}

// ---------- assistant-chat ----------

/// v1 时期的完整建表语句（冻结的历史 schema，用于迁移测试）。
const SCHEMA_V1: &str = r#"
CREATE TABLE providers (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, base_url TEXT NOT NULL, api_key TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, sort INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE provider_models (id INTEGER PRIMARY KEY AUTOINCREMENT, provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE, model_id TEXT NOT NULL, created_at TEXT NOT NULL, UNIQUE(provider_id, model_id));
CREATE TABLE preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE topics (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, title_source TEXT NOT NULL DEFAULT 'default', title_error TEXT, last_provider_id INTEGER, last_model_id TEXT, last_size TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE, parent_id INTEGER, role TEXT NOT NULL, content TEXT NOT NULL, provider_id INTEGER, model_id TEXT, size TEXT, request_json TEXT, status TEXT NOT NULL, http_status INTEGER, response_body TEXT, error TEXT, failures TEXT, created_at TEXT NOT NULL);
CREATE INDEX idx_messages_topic ON messages(topic_id, id);
CREATE TABLE message_images (id INTEGER PRIMARY KEY AUTOINCREMENT, message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE, local_path TEXT NOT NULL, format TEXT NOT NULL, width INTEGER, height INTEGER, source TEXT NOT NULL, origin TEXT, created_at TEXT NOT NULL);
CREATE INDEX idx_images_message ON message_images(message_id);
"#;

// assistant-chat AC-002, AC-004, BN-005：v1 库迁移后数据保留，供应商类型为 OpenAI，存在 default 助手
#[test]
fn migrate_v1_to_v2_keeps_data_and_defaults_kind() {
    let (_d, path) = fresh();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(SCHEMA_V1).unwrap();
        c.execute_batch(
            "INSERT INTO providers (name, base_url, api_key, created_at, updated_at) VALUES ('旧网关', 'https://old', 'k', 't', 't');
             INSERT INTO topics (title, created_at, updated_at) VALUES ('旧话题', 't', 't');
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let (db, _) = Db::open(&path).unwrap();
    let ps = db.list_providers().unwrap();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].name, "旧网关");
    assert_eq!(ps[0].api_key, "k");
    assert_eq!(ps[0].kind, ProviderKind::OpenAI);
    assert_eq!(db.list_topics().unwrap()[0].title, "旧话题");
    let assistants = db.list_assistants().unwrap();
    assert_eq!(assistants.len(), 1);
    assert_eq!(assistants[0].name, "default");
    assert_eq!(assistants[0].system_prompt, "");
    assert!(assistants[0].is_default);
}

// assistant-chat AC-002
#[test]
fn provider_kind_roundtrip() {
    let (_d, path) = fresh();
    let (db, _) = Db::open(&path).unwrap();
    let id = db.add_provider("C", "https://c", KEY, ProviderKind::Anthropic).unwrap();
    assert_eq!(db.list_providers().unwrap()[0].kind, ProviderKind::Anthropic);
    let mut p = db.list_providers().unwrap().remove(0);
    p.kind = ProviderKind::OpenAI;
    db.update_provider(&p).unwrap();
    drop(db);
    let (db, _) = Db::open(&path).unwrap();
    let p = db.list_providers().unwrap().into_iter().find(|p| p.id == id).unwrap();
    assert_eq!(p.kind, ProviderKind::OpenAI);
}

// assistant-chat AC-004, BN-001
#[test]
fn default_assistant_exists_and_undeletable() {
    let (_d, path) = fresh();
    let (db, _) = Db::open(&path).unwrap();
    let def = db.list_assistants().unwrap().remove(0);
    assert!(def.is_default);
    assert!(db.delete_assistant(def.id).is_err(), "default 助手不能删除");
    assert_eq!(db.list_assistants().unwrap().len(), 1);

    let id = db.add_assistant("翻译", "你是翻译").unwrap();
    let mut a = db.list_assistants().unwrap().into_iter().find(|a| a.id == id).unwrap();
    assert!(!a.is_default);
    a.name = "翻译2".into();
    a.system_prompt = "translate".into();
    db.update_assistant(&a).unwrap();
    let a2 = db.list_assistants().unwrap().into_iter().find(|a| a.id == id).unwrap();
    assert_eq!((a2.name.as_str(), a2.system_prompt.as_str()), ("翻译2", "translate"));
    assert!(db.add_assistant("  ", "").is_err(), "名称必填");
    // 重开不重复插入 default
    drop(db);
    let (db, _) = Db::open(&path).unwrap();
    assert_eq!(db.list_assistants().unwrap().iter().filter(|a| a.is_default).count(), 1);
}

// assistant-chat AC-009, AC-012
#[test]
fn conversations_messages_and_cascade() {
    let (_d, path) = fresh();
    let (db, _) = Db::open(&path).unwrap();
    let pid = db.add_provider("A", "https://a", KEY, ProviderKind::OpenAI).unwrap();
    let aid = db.add_assistant("助手", "sys").unwrap();
    let c = db.create_conversation(aid).unwrap();
    assert_eq!(c.assistant_id, aid);
    assert_eq!(c.title_source, TitleSource::Default);
    let u = db.add_chat_user(c.id, "你好", pid, "m", r#"{"model":"m"}"#).unwrap();
    let a = db.add_chat_pending(c.id).unwrap();
    let a2 = db.add_chat_pending(c.id).unwrap();
    db.finish_chat(a, Status::Success, "你好呀", Some(200), None).unwrap();
    db.set_conversation_model(c.id, pid, "m").unwrap();
    db.set_conversation_title(c.id, "问候", TitleSource::Generated, None).unwrap();
    drop(db);

    let (db, _) = Db::open(&path).unwrap();
    assert!(db.mark_pending_interrupted().unwrap() >= 1);
    let conv = db.get_conversation(c.id).unwrap().unwrap();
    assert_eq!(conv.title, "问候");
    assert_eq!(conv.last_model_id.as_deref(), Some("m"));
    let msgs = db.list_chat_messages(c.id).unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!((msgs[0].id, msgs[0].role, msgs[0].content.as_str()), (u, Role::User, "你好"));
    assert_eq!(msgs[0].request_json.as_deref(), Some(r#"{"model":"m"}"#));
    assert_eq!((msgs[1].status, msgs[1].content.as_str()), (Status::Success, "你好呀"));
    assert_eq!((msgs[2].id, msgs[2].status), (a2, Status::Interrupted));
    assert_eq!(db.list_conversations(aid).unwrap().len(), 1);

    db.delete_assistant(aid).unwrap();
    assert!(db.get_conversation(c.id).unwrap().is_none(), "删除助手级联删除对话");
    assert!(db.list_chat_messages(c.id).unwrap().is_empty());
}
