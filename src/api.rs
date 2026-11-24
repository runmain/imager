//! 上游 HTTP 调用：模型列表、生图、图片下载、标题与聊天（OpenAI `/responses` 或 Anthropic `/messages`）。

use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder, Response};
use serde_json::{Value, json};

use crate::db::{ProviderKind, Role, Status};
use crate::extract::{self, SourceKind};
use crate::storage;

pub const GENERATION_TIMEOUT_SECS: u64 = 300;
pub const TITLE_TIMEOUT_SECS: u64 = 60;
pub const CHAT_TIMEOUT_SECS: u64 = 300;
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic Messages 必填的输出上限：只限制单次回复长度，不影响上下文窗口；取主流模型均支持的值。
pub const ANTHROPIC_MAX_TOKENS: u32 = 8192;
const MODELS_TIMEOUT_SECS: u64 = 60;
const ERROR_BODY_CHARS: usize = 2000;
const MODELS_ERROR_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateRequest {
    pub model: String,
    pub prompt: String,
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedImage {
    pub path: PathBuf,
    pub format: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source_field: String,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GenerationOutcome {
    pub http_status: Option<u16>,
    /// 已把 base64 值替换为占位描述的响应体。
    pub response_body: Option<String>,
    pub images: Vec<SavedImage>,
    pub failures: Vec<String>,
    pub error: Option<String>,
}

/// 拼接地址：去掉 base 尾部 `/` 与 path 开头 `/`，避免出现 `//`。
pub fn join_url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim().trim_end_matches('/'), path.trim_start_matches('/'))
}

pub fn generation_body(req: &GenerateRequest) -> Value {
    json!({
        "model": req.model,
        "prompt": req.prompt,
        "n": 1,
        "size": req.size,
        "response_format": "b64_json",
    })
}

fn truncate(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().nth(max).is_some() {
        out.push('…');
    }
    out
}

fn client(timeout_secs: u64) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|err| format!("无法创建 HTTP 客户端：{err}"))
}

/// 网络层错误描述；reqwest 的错误文本只含 URL，不含请求头中的 Key。
fn net_error(err: &reqwest::Error, timeout_secs: u64) -> String {
    if err.is_timeout() {
        format!("请求超时：接口 {timeout_secs} 秒内未响应")
    } else {
        format!("网络错误：{err}")
    }
}

/// 按供应商类型附加鉴权头。
fn authorize(req: RequestBuilder, kind: ProviderKind, key: &str) -> RequestBuilder {
    match kind {
        ProviderKind::OpenAI => req.bearer_auth(key),
        ProviderKind::Anthropic => req.header("x-api-key", key).header("anthropic-version", ANTHROPIC_VERSION),
    }
}

fn read_text(resp: Response) -> Result<(u16, String), String> {
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|err| format!("读取响应失败：{err}"))?;
    Ok((status, text))
}

pub fn list_models(kind: ProviderKind, base: &str, key: &str) -> Result<Vec<String>, String> {
    let req = client(MODELS_TIMEOUT_SECS)?.get(join_url(base, "/models"));
    let resp = authorize(req, kind, key)
        .send()
        .map_err(|err| net_error(&err, MODELS_TIMEOUT_SECS))?;
    let (status, text) = read_text(resp)?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", truncate(&text, MODELS_ERROR_CHARS)));
    }
    let json: Value = serde_json::from_str(&text)
        .map_err(|_| format!("模型列表不是 JSON：{}", truncate(&text, MODELS_ERROR_CHARS)))?;
    let ids: Vec<String> = json
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).map(String::from).collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Err(format!("返回中没有模型：{}", truncate(&text, MODELS_ERROR_CHARS)));
    }
    Ok(ids)
}

fn download(client: &Client, url: &str) -> Result<(Vec<u8>, String), String> {
    let resp = client.get(url).send().map_err(|err| format!("下载失败：{} {url}", net_error(&err, GENERATION_TIMEOUT_SECS)))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("下载失败：HTTP {status} {url}"));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("未知类型")
        .to_string();
    let bytes = resp.bytes().map_err(|err| format!("下载失败：{err} {url}"))?;
    Ok((bytes.to_vec(), content_type))
}

fn dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map_or((None, None), |(w, h)| (Some(w), Some(h)))
}

/// 调用生图接口，解析全部候选并落盘；任何失败都体现在返回值中，不 panic、不吞错。
pub fn generate_images(base: &str, key: &str, req: &GenerateRequest, image_dir: &Path) -> GenerationOutcome {
    let mut out = GenerationOutcome::default();
    let http = match client(GENERATION_TIMEOUT_SECS) {
        Ok(c) => c,
        Err(err) => {
            out.error = Some(err);
            return out;
        }
    };
    let resp = match http
        .post(join_url(base, "/images/generations"))
        .bearer_auth(key)
        .json(&generation_body(req))
        .send()
    {
        Ok(r) => r,
        Err(err) => {
            out.error = Some(net_error(&err, GENERATION_TIMEOUT_SECS));
            return out;
        }
    };
    let (status, text) = match read_text(resp) {
        Ok(v) => v,
        Err(err) => {
            out.error = Some(err);
            return out;
        }
    };
    out.http_status = Some(status);
    if !(200..300).contains(&status) {
        out.error = Some(format!("HTTP {status}: {}", truncate(&text, ERROR_BODY_CHARS)));
        out.response_body = Some(text);
        return out;
    }

    let candidates = extract::extract_candidates(&text);
    let mut replacements = Vec::new();
    for cand in &candidates {
        let fetched = match cand.kind {
            SourceKind::Base64 => storage::decode_base64(&cand.value).map(|b| (b, None)),
            SourceKind::Url => download(&http, &cand.value).map(|(b, ct)| (b, Some(ct))),
        };
        let saved = fetched.and_then(|(bytes, content_type)| {
            let format = storage::detect_format(&bytes).ok_or_else(|| match &content_type {
                Some(ct) => format!("不是图片：{ct} {}", cand.value),
                None => format!("不是图片：{} 字段解码后不是 png/jpg/webp/gif", cand.field),
            })?;
            let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f").to_string();
            let path = storage::save_image(image_dir, &bytes, format, &stamp)?;
            Ok((bytes.len(), format, path, dimensions(&bytes)))
        });
        match saved {
            Ok((len, format, path, (width, height))) => {
                if cand.kind == SourceKind::Base64 {
                    replacements.push((cand.value.clone(), format!("<base64 {len} bytes -> {}>", path.display())));
                }
                out.images.push(SavedImage {
                    path,
                    format: format.name().to_string(),
                    width,
                    height,
                    source_field: cand.field.clone(),
                    origin: (cand.kind == SourceKind::Url).then(|| cand.value.clone()),
                });
            }
            Err(reason) => {
                if cand.kind == SourceKind::Base64 {
                    replacements.push((cand.value.clone(), format!("<base64 {} chars, not saved>", cand.value.len())));
                }
                out.failures.push(reason);
            }
        }
    }
    out.response_body = Some(extract::redact(&text, &replacements));
    if out.images.is_empty() {
        out.error = Some(if candidates.is_empty() {
            "返回中没有图片".to_string()
        } else {
            "返回中的图片全部获取失败".to_string()
        });
    }
    out
}

/// 解析 `/responses` 返回文本：优先顶层 `output_text`，否则拼接 `output[].content[]` 中的 `output_text`。
pub fn parse_responses_text(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    if let Some(t) = json.get("output_text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
        return Some(t.to_string());
    }
    let text: String = json
        .get("output")?
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter(|c| c.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|c| c.get("text").and_then(Value::as_str))
        .collect();
    (!text.trim().is_empty()).then_some(text)
}

/// 解析 Anthropic Messages 非流式返回：拼接 `content[]` 中 `type == "text"` 的文本。
pub fn parse_messages_text(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    let text: String = json
        .get("content")?
        .as_array()?
        .iter()
        .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|c| c.get("text").and_then(Value::as_str))
        .collect();
    (!text.trim().is_empty()).then_some(text)
}

/// 生成标题：OpenAI 走 `/responses`，Anthropic 走 `/messages`（均为非流式）。
pub fn generate_title(kind: ProviderKind, base: &str, key: &str, model: &str, input: &str) -> Result<String, String> {
    let (path, body) = match kind {
        ProviderKind::OpenAI => ("/responses", json!({ "model": model, "input": input })),
        ProviderKind::Anthropic => (
            "/messages",
            json!({
                "model": model,
                "max_tokens": 256,
                "messages": [{ "role": "user", "content": [{ "type": "text", "text": input }] }],
            }),
        ),
    };
    let req = client(TITLE_TIMEOUT_SECS)?.post(join_url(base, path)).json(&body);
    let resp = authorize(req, kind, key).send().map_err(|err| net_error(&err, TITLE_TIMEOUT_SECS))?;
    let (status, text) = read_text(resp)?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", truncate(&text, MODELS_ERROR_CHARS)));
    }
    let parsed = match kind {
        ProviderKind::OpenAI => parse_responses_text(&text),
        ProviderKind::Anthropic => parse_messages_text(&text),
    };
    parsed.ok_or_else(|| format!("标题接口返回中没有文本：{}", truncate(&text, MODELS_ERROR_CHARS)))
}

// ---------- 聊天 ----------

/// 按时间顺序的已存消息 `(角色, 内容, 状态)` 组装多轮历史，并追加本次用户输入。
///
/// 只保留“用户消息 + 紧随其后的成功助手回复”成对的轮次；失败 / 中断 / 没有回复的轮次整体跳过。
/// 结果以 user 开头、user/assistant 严格交替、以 user 结尾，满足两种协议的约束。
pub fn build_history(stored: &[(Role, &str, Status)], next_user: &str) -> Vec<(Role, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < stored.len() {
        if let (Some((Role::User, q, _)), Some((Role::Assistant, a, Status::Success))) =
            (stored.get(i), stored.get(i + 1))
        {
            out.push((Role::User, q.to_string()));
            out.push((Role::Assistant, a.to_string()));
            i += 2;
        } else {
            i += 1;
        }
    }
    out.push((Role::User, next_user.to_string()));
    out
}

/// 构造流式对话请求体。系统提示词为空时不出现 `instructions` / `system` 字段。
pub fn chat_body(kind: ProviderKind, model: &str, system: &str, history: &[(Role, String)]) -> Value {
    let system = system.trim();
    match kind {
        ProviderKind::OpenAI => {
            let input: Vec<Value> = history
                .iter()
                .map(|(role, text)| match role {
                    Role::User => json!({ "role": "user", "content": [{ "type": "input_text", "text": text }] }),
                    Role::Assistant => {
                        json!({ "role": "assistant", "content": [{ "type": "output_text", "text": text }] })
                    }
                })
                .collect();
            let mut body = json!({ "model": model, "input": input, "stream": true });
            if !system.is_empty() {
                body["instructions"] = json!(system);
            }
            body
        }
        ProviderKind::Anthropic => {
            let messages: Vec<Value> = history
                .iter()
                .map(|(role, text)| {
                    let role = if *role == Role::User { "user" } else { "assistant" };
                    json!({ "role": role, "content": [{ "type": "text", "text": text }] })
                })
                .collect();
            let mut body =
                json!({ "model": model, "max_tokens": ANTHROPIC_MAX_TOKENS, "messages": messages, "stream": true });
            if !system.is_empty() {
                body["system"] = json!(system);
            }
            body
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatOutcome {
    pub http_status: Option<u16>,
    /// 已收到的全部文本（失败时为部分文本）。
    pub text: String,
    pub error: Option<String>,
}

/// SSE 事件中的错误描述：优先 `error.message`，其次 `response.error.message`。
fn event_error(ev: &Value) -> String {
    ev.pointer("/error/message")
        .or_else(|| ev.pointer("/response/error/message"))
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| truncate(&ev.to_string(), ERROR_BODY_CHARS))
}

/// 处理一条 SSE `data` 负载；返回 `Some(Ok(()))` 表示流结束，`Some(Err)` 表示上游报错。
fn handle_event(kind: ProviderKind, data: &str, text: &mut String, on_delta: &mut dyn FnMut(&str)) -> Option<Result<(), String>> {
    let ev: Value = serde_json::from_str(data).ok()?;
    let ty = ev.get("type").and_then(Value::as_str).unwrap_or("");
    let delta = match (kind, ty) {
        (ProviderKind::OpenAI, "response.output_text.delta") => ev.get("delta").and_then(Value::as_str),
        (ProviderKind::Anthropic, "content_block_delta")
            if ev.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
        {
            ev.pointer("/delta/text").and_then(Value::as_str)
        }
        (ProviderKind::OpenAI, "response.completed") | (ProviderKind::Anthropic, "message_stop") => {
            return Some(Ok(()));
        }
        (_, "error") | (ProviderKind::OpenAI, "response.failed") => return Some(Err(event_error(&ev))),
        _ => None,
    };
    if let Some(d) = delta {
        text.push_str(d);
        on_delta(d);
    }
    None
}

/// 发送流式对话请求，逐段回调增量文本；任何失败都体现在返回值中，不 panic、不吞错。
/// 网关忽略 `stream` 返回普通 JSON 时按非流式解析。
pub fn stream_chat(
    kind: ProviderKind,
    base: &str,
    key: &str,
    body: &Value,
    on_delta: &mut dyn FnMut(&str),
) -> ChatOutcome {
    let mut out = ChatOutcome::default();
    let http = match client(CHAT_TIMEOUT_SECS) {
        Ok(c) => c,
        Err(err) => {
            out.error = Some(err);
            return out;
        }
    };
    let path = match kind {
        ProviderKind::OpenAI => "/responses",
        ProviderKind::Anthropic => "/messages",
    };
    let req = http.post(join_url(base, path)).header("accept", "text/event-stream").json(body);
    let resp = match authorize(req, kind, key).send() {
        Ok(r) => r,
        Err(err) => {
            out.error = Some(net_error(&err, CHAT_TIMEOUT_SECS));
            return out;
        }
    };
    let status = resp.status().as_u16();
    out.http_status = Some(status);
    let is_sse = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/event-stream"));
    if !(200..300).contains(&status) || !is_sse {
        let text = match resp.text() {
            Ok(t) => t,
            Err(err) => {
                out.error = Some(format!("读取响应失败：{err}"));
                return out;
            }
        };
        if !(200..300).contains(&status) {
            out.error = Some(format!("HTTP {status}: {}", truncate(&text, ERROR_BODY_CHARS)));
            return out;
        }
        let parsed = match kind {
            ProviderKind::OpenAI => parse_responses_text(&text),
            ProviderKind::Anthropic => parse_messages_text(&text),
        };
        match parsed {
            Some(t) => {
                on_delta(&t);
                out.text = t;
            }
            None => out.error = Some(format!("返回中没有文本：{}", truncate(&text, ERROR_BODY_CHARS))),
        }
        return out;
    }

    let mut finished = false;
    let mut data = String::new();
    for line in BufReader::new(resp).lines() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                out.error = Some(format!("读取流失败：{err}"));
                return out;
            }
        };
        if let Some(d) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.trim_start());
            continue;
        }
        if !line.is_empty() {
            continue;
        }
        // 空行：一个事件结束
        let payload = std::mem::take(&mut data);
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        match handle_event(kind, &payload, &mut out.text, on_delta) {
            Some(Ok(())) => finished = true,
            Some(Err(err)) => {
                out.error = Some(err);
                return out;
            }
            None => {}
        }
    }
    if !data.is_empty()
        && let Some(r) = handle_event(kind, &data, &mut out.text, on_delta)
    {
        match r {
            Ok(()) => finished = true,
            Err(err) => {
                out.error = Some(err);
                return out;
            }
        }
    }
    if out.text.is_empty() {
        out.error = Some(if finished { "返回中没有文本".into() } else { "连接提前结束，没有收到回复".into() });
    } else if !finished {
        out.error = Some("连接提前结束，回复可能不完整".into());
    }
    out
}
