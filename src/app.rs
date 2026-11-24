//! 应用状态、后台任务与业务动作；界面绘制见 `ui` 模块。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

use crate::api::{self, ChatOutcome, GenerateRequest, GenerationOutcome};
use crate::db::{
    Assistant, AssistantResult, ChatMessage, Conversation, Db, ImageRecord, Message, Provider, ProviderKind, Role,
    SentRequest, Settings, Status, Topic,
};
use crate::storage;
use crate::title::{self, TitleSource};

pub const SIZES: &[&str] = &["1024x1024", "2048x2048", "4096x4096"];
pub const DEFAULT_SIZE: &str = "1024x1024";
/// 解析形如 "1024x1024" 或 "128x128" 的尺寸字符串。
pub fn parse_size_str(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    if w > 0 && h > 0 { Some((w, h)) } else { None }
}
/// 缩略图纹理的最长边，避免 4K 图占用过多显存。
const THUMB_MAX_SIDE: u32 = 1024;
const TOAST_SECS: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 生图。
    Chat,
    /// 助手聊天。
    Talk,
    Settings,
}

/// 聊天页左栏页签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkTab {
    Assistants,
    Conversations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Preferences,
    Title,
    Providers,
    Assistants,
}

/// 模型列表拉取状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    Loading,
    Done(Vec<String>),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelsPurpose {
    Provider,
    Title,
}

pub enum Texture {
    Loading,
    Ready(egui::TextureHandle),
    Missing(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    DeleteProvider(i64),
    DeleteTopic(i64),
    DeleteConversation(i64),
    DeleteAssistant(i64),
}

/// 重命名 / 标题的作用对象：生图话题或聊天对话。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleFor {
    Topic(i64),
    Conversation(i64),
}

pub enum Job {
    Models { provider_id: i64, purpose: ModelsPurpose, result: Result<Vec<String>, String> },
    Generation { topic_id: i64, assistant_id: i64, outcome: GenerationOutcome },
    Title { target: TitleFor, force: bool, result: Result<String, String> },
    /// 聊天流式增量（`message_id` 为生成中的助手消息）。
    ChatDelta { message_id: i64, delta: String },
    ChatDone { conversation_id: i64, message_id: i64, outcome: ChatOutcome },
    Texture { path: String, result: Result<egui::ColorImage, String> },
    CopyImage { result: Result<egui::ColorImage, String> },
    FontList(Vec<String>),
    /// `name` 为 `None` 表示“自动”（系统字体），此时只加载粗体字重。
    FontLoaded { name: Option<String>, data: Option<egui::FontData>, bold: Option<egui::FontData> },
}

/// “添加供应商”窗口中的草稿。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewProvider {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub kind: ProviderKind,
}

/// 应用内图片预览状态；`scene_rect` 为预览区可视范围（滚轮缩放 / 拖动平移）。
pub struct Viewer {
    pub path: String,
    pub scene_rect: egui::Rect,
}

pub struct App {
    pub(crate) db: Db,
    pub(crate) page: Page,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) banner: Option<String>,

    pub(crate) providers: Vec<Provider>,
    pub(crate) models: HashMap<i64, Vec<String>>,
    pub(crate) settings: Settings,

    pub(crate) topics: Vec<Topic>,
    pub(crate) current_topic: Option<i64>,
    pub(crate) messages: Vec<Message>,
    pub(crate) input: String,
    pub(crate) chat_model: Option<(i64, String)>,
    pub(crate) chat_size: String,
    pub(crate) custom_width: String,
    pub(crate) custom_height: String,
    pub(crate) generating: HashSet<i64>,

    pub(crate) assistants: Vec<Assistant>,
    pub(crate) talk_tab: TalkTab,
    pub(crate) talk_assistant: Option<i64>,
    pub(crate) conversations: Vec<Conversation>,
    pub(crate) current_conversation: Option<i64>,
    pub(crate) chat_messages: Vec<ChatMessage>,
    pub(crate) talk_input: String,
    pub(crate) talk_model: Option<(i64, String)>,
    /// 生成中的对话。
    pub(crate) chatting: HashSet<i64>,
    /// 生成中助手消息已收到的文本。
    pub(crate) streaming: HashMap<i64, String>,
    /// 设置 → 助手 中选中的助手。
    pub(crate) settings_assistant: Option<i64>,
    /// 助手回复的 Markdown 解析缓存。
    pub(crate) md_cache: crate::ui::markdown::Cache,

    pub(crate) selected_provider: Option<i64>,
    pub(crate) show_key: bool,
    pub(crate) provider_fetch: HashMap<i64, Fetch>,
    pub(crate) fetch_selection: HashMap<i64, HashSet<String>>,
    pub(crate) title_fetch: HashMap<i64, Fetch>,

    pub(crate) confirm: Option<Confirm>,
    pub(crate) rename: Option<(TitleFor, String)>,
    pub(crate) new_provider: Option<NewProvider>,
    pub(crate) textures: HashMap<String, Texture>,
    pub(crate) body_cache: HashMap<i64, String>,
    /// 展开了“原始结果”的助手消息。
    pub(crate) raw_open: HashSet<i64>,
    pub(crate) viewer: Option<Viewer>,
    /// 短暂提示：文字与消失时刻（`InputState::time` 秒）。
    pub(crate) toast: Option<(String, f64)>,
    /// 可显示中文的系统字体族；`None` 表示尚未枚举或正在枚举。
    pub(crate) font_families: Option<Vec<String>>,
    font_list_requested: bool,
    /// 字体下拉中的搜索词。
    pub(crate) font_query: String,
    /// macOS 菜单栏图标；存在时关闭窗口改为隐藏到菜单栏。
    #[cfg(target_os = "macos")]
    tray: Option<crate::tray::Tray>,

    tx: Sender<Job>,
    rx: Receiver<Job>,
    ctx: egui::Context,
}

/// 默认数据库位置：`~/.config/imageclient/imageclient.db`。
/// 聊天可用的模型：排除 ID 含 image / video（不区分大小写）的生图、视频模型。
pub(crate) fn is_chat_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    !m.contains("image") && !m.contains("video")
}

pub fn default_db_path() -> PathBuf {
    storage::expand_tilde("~/.config/imageclient/imageclient.db")
}

impl App {
    /// `db` 已打开；`backup` 为损坏备份路径（若有），会在界面提示。
    pub fn new(ctx: egui::Context, db: Db, backup: Option<PathBuf>) -> App {
        crate::ui::style::apply(&ctx, crate::db::DEFAULT_UI_FONT_SIZE);
        let (tx, rx) = channel();
        let mut app = App {
            db,
            page: Page::Chat,
            settings_tab: SettingsTab::Providers,
            banner: backup.map(|p| format!("数据库无法读取，已备份为 {} 并新建空库", p.display())),
            providers: Vec::new(),
            models: HashMap::new(),
            settings: Settings {
                image_dir: crate::db::DEFAULT_IMAGE_DIR.into(),
                title_provider_id: None,
                title_model_id: String::new(),
                title_prompt: title::DEFAULT_TITLE_PROMPT.into(),
                ui_font: None,
                ui_font_size: crate::db::DEFAULT_UI_FONT_SIZE,
            },
            topics: Vec::new(),
            current_topic: None,
            messages: Vec::new(),
            input: String::new(),
            chat_model: None,
            chat_size: DEFAULT_SIZE.into(),
            custom_width: "1024".into(),
            custom_height: "1024".into(),
            generating: HashSet::new(),
            assistants: Vec::new(),
            talk_tab: TalkTab::Assistants,
            talk_assistant: None,
            conversations: Vec::new(),
            current_conversation: None,
            chat_messages: Vec::new(),
            talk_input: String::new(),
            talk_model: None,
            chatting: HashSet::new(),
            streaming: HashMap::new(),
            settings_assistant: None,
            md_cache: Default::default(),
            selected_provider: None,
            show_key: false,
            provider_fetch: HashMap::new(),
            fetch_selection: HashMap::new(),
            title_fetch: HashMap::new(),
            confirm: None,
            rename: None,
            new_provider: None,
            textures: HashMap::new(),
            body_cache: HashMap::new(),
            raw_open: HashSet::new(),
            viewer: None,
            toast: None,
            #[cfg(target_os = "macos")]
            tray: None,
            font_families: None,
            font_list_requested: false,
            font_query: String::new(),
            tx,
            rx,
            ctx,
        };
        if let Err(err) = app.db.mark_pending_interrupted() {
            app.report(err);
        }
        app.reload_providers();
        match app.db.load_settings() {
            Ok(s) => app.settings = s,
            Err(err) => app.report(err),
        }
        crate::ui::style::set_font_size(&app.ctx, app.settings.ui_font_size);
        app.apply_font();
        app.reload_topics();
        let first = app.topics.first().map(|t| t.id);
        app.select_topic(first);
        app.selected_provider = app.providers.first().map(|p| p.id);
        app.reload_assistants();
        let first = app.assistants.first().map(|a| a.id);
        app.select_assistant(first);
        app.talk_tab = TalkTab::Assistants;
        app
    }

    pub(crate) fn report(&mut self, err: impl Into<String>) {
        self.banner = Some(err.into());
    }

    fn spawn(&self, work: impl FnOnce() -> Job + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    // ---------- 数据刷新 ----------

    pub(crate) fn reload_providers(&mut self) {
        match self.db.list_providers() {
            Ok(list) => self.providers = list,
            Err(err) => return self.report(err),
        }
        self.models.clear();
        for p in &self.providers {
            match self.db.provider_models(p.id) {
                Ok(m) => {
                    self.models.insert(p.id, m);
                }
                Err(err) => self.banner = Some(err),
            }
        }
    }

    pub(crate) fn reload_topics(&mut self) {
        match self.db.list_topics() {
            Ok(t) => self.topics = t,
            Err(err) => self.report(err),
        }
    }

    pub(crate) fn reload_messages(&mut self) {
        self.messages = match self.current_topic {
            Some(id) => self.db.list_messages(id).unwrap_or_else(|err| {
                self.banner = Some(err);
                Vec::new()
            }),
            None => Vec::new(),
        };
    }

    pub(crate) fn provider(&self, id: i64) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// 生图可选模型：已启用的 OpenAI 类型供应商的保留模型（Claude 不支持生图接口）。
    pub(crate) fn available_models(&self) -> Vec<(i64, String, String)> {
        self.enabled_models(|p| p.kind == ProviderKind::OpenAI)
    }

    /// 聊天可选模型：全部已启用供应商的保留模型，排除生图 / 视频模型。
    pub(crate) fn talk_models(&self) -> Vec<(i64, String, String)> {
        let mut models = self.enabled_models(|_| true);
        models.retain(|(_, _, m)| is_chat_model(m));
        models
    }

    fn enabled_models(&self, keep: impl Fn(&Provider) -> bool) -> Vec<(i64, String, String)> {
        self.providers
            .iter()
            .filter(|p| p.enabled && keep(p))
            .flat_map(|p| {
                self.models
                    .get(&p.id)
                    .into_iter()
                    .flatten()
                    .map(move |m| (p.id, p.name.clone(), m.clone()))
            })
            .collect()
    }

    /// 当前模型选择不再可用（供应商禁用/删除、模型被移除）时回退到第一个可用模型。
    pub(crate) fn ensure_chat_model(&mut self) {
        let available = self.available_models();
        let valid = self
            .chat_model
            .as_ref()
            .is_some_and(|(pid, m)| available.iter().any(|(p, _, am)| p == pid && am == m));
        if !valid {
            self.chat_model = available.first().map(|(p, _, m)| (*p, m.clone()));
        }
        self.ensure_talk_model();
    }

    /// 聊天模型失效时回退到第一个可用模型。
    pub(crate) fn ensure_talk_model(&mut self) {
        let available = self.talk_models();
        let valid = self
            .talk_model
            .as_ref()
            .is_some_and(|(pid, m)| available.iter().any(|(p, _, am)| p == pid && am == m));
        if !valid {
            self.talk_model = available.first().map(|(p, _, m)| (*p, m.clone()));
        }
    }

    pub(crate) fn select_topic(&mut self, id: Option<i64>) {
        self.current_topic = id;
        self.body_cache.clear();
        if let Some(t) = id.and_then(|id| self.topics.iter().find(|t| t.id == id)) {
            self.chat_model = t.last_provider_id.zip(t.last_model_id.clone());
            self.chat_size = t.last_size.clone().unwrap_or_else(|| DEFAULT_SIZE.into());
            self.sync_custom_size_from_chat_size();
        } else {
            self.sync_custom_size_from_chat_size();
        }
        self.ensure_chat_model();
        self.reload_messages();
    }

    pub(crate) fn sync_custom_size_from_chat_size(&mut self) {
        if let Some((w, h)) = parse_size_str(&self.chat_size) {
            self.custom_width = w.to_string();
            self.custom_height = h.to_string();
        }
    }

    pub(crate) fn apply_custom_size(&mut self) {
        let w = self.custom_width.trim().parse::<u32>().unwrap_or(1024).clamp(16, 8192);
        let h = self.custom_height.trim().parse::<u32>().unwrap_or(1024).clamp(16, 8192);
        self.custom_width = w.to_string();
        self.custom_height = h.to_string();
        self.chat_size = format!("{w}x{h}");
    }

    pub fn chat_size_str(&self) -> &str {
        &self.chat_size
    }

    pub fn custom_size_pair(&self) -> (&str, &str) {
        (&self.custom_width, &self.custom_height)
    }

    pub fn set_custom_size_for_test(&mut self, w: &str, h: &str) {
        self.custom_width = w.into();
        self.custom_height = h.into();
        self.apply_custom_size();
    }
    // ---------- 话题 ----------

    pub(crate) fn new_topic(&mut self) {
        // 如果当前已有话题为空（没有产生任何消息），直接切换并聚焦到该空话题，避免堆积多个无用空话题
        if let Some(empty_topic) = self.topics.iter().find(|t| {
            self.db.list_messages(t.id).map(|m| m.is_empty()).unwrap_or(false)
        }) {
            let id = empty_topic.id;
            self.select_topic(Some(id));
            self.page = Page::Chat;
            return;
        }

        match self.db.create_topic() {
            Ok(t) => {
                self.reload_topics();
                self.select_topic(Some(t.id));
                self.page = Page::Chat;
            }
            Err(err) => self.report(err),
        }
    }

    pub(crate) fn delete_topic(&mut self, id: i64) {
        if let Err(err) = self.db.delete_topic(id) {
            return self.report(err);
        }
        self.reload_topics();
        if self.current_topic == Some(id) {
            let first = self.topics.first().map(|t| t.id);
            self.select_topic(first);
        }
    }

    pub(crate) fn rename(&mut self, target: TitleFor, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Err(err) = self.set_title(target, name, TitleSource::Manual, None) {
            return self.report(err);
        }
        self.reload_topics();
        self.reload_conversations();
    }

    // ---------- 聊天：助手 / 对话 ----------

    pub(crate) fn reload_assistants(&mut self) {
        match self.db.list_assistants() {
            Ok(list) => self.assistants = list,
            Err(err) => self.report(err),
        }
    }

    pub(crate) fn reload_conversations(&mut self) {
        self.conversations = match self.talk_assistant {
            Some(id) => self.db.list_conversations(id).unwrap_or_else(|err| {
                self.banner = Some(err);
                Vec::new()
            }),
            None => Vec::new(),
        };
    }

    pub(crate) fn reload_chat_messages(&mut self) {
        self.chat_messages = match self.current_conversation {
            Some(id) => self.db.list_chat_messages(id).unwrap_or_else(|err| {
                self.banner = Some(err);
                Vec::new()
            }),
            None => Vec::new(),
        };
    }

    /// 选中助手并切到它的对话列表，默认打开最近的对话。
    pub(crate) fn select_assistant(&mut self, id: Option<i64>) {
        self.talk_assistant = id;
        self.talk_tab = TalkTab::Conversations;
        self.reload_conversations();
        let first = self.conversations.first().map(|c| c.id);
        self.select_conversation(first);
    }

    pub(crate) fn select_conversation(&mut self, id: Option<i64>) {
        self.current_conversation = id;
        if let Some(c) = id.and_then(|id| self.conversations.iter().find(|c| c.id == id))
            && let Some(sel) = c.last_provider_id.zip(c.last_model_id.clone())
        {
            self.talk_model = Some(sel);
        }
        self.ensure_talk_model();
        self.reload_chat_messages();
    }

    /// 新建对话；当前助手已有空对话时直接切换过去，不重复创建。
    pub(crate) fn new_conversation(&mut self) {
        let Some(assistant_id) = self.talk_assistant else { return };
        let empty = self.conversations.iter().map(|c| c.id).find(|id| self.db.conversation_is_empty(*id).unwrap_or(false));
        let id = match empty {
            Some(id) => id,
            None => match self.db.create_conversation(assistant_id) {
                Ok(c) => c.id,
                Err(err) => return self.report(err),
            },
        };
        self.reload_conversations();
        self.select_conversation(Some(id));
    }

    pub(crate) fn delete_conversation(&mut self, id: i64) {
        if let Err(err) = self.db.delete_conversation(id) {
            return self.report(err);
        }
        self.chatting.remove(&id);
        self.reload_conversations();
        if self.current_conversation == Some(id) {
            let first = self.conversations.first().map(|c| c.id);
            self.select_conversation(first);
        }
    }

    /// 设置 → 助手：新增一个助手并选中。
    pub(crate) fn add_assistant(&mut self) {
        match self.db.add_assistant("新助手", "") {
            Ok(id) => {
                self.reload_assistants();
                self.settings_assistant = Some(id);
            }
            Err(err) => self.report(err),
        }
    }

    /// 名称为空的中间编辑状态不落库。
    pub(crate) fn save_assistant(&mut self, id: i64) {
        let Some(a) = self.assistants.iter().find(|a| a.id == id).cloned() else { return };
        if a.name.trim().is_empty() {
            return;
        }
        if let Err(err) = self.db.update_assistant(&a) {
            self.report(err);
        }
    }

    pub(crate) fn delete_assistant(&mut self, id: i64) {
        if let Err(err) = self.db.delete_assistant(id) {
            return self.report(err);
        }
        self.reload_assistants();
        self.settings_assistant = None;
        if self.talk_assistant == Some(id) {
            let first = self.assistants.first().map(|a| a.id);
            self.select_assistant(first);
            self.talk_tab = TalkTab::Assistants;
        }
    }

    // ---------- 聊天：发送 ----------

    pub(crate) fn talk_send_blocker(&self) -> Option<&'static str> {
        if self.talk_model.is_none() {
            return Some("请先在设置中添加供应商并保留模型");
        }
        if self.talk_assistant.is_none() {
            return Some("请先选择助手");
        }
        if self.current_conversation.is_some_and(|c| self.chatting.contains(&c)) {
            return Some("当前对话正在生成中");
        }
        if self.talk_input.trim().is_empty() {
            return Some("请输入内容");
        }
        None
    }

    pub(crate) fn send_chat(&mut self) {
        if self.talk_send_blocker().is_some() {
            return;
        }
        let (Some((provider_id, model)), Some(assistant_id)) = (self.talk_model.clone(), self.talk_assistant) else {
            return;
        };
        let Some(p) = self.provider(provider_id).cloned() else { return self.report("供应商不存在") };
        let system = self.assistants.iter().find(|a| a.id == assistant_id).map(|a| a.system_prompt.clone()).unwrap_or_default();
        let conversation_id = match self.current_conversation {
            Some(id) => id,
            None => match self.db.create_conversation(assistant_id) {
                Ok(c) => {
                    self.current_conversation = Some(c.id);
                    c.id
                }
                Err(err) => return self.report(err),
            },
        };
        let prompt = self.talk_input.trim().to_string();
        let stored = match self.db.list_chat_messages(conversation_id) {
            Ok(m) => m,
            Err(err) => return self.report(err),
        };
        let triples: Vec<(Role, &str, Status)> = stored.iter().map(|m| (m.role, m.content.as_str(), m.status)).collect();
        let history = api::build_history(&triples, &prompt);
        let body = api::chat_body(p.kind, &model, &system, &history);
        if let Err(err) = self.db.add_chat_user(conversation_id, &prompt, provider_id, &model, &body.to_string()) {
            return self.report(err);
        }
        if let Err(err) = self.db.set_conversation_model(conversation_id, provider_id, &model) {
            self.report(err);
        }
        let message_id = match self.db.add_chat_pending(conversation_id) {
            Ok(id) => id,
            Err(err) => return self.report(err),
        };
        self.talk_input.clear();
        if p.base_url.trim().is_empty() || p.api_key.trim().is_empty() {
            let error = Some(format!("供应商「{}」缺少 API Base URL 或 API Key", p.name));
            return self.finish_chat(conversation_id, message_id, ChatOutcome { error, ..Default::default() });
        }
        self.chatting.insert(conversation_id);
        self.streaming.insert(message_id, String::new());
        self.reload_conversations();
        self.reload_chat_messages();

        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let outcome = api::stream_chat(p.kind, &p.base_url, &p.api_key, &body, &mut |d| {
                let _ = tx.send(Job::ChatDelta { message_id, delta: d.to_string() });
                ctx.request_repaint();
            });
            let _ = tx.send(Job::ChatDone { conversation_id, message_id, outcome });
            ctx.request_repaint();
        });
    }

    fn finish_chat(&mut self, conversation_id: i64, message_id: i64, outcome: ChatOutcome) {
        self.chatting.remove(&conversation_id);
        self.streaming.remove(&message_id);
        let status = if outcome.error.is_none() { Status::Success } else { Status::Error };
        if let Err(err) =
            self.db.finish_chat(message_id, status, &outcome.text, outcome.http_status, outcome.error.as_deref())
        {
            self.report(err);
        }
        self.auto_title(TitleFor::Conversation(conversation_id));
        self.reload_conversations();
        if self.current_conversation == Some(conversation_id) {
            self.reload_chat_messages();
        }
    }

    /// 测试与状态检查用：是否有聊天请求进行中。
    pub fn is_chatting(&self) -> bool {
        !self.chatting.is_empty()
    }

    pub fn chat_send_blocker_for_test(&self) -> Option<&'static str> {
        self.talk_send_blocker()
    }

    /// 测试用：把当前对话（没有时新建）标记为生成中。
    pub fn mark_chatting_for_test(&mut self) {
        if self.current_conversation.is_none() {
            self.new_conversation();
        }
        if let Some(id) = self.current_conversation {
            self.chatting.insert(id);
        }
    }

    // ---------- 生图 ----------

    /// 发送不可用时返回原因；可用时返回 `None`。
    pub(crate) fn send_blocker(&self) -> Option<&'static str> {
        if self.chat_model.is_none() {
            return Some("请先在设置中添加供应商并保留模型");
        }
        if self.current_topic.is_some_and(|t| self.generating.contains(&t)) {
            return Some("当前话题正在生成中");
        }
        if self.input.trim().is_empty() {
            return Some("请输入图片描述");
        }
        None
    }

    pub(crate) fn send(&mut self) {
        if self.send_blocker().is_some() {
            return;
        }
        let Some((provider_id, model)) = self.chat_model.clone() else { return };
        let topic_id = match self.current_topic {
            Some(id) => id,
            None => match self.db.create_topic() {
                Ok(t) => {
                    self.reload_topics();
                    self.current_topic = Some(t.id);
                    t.id
                }
                Err(err) => return self.report(err),
            },
        };
        let prompt = self.input.trim().to_string();
        let size = self.chat_size.clone();
        let req = GenerateRequest { model: model.clone(), prompt: prompt.clone(), size: size.clone() };
        let sent = SentRequest {
            provider_id,
            model_id: model.clone(),
            size: size.clone(),
            request_json: api::generation_body(&req).to_string(),
        };
        let user_id = match self.db.add_user_message(topic_id, &prompt, &sent) {
            Ok(id) => id,
            Err(err) => return self.report(err),
        };
        if let Err(err) = self.db.set_topic_selection(topic_id, provider_id, &model, &size) {
            self.report(err);
        }
        self.input.clear();
        self.start_generation(topic_id, user_id, provider_id, req);
    }

    /// 用某条用户消息的原始 prompt / 模型 / 尺寸重新生成。
    pub(crate) fn retry(&mut self, user_message_id: i64) {
        let Some(user) = self.messages.iter().find(|m| m.id == user_message_id && m.role == Role::User) else {
            return;
        };
        if self.generating.contains(&user.topic_id) {
            return self.report("当前话题正在生成中，请稍后再试");
        }
        let (Some(provider_id), Some(model)) = (user.provider_id, user.model_id.clone()) else {
            return self.report("该消息缺少模型信息，无法重试");
        };
        if self.provider(provider_id).is_none() {
            return self.report("该消息使用的供应商已被删除，无法重试");
        }
        let req = GenerateRequest {
            model,
            prompt: user.content.clone(),
            size: user.size.clone().unwrap_or_else(|| DEFAULT_SIZE.into()),
        };
        let topic_id = user.topic_id;
        self.start_generation(topic_id, user_message_id, provider_id, req);
    }

    fn start_generation(&mut self, topic_id: i64, user_id: i64, provider_id: i64, req: GenerateRequest) {
        let Some(p) = self.provider(provider_id).cloned() else {
            return self.report("供应商不存在");
        };
        let assistant_id = match self.db.add_pending_assistant(topic_id, user_id) {
            Ok(id) => id,
            Err(err) => return self.report(err),
        };
        if p.base_url.trim().is_empty() || p.api_key.trim().is_empty() {
            self.finish_generation(
                topic_id,
                assistant_id,
                GenerationOutcome {
                    error: Some(format!("供应商「{}」缺少 API Base URL 或 API Key", p.name)),
                    ..Default::default()
                },
            );
            return;
        }
        self.generating.insert(topic_id);
        self.reload_topics();
        self.reload_messages();
        let dir = storage::expand_tilde(&self.settings.image_dir);
        self.spawn(move || Job::Generation {
            topic_id,
            assistant_id,
            outcome: api::generate_images(&p.base_url, &p.api_key, &req, &dir),
        });
    }

    fn finish_generation(&mut self, topic_id: i64, assistant_id: i64, outcome: GenerationOutcome) {
        self.generating.remove(&topic_id);
        let result = AssistantResult {
            status: if outcome.images.is_empty() { Status::Error } else { Status::Success },
            http_status: outcome.http_status,
            response_body: outcome.response_body,
            error: outcome.error,
            failures: outcome.failures,
            images: outcome
                .images
                .into_iter()
                .map(|i| ImageRecord {
                    local_path: i.path.display().to_string(),
                    format: i.format,
                    width: i.width,
                    height: i.height,
                    source_field: i.source_field,
                    origin: i.origin,
                })
                .collect(),
        };
        if let Err(err) = self.db.finish_assistant(assistant_id, &result) {
            self.report(err);
        }
        self.body_cache.remove(&assistant_id);
        self.auto_title(TitleFor::Topic(topic_id));
        self.reload_topics();
        if self.current_topic == Some(topic_id) {
            self.reload_messages();
        }
    }

    // ---------- 标题 ----------

    fn first_prompt(&self, target: TitleFor) -> Option<String> {
        match target {
            TitleFor::Topic(id) => {
                self.db.list_messages(id).ok()?.into_iter().find(|m| m.role == Role::User).map(|m| m.content)
            }
            TitleFor::Conversation(id) => {
                self.db.list_chat_messages(id).ok()?.into_iter().find(|m| m.role == Role::User).map(|m| m.content)
            }
        }
    }

    /// 当前标题与来源；对象已删除时为 `None`。
    fn current_title(&self, target: TitleFor) -> Option<(String, TitleSource)> {
        match target {
            TitleFor::Topic(id) => self.db.get_topic(id).ok()?.map(|t| (t.title, t.title_source)),
            TitleFor::Conversation(id) => self.db.get_conversation(id).ok()?.map(|c| (c.title, c.title_source)),
        }
    }

    fn set_title(&self, target: TitleFor, title: &str, source: TitleSource, error: Option<&str>) -> Result<(), String> {
        match target {
            TitleFor::Topic(id) => self.db.set_topic_title(id, title, source, error),
            TitleFor::Conversation(id) => self.db.set_conversation_title(id, title, source, error),
        }
    }

    /// 首次收到返回：先写兜底标题（同时标记已触发，避免重复），再按配置调用标题接口。
    fn auto_title(&mut self, target: TitleFor) {
        let Some((_, source)) = self.current_title(target) else { return };
        if !title::should_auto_generate(source) {
            return;
        }
        let prompt = self.first_prompt(target).unwrap_or_default();
        let fallback = title::fallback_title(&prompt);
        match self.title_target() {
            Ok((p, model)) => {
                if let Err(err) = self.set_title(target, &fallback, TitleSource::Fallback, None) {
                    return self.report(err);
                }
                self.spawn_title(target, false, p, model, &prompt);
            }
            Err(reason) => {
                if let Err(err) = self.set_title(target, &fallback, TitleSource::Fallback, Some(&reason)) {
                    self.report(err);
                }
            }
        }
    }

    fn title_target(&self) -> Result<(Provider, String), String> {
        let enabled = self.settings.title_provider_id.and_then(|id| self.provider(id)).map(|p| p.enabled);
        let target = title::resolve_target(self.settings.title_provider_id, &self.settings.title_model_id, enabled)?;
        let p = self.provider(target.provider_id).cloned().ok_or("标题供应商不存在")?;
        Ok((p, target.model))
    }

    fn spawn_title(&self, target: TitleFor, force: bool, p: Provider, model: String, prompt: &str) {
        let input = title::build_input(&self.settings.title_prompt, prompt);
        self.spawn(move || Job::Title {
            target,
            force,
            result: api::generate_title(p.kind, &p.base_url, &p.api_key, &model, &input),
        });
    }

    /// 手动“重新生成标题”。
    pub(crate) fn regenerate_title(&mut self, target: TitleFor) {
        let Some(prompt) = self.first_prompt(target) else {
            return self.report("还没有消息，无法生成标题");
        };
        match self.title_target() {
            Ok((p, model)) => self.spawn_title(target, true, p, model, &prompt),
            Err(reason) => self.report(format!("无法生成标题：{reason}")),
        }
    }

    fn finish_title(&mut self, target: TitleFor, force: bool, result: Result<String, String>) {
        let Some((current, source)) = self.current_title(target) else { return };
        if !title::should_apply(source, force) {
            return;
        }
        let outcome = match result.and_then(|raw| title::clean_title(&raw).ok_or_else(|| "标题接口返回为空".into())) {
            Ok(t) => self.set_title(target, &t, TitleSource::Generated, None),
            Err(reason) => self.set_title(target, &current, source, Some(&reason)),
        };
        if let Err(err) = outcome {
            self.report(err);
        }
        self.reload_topics();
        self.reload_conversations();
    }

    // ---------- 供应商 / 设置 ----------

    /// 打开“添加供应商”窗口。
    pub(crate) fn begin_add_provider(&mut self) {
        self.new_provider = Some(NewProvider::default());
    }

    /// 提交添加窗口；名称为空等错误返回原因，由窗口展示。
    pub(crate) fn add_provider(&mut self, draft: &NewProvider) -> Result<(), String> {
        let id = self.db.add_provider(&draft.name, &draft.base_url, &draft.api_key, draft.kind)?;
        self.reload_providers();
        self.selected_provider = Some(id);
        self.show_key = false;
        Ok(())
    }

    /// 名称为空的中间编辑状态不落库，失焦后由界面恢复原名称。
    pub(crate) fn save_provider(&mut self, id: i64) {
        let Some(p) = self.provider(id).cloned() else { return };
        if p.name.trim().is_empty() {
            return;
        }
        if let Err(err) = self.db.update_provider(&p) {
            self.report(err);
        }
        self.ensure_chat_model();
    }

    /// 详情页名称被清空后失焦：从数据库恢复原名称。
    pub(crate) fn restore_provider_name(&mut self, id: i64) {
        let saved = self.db.list_providers().ok().and_then(|l| l.into_iter().find(|p| p.id == id));
        if let (Some(saved), Some(p)) = (saved, self.providers.iter_mut().find(|p| p.id == id)) {
            p.name = saved.name;
        }
    }

    pub(crate) fn delete_provider(&mut self, id: i64) {
        if let Err(err) = self.db.delete_provider(id) {
            return self.report(err);
        }
        self.provider_fetch.remove(&id);
        self.title_fetch.remove(&id);
        self.reload_providers();
        if let Ok(s) = self.db.load_settings() {
            self.settings = s;
        }
        self.selected_provider = self.providers.first().map(|p| p.id);
        self.ensure_chat_model();
    }

    pub(crate) fn fetch_models(&mut self, provider_id: i64, purpose: ModelsPurpose) {
        let Some(p) = self.provider(provider_id).cloned() else { return };
        let state = match purpose {
            ModelsPurpose::Provider => &mut self.provider_fetch,
            ModelsPurpose::Title => &mut self.title_fetch,
        };
        if p.base_url.trim().is_empty() || p.api_key.trim().is_empty() {
            state.insert(provider_id, Fetch::Failed("请先填写 API Base URL 和 API Key".into()));
            return;
        }
        state.insert(provider_id, Fetch::Loading);
        self.spawn(move || Job::Models {
            provider_id,
            purpose,
            result: api::list_models(p.kind, &p.base_url, &p.api_key),
        });
    }

    pub(crate) fn set_models(&mut self, provider_id: i64, models: Vec<String>) {
        if let Err(err) = self.db.set_provider_models(provider_id, &models) {
            return self.report(err);
        }
        self.models.insert(provider_id, models);
        self.ensure_chat_model();
    }

    pub(crate) fn save_settings(&mut self) {
        if let Err(err) = self.db.save_settings(&self.settings) {
            self.report(err);
        }
    }

    // ---------- 界面字体 / 字号 ----------

    /// 后台枚举一次可显示中文的系统字体（打开偏好设置时触发）。
    pub(crate) fn request_font_list(&mut self) {
        if !self.font_list_requested {
            self.font_list_requested = true;
            self.spawn(|| Job::FontList(crate::fonts::chinese_families()));
        }
    }

    /// 按偏好安装界面字体：“自动”立即恢复默认；字体文件（含粗体字重）在后台加载，完成后安装。
    pub(crate) fn apply_font(&mut self) {
        let name = self.settings.ui_font.clone();
        if name.is_none() {
            crate::fonts::install(&self.ctx, None, None);
        }
        self.spawn(move || {
            let data = name.as_deref().and_then(crate::fonts::load_family);
            let bold = crate::fonts::load_bold(name.as_deref());
            Job::FontLoaded { name, data, bold }
        });
    }

    pub(crate) fn set_ui_font(&mut self, font: Option<String>) {
        if self.settings.ui_font != font {
            self.settings.ui_font = font;
            self.save_settings();
            self.apply_font();
        }
    }

    pub(crate) fn set_font_size(&mut self, size: u8) {
        let size = crate::db::clamp_font_size(size);
        if self.settings.ui_font_size != size {
            self.settings.ui_font_size = size;
            crate::ui::style::set_font_size(&self.ctx, size);
            self.save_settings();
        }
    }

    // ---------- 图片纹理 ----------

    /// 消息流中的缩略图纹理（最长边 1024）。
    pub(crate) fn texture(&mut self, path: &str) -> &Texture {
        self.texture_sized(path.to_string(), path, THUMB_MAX_SIDE)
    }

    /// 查看器中的原图纹理，仅在超过显卡纹理上限时缩小。
    pub(crate) fn full_texture(&mut self, path: &str) -> &Texture {
        let max = self.ctx.input(|i| i.max_texture_side) as u32;
        self.texture_sized(format!("full:{path}"), path, max)
    }

    fn texture_sized(&mut self, key: String, path: &str, max_side: u32) -> &Texture {
        if !self.textures.contains_key(&key) {
            self.textures.insert(key.clone(), Texture::Loading);
            let (owned, k) = (path.to_string(), key.clone());
            self.spawn(move || Job::Texture { result: load_image(Path::new(&owned), max_side), path: k });
        }
        &self.textures[&key]
    }

    pub(crate) fn open_viewer(&mut self, path: &str) {
        self.viewer = Some(Viewer { path: path.to_string(), scene_rect: egui::Rect::NOTHING });
    }

    /// 创建菜单栏图标；失败只提示，不影响使用（此时关闭窗口即退出）。
    #[cfg(target_os = "macos")]
    pub fn install_tray(&mut self) {
        match crate::tray::install(&self.ctx) {
            Ok(tray) => self.tray = Some(tray),
            Err(err) => self.report(format!("菜单栏图标创建失败，关闭窗口将直接退出：{err}")),
        }
    }

    /// 有菜单栏图标时，点关闭只隐藏窗口；菜单“退出”才真正关闭。
    fn close_to_tray(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        if self.tray.is_some() {
            if crate::tray::quit_requested() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                crate::tray::set_dock_visible(false);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
    }

    /// 解码原图并写入系统剪贴板（后台解码，完成后在 UI 线程写剪贴板）。
    pub(crate) fn copy_image(&mut self, path: &str) {
        let owned = path.to_string();
        self.spawn(move || Job::CopyImage { result: load_image(Path::new(&owned), u32::MAX) });
    }

    pub(crate) fn toggle_raw(&mut self, message_id: i64) {
        if !self.raw_open.remove(&message_id) {
            self.raw_open.insert(message_id);
        }
    }

    pub(crate) fn show_toast(&mut self, text: &str) {
        let now = self.ctx.input(|i| i.time);
        self.toast = Some((text.to_string(), now + TOAST_SECS));
        self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(TOAST_SECS));
    }

    // ---------- 后台结果 ----------

    pub fn poll_jobs(&mut self) {
        while let Ok(job) = self.rx.try_recv() {
            match job {
                Job::Models { provider_id, purpose, result } => {
                    let fetch = match result {
                        Ok(list) => {
                            if purpose == ModelsPurpose::Provider {
                                let saved = self.models.get(&provider_id).cloned().unwrap_or_default();
                                self.fetch_selection.insert(provider_id, saved.into_iter().collect());
                            }
                            Fetch::Done(list)
                        }
                        Err(err) => Fetch::Failed(err),
                    };
                    match purpose {
                        ModelsPurpose::Provider => self.provider_fetch.insert(provider_id, fetch),
                        ModelsPurpose::Title => self.title_fetch.insert(provider_id, fetch),
                    };
                }
                Job::Generation { topic_id, assistant_id, outcome } => {
                    self.finish_generation(topic_id, assistant_id, outcome);
                }
                Job::Title { target, force, result } => self.finish_title(target, force, result),
                Job::ChatDelta { message_id, delta } => {
                    if let Some(text) = self.streaming.get_mut(&message_id) {
                        text.push_str(&delta);
                    }
                }
                Job::ChatDone { conversation_id, message_id, outcome } => {
                    self.finish_chat(conversation_id, message_id, outcome);
                }
                Job::Texture { path, result } => {
                    let tex = match result {
                        Ok(img) => Texture::Ready(self.ctx.load_texture(&path, img, egui::TextureOptions::LINEAR)),
                        Err(err) => Texture::Missing(err),
                    };
                    self.textures.insert(path, tex);
                }
                Job::CopyImage { result } => match result {
                    Ok(img) => {
                        self.ctx.copy_image(img);
                        self.show_toast("已复制图片");
                    }
                    Err(err) => self.report(format!("复制图片失败：{err}")),
                },
                Job::FontList(list) => self.font_families = Some(list),
                // 只应用仍是当前偏好的字体，避免快速切换时旧结果覆盖新选择
                Job::FontLoaded { name, data, bold } if self.settings.ui_font == name => {
                    if let (Some(name), None) = (&name, &data) {
                        self.report(format!("界面字体「{name}」不可用，已使用系统字体"));
                    }
                    crate::fonts::install(&self.ctx, data, bold);
                }
                Job::FontLoaded { .. } => {}
            }
        }
    }

    /// 测试与状态检查用：当前是否有生图请求进行中。
    pub fn is_generating(&self) -> bool {
        !self.generating.is_empty()
    }
}

fn load_image(path: &Path, max_side: u32) -> Result<egui::ColorImage, String> {
    if !path.exists() {
        return Err(format!("本地图片不存在：{}", path.display()));
    }
    let img = image::open(path).map_err(|err| format!("无法读取图片 {}：{err}", path.display()))?;
    let img = if img.width() > max_side || img.height() > max_side {
        img.thumbnail(max_side, max_side)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &rgba))
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_jobs();
        self.close_to_tray(ui.ctx());
        crate::ui::draw(self, ui);
    }

    /// eframe 默认清屏色接近黑色，未填充的区域会透出黑底；改为面板底色。
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_str() {
        assert_eq!(parse_size_str("1024x1024"), Some((1024, 1024)));
        assert_eq!(parse_size_str("128x128"), Some((128, 128)));
        assert_eq!(parse_size_str(" 512 x 768 "), Some((512, 768)));
        assert_eq!(parse_size_str("invalid"), None);
        assert_eq!(parse_size_str("0x1024"), None);
        assert_eq!(parse_size_str("-100x200"), None);
    }

    #[test]
    fn chat_model_filter() {
        assert!(is_chat_model("gpt-4o"));
        assert!(is_chat_model("claude-sonnet-4-5"));
        assert!(!is_chat_model("gpt-image-1"));
        assert!(!is_chat_model("Sora-Video-2"));
        assert!(!is_chat_model("IMAGEN-4"));
        assert!(!is_chat_model("veo-3-VIDEO"));
    }

    #[test]
    fn talk_models_exclude_image_video_but_image_page_keeps_them() {
        let dir = tempfile::tempdir().unwrap();
        let (db, _) = Db::open(&dir.path().join("t.db")).unwrap();
        let pid = db.add_provider("网关", "http://127.0.0.1:9", "sk-x", ProviderKind::OpenAI).unwrap();
        let models = ["gpt-image-1", "Sora-Video-2", "gpt-4o"].map(String::from);
        db.set_provider_models(pid, &models).unwrap();
        let mut app = App::new(egui::Context::default(), db, None);

        let talk: Vec<String> = app.talk_models().into_iter().map(|(_, _, m)| m).collect();
        assert_eq!(talk, ["gpt-4o"]);
        let image: Vec<String> = app.available_models().into_iter().map(|(_, _, m)| m).collect();
        assert_eq!(image, models);

        app.talk_model = Some((pid, "gpt-image-1".into()));
        app.ensure_talk_model();
        assert_eq!(app.talk_model, Some((pid, "gpt-4o".into())));
    }
}
