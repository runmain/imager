mod common;

use base64::Engine;
use common::{tiny_png, MockServer, Reply};
use imageclient::api::{self, GenerateRequest};
use imageclient::db::{ProviderKind, Role, Status};

const KEY: &str = "sk-test-SECRET-123";

fn req() -> GenerateRequest {
    GenerateRequest { model: "gpt-image-x".into(), prompt: "一只猫".into(), size: "1024x1024".into() }
}

fn b64_png() -> String {
    base64::engine::general_purpose::STANDARD.encode(tiny_png())
}

// AC-003, BN-003
#[test]
fn list_models_parses_ids_and_sends_bearer() {
    let srv = MockServer::start(vec![(
        "/v1/models",
        Reply::json(200, r#"{"data":[{"id":"gpt-image-1"},{"id":"gpt-4o"}]}"#),
    )]);
    let models = api::list_models(ProviderKind::OpenAI, &format!("{}/v1/", srv.base), KEY).unwrap();
    assert_eq!(models, vec!["gpt-image-1", "gpt-4o"]);
    let r = &srv.recorded()[0];
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/v1/models");
    assert_eq!(r.authorization.as_deref(), Some(format!("Bearer {KEY}").as_str()));
}

// AC-009, BN-001
#[test]
fn list_models_error_shows_status_without_key() {
    let srv = MockServer::start(vec![("/models", Reply::json(401, r#"{"error":"bad key"}"#))]);
    let err = api::list_models(ProviderKind::OpenAI, &srv.base, KEY).unwrap_err();
    assert!(err.contains("401"), "{err}");
    assert!(err.contains("bad key"), "{err}");
    assert!(!err.contains(KEY));
}

// AC-006, AC-007, AC-013, Q-4
#[test]
fn generation_b64_saved_and_body_redacted() {
    let b64 = b64_png();
    let srv = MockServer::start(vec![(
        "/v1/images/generations",
        Reply::json(200, &format!(r#"{{"created":1,"data":[{{"b64_json":"{b64}"}}]}}"#)),
    )]);
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images(&format!("{}/v1", srv.base), KEY, &req(), dir.path());

    let r = &srv.recorded()[0];
    assert_eq!(r.method, "POST");
    assert_eq!(r.path, "/v1/images/generations");
    let sent: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(sent["model"], "gpt-image-x");
    assert_eq!(sent["prompt"], "一只猫");
    assert_eq!(sent["n"], 1);
    assert_eq!(sent["size"], "1024x1024");
    assert_eq!(sent["response_format"], "b64_json");

    assert_eq!(out.http_status, Some(200));
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.images.len(), 1);
    let img = &out.images[0];
    assert!(img.path.starts_with(dir.path()));
    assert_eq!(img.path.extension().unwrap(), "png");
    assert_eq!(std::fs::read(&img.path).unwrap(), tiny_png());
    assert_eq!((img.width, img.height), (Some(2), Some(3)));
    assert_eq!(img.source_field, "b64_json");

    let body = out.response_body.unwrap();
    assert!(!body.contains(&b64), "base64 must be replaced in stored body");
    assert!(body.contains("\"created\""));
    assert!(body.contains(&img.path.display().to_string()));
}

// AC-007: url 与 response_url 下载
#[test]
fn generation_downloads_url_and_response_url() {
    let png = tiny_png();
    let files = MockServer::start(vec![
        ("/files/a.png", Reply::bytes("image/png", png.clone())),
        ("/files/b", Reply::bytes("application/octet-stream", png.clone())),
    ]);
    let body = format!(
        r#"{{"data":[{{"url":"{0}/files/a.png"}},{{"response_url":"{0}/files/b"}}]}}"#,
        files.base
    );
    let gw = MockServer::start(vec![("/images/generations", Reply::json(200, &body))]);
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images(&gw.base, KEY, &req(), dir.path());
    assert_eq!(out.images.len(), 2, "{:?}", out);
    assert_eq!(out.images[0].source_field, "url");
    assert_eq!(out.images[0].origin.as_deref(), Some(format!("{}/files/a.png", files.base).as_str()));
    assert_eq!(out.images[1].source_field, "response_url");
    assert_ne!(out.images[0].path, out.images[1].path, "BN-002: no overwrite");
    for img in &out.images {
        assert_eq!(std::fs::read(&img.path).unwrap(), png);
    }
    assert_eq!(out.response_body.as_deref(), Some(body.as_str()));
}

#[test]
fn generation_response_url_only() {
    let png = tiny_png();
    let files = MockServer::start(vec![("/r/1", Reply::bytes("application/octet-stream", png.clone()))]);
    let gw = MockServer::start(vec![(
        "/images/generations",
        Reply::json(200, &format!(r#"{{"data":[{{"response_url":"{}/r/1"}}]}}"#, files.base)),
    )]);
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images(&gw.base, KEY, &req(), dir.path());
    assert_eq!(out.images.len(), 1, "{:?}", out);
    assert_eq!(out.images[0].source_field, "response_url");
}

// AC-019, BN-011: 链接返回 HTML，不能存成图片，失败原因可见，原文保留
#[test]
fn html_link_rejected_and_plaintext_kept() {
    let files = MockServer::start(vec![(
        "/x.png",
        Reply { status: 200, content_type: "text/html", body: b"<!DOCTYPE html><html></html>".to_vec() },
    )]);
    let raw = format!(r#"{{"data":[{{"url":"{}/x.png"}}]}}"#, files.base);
    let gw = MockServer::start(vec![("/images/generations", Reply::json(200, &raw))]);
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images(&gw.base, KEY, &req(), dir.path());
    assert!(out.images.is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "no file written");
    assert_eq!(out.failures.len(), 1);
    assert!(out.failures[0].contains("不是图片"), "{:?}", out.failures);
    assert_eq!(out.response_body.as_deref(), Some(raw.as_str()));
}

// AC-019: 无任何候选
#[test]
fn no_candidates_returns_plaintext() {
    let gw = MockServer::start(vec![("/images/generations", Reply::json(200, r#"{"result":"ok"}"#))]);
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images(&gw.base, KEY, &req(), dir.path());
    assert!(out.images.is_empty());
    assert_eq!(out.response_body.as_deref(), Some(r#"{"result":"ok"}"#));
    assert!(out.error.as_deref().unwrap_or("").contains("没有"), "{:?}", out.error);
}

// 部分成功
#[test]
fn partial_success_lists_failures() {
    let png = tiny_png();
    let files = MockServer::start(vec![("/ok.png", Reply::bytes("image/png", png))]);
    let raw = format!(
        r#"{{"data":[{{"url":"{0}/ok.png"}},{{"url":"{0}/missing.png"}}]}}"#,
        files.base
    );
    let gw = MockServer::start(vec![("/images/generations", Reply::json(200, &raw))]);
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images(&gw.base, KEY, &req(), dir.path());
    assert_eq!(out.images.len(), 1);
    assert_eq!(out.failures.len(), 1);
    assert!(out.failures[0].contains("404"), "{:?}", out.failures);
}

// AC-009, BN-001: 非 2xx
#[test]
fn http_error_kept_without_key() {
    let gw = MockServer::start(vec![(
        "/images/generations",
        Reply { status: 502, content_type: "text/html", body: b"<!DOCTYPE html>bad gateway".to_vec() },
    )]);
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images(&gw.base, KEY, &req(), dir.path());
    assert_eq!(out.http_status, Some(502));
    assert!(out.images.is_empty());
    let err = out.error.unwrap();
    assert!(err.contains("HTTP 502"), "{err}");
    assert!(!err.contains(KEY));
    assert_eq!(out.response_body.as_deref(), Some("<!DOCTYPE html>bad gateway"));
}

// 网络错误不得含 Key
#[test]
fn network_error_without_key() {
    let dir = tempfile::tempdir().unwrap();
    let out = api::generate_images("http://127.0.0.1:1", KEY, &req(), dir.path());
    let err = out.error.unwrap();
    assert!(!err.is_empty());
    assert!(!err.contains(KEY));
    assert!(out.http_status.is_none());
}

// AC-016
#[test]
fn title_via_responses_output_array() {
    let gw = MockServer::start(vec![(
        "/v1/responses",
        Reply::json(
            200,
            r#"{"output":[{"type":"reasoning"},{"type":"message","content":[{"type":"output_text","text":"橘猫晒太阳"}]}]}"#,
        ),
    )]);
    let t = api::generate_title(ProviderKind::OpenAI, &format!("{}/v1/", gw.base), KEY, "gpt-4o-mini", "输入文本").unwrap();
    assert_eq!(t, "橘猫晒太阳");
    let r = &gw.recorded()[0];
    assert_eq!(r.path, "/v1/responses");
    let sent: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(sent["model"], "gpt-4o-mini");
    assert_eq!(sent["input"], "输入文本");
    assert_eq!(r.authorization.as_deref(), Some(format!("Bearer {KEY}").as_str()));
}

#[test]
fn title_prefers_output_text_and_reports_errors() {
    assert_eq!(api::parse_responses_text(r#"{"output_text":"标题A","output":[]}"#).as_deref(), Some("标题A"));
    assert_eq!(api::parse_responses_text(r#"{"output":[]}"#), None);

    let gw = MockServer::start(vec![("/responses", Reply::json(500, r#"{"error":"boom"}"#))]);
    let err = api::generate_title(ProviderKind::OpenAI, &gw.base, KEY, "m", "x").unwrap_err();
    assert!(err.contains("500") && !err.contains(KEY), "{err}");
}

// ---------- assistant-chat ----------

fn hist(pairs: &[(Role, &str)]) -> Vec<(Role, String)> {
    pairs.iter().map(|(r, t)| (*r, t.to_string())).collect()
}

// assistant-chat AC-003
#[test]
fn anthropic_models_uses_api_key_headers() {
    let srv = MockServer::start(vec![(
        "/v1/models",
        Reply::json(200, r#"{"data":[{"id":"claude-sonnet-4-5","type":"model"}],"has_more":false}"#),
    )]);
    let models = api::list_models(ProviderKind::Anthropic, &format!("{}/v1", srv.base), KEY).unwrap();
    assert_eq!(models, vec!["claude-sonnet-4-5"]);
    let r = &srv.recorded()[0];
    assert_eq!(r.headers.get("x-api-key").map(String::as_str), Some(KEY));
    assert_eq!(r.headers.get("anthropic-version").map(String::as_str), Some(api::ANTHROPIC_VERSION));
    assert!(r.authorization.is_none(), "Claude 不发送 Bearer");
}

// assistant-chat AC-007, BN-002
#[test]
fn anthropic_chat_headers_body_and_stream() {
    let srv = MockServer::start(vec![(
        "/v1/messages",
        Reply::sse(&[
            ("message_start", r#"{"type":"message_start","message":{"id":"m1","role":"assistant","content":[]}}"#),
            ("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
            ("ping", r#"{"type":"ping"}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你"}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"好"}}"#),
            ("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]),
    )]);
    let h = hist(&[(Role::User, "hi"), (Role::Assistant, "hello"), (Role::User, "再说一遍")]);
    let body = api::chat_body(ProviderKind::Anthropic, "claude-x", "你是助手", &h);
    let mut deltas = Vec::new();
    let out = api::stream_chat(ProviderKind::Anthropic, &format!("{}/v1", srv.base), KEY, &body, &mut |d| deltas.push(d.to_string()));
    assert_eq!(out.error, None);
    assert_eq!(out.http_status, Some(200));
    assert_eq!(out.text, "你好");
    assert_eq!(deltas, vec!["你", "好"]);

    let r = &srv.recorded()[0];
    assert_eq!(r.path, "/v1/messages");
    assert_eq!(r.headers.get("x-api-key").map(String::as_str), Some(KEY));
    assert_eq!(r.headers.get("anthropic-version").map(String::as_str), Some(api::ANTHROPIC_VERSION));
    let sent: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(sent["model"], "claude-x");
    assert_eq!(sent["system"], "你是助手");
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["max_tokens"], api::ANTHROPIC_MAX_TOKENS);
    assert_eq!(
        sent["messages"],
        serde_json::json!([
            {"role":"user","content":[{"type":"text","text":"hi"}]},
            {"role":"assistant","content":[{"type":"text","text":"hello"}]},
            {"role":"user","content":[{"type":"text","text":"再说一遍"}]}
        ])
    );
    // BN-002：空系统提示词不出现 system 字段
    let empty = api::chat_body(ProviderKind::Anthropic, "claude-x", "  ", &h);
    assert!(empty.get("system").is_none());
}

// assistant-chat AC-006, AC-009, BN-002
#[test]
fn responses_chat_body_and_stream() {
    let srv = MockServer::start(vec![(
        "/responses",
        Reply::sse(&[
            ("response.created", r#"{"type":"response.created","response":{"id":"r1"}}"#),
            ("response.output_text.delta", r#"{"type":"response.output_text.delta","delta":"Hel"}"#),
            ("response.output_text.delta", r#"{"type":"response.output_text.delta","delta":"lo"}"#),
            ("response.output_text.done", r#"{"type":"response.output_text.done","text":"Hello"}"#),
            ("response.completed", r#"{"type":"response.completed","response":{"id":"r1"}}"#),
        ]),
    )]);
    let h = hist(&[(Role::User, "a"), (Role::Assistant, "b"), (Role::User, "c")]);
    let body = api::chat_body(ProviderKind::OpenAI, "gpt-x", "sys", &h);
    let out = api::stream_chat(ProviderKind::OpenAI, &srv.base, KEY, &body, &mut |_| {});
    assert_eq!((out.error, out.text.as_str()), (None, "Hello"));

    let r = &srv.recorded()[0];
    assert_eq!(r.authorization.as_deref(), Some(format!("Bearer {KEY}").as_str()));
    let sent: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(sent["model"], "gpt-x");
    assert_eq!(sent["instructions"], "sys");
    assert_eq!(sent["stream"], true);
    assert_eq!(
        sent["input"],
        serde_json::json!([
            {"role":"user","content":[{"type":"input_text","text":"a"}]},
            {"role":"assistant","content":[{"type":"output_text","text":"b"}]},
            {"role":"user","content":[{"type":"input_text","text":"c"}]}
        ])
    );
    assert!(api::chat_body(ProviderKind::OpenAI, "gpt-x", "", &h).get("instructions").is_none());
    // BN-004：请求体不含 Key
    assert!(!r.body.contains(KEY));
}

// assistant-chat BN-006：流中 error 事件与 HTTP 错误都记为失败，保留已收到的文本，错误不含 Key
#[test]
fn chat_errors_are_reported() {
    let srv = MockServer::start(vec![
        (
            "/a/messages",
            Reply::sse(&[
                ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"部分"}}"#),
                ("error", r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#),
            ]),
        ),
        ("/b/responses", Reply::json(401, r#"{"error":{"message":"bad key"}}"#)),
        (
            "/c/responses",
            Reply::sse(&[("response.failed", r#"{"type":"response.failed","response":{"error":{"code":"server_error","message":"boom"}}}"#)]),
        ),
    ]);
    let h = hist(&[(Role::User, "x")]);
    let a = api::stream_chat(ProviderKind::Anthropic, &format!("{}/a", srv.base), KEY, &api::chat_body(ProviderKind::Anthropic, "m", "", &h), &mut |_| {});
    assert_eq!(a.text, "部分");
    assert!(a.error.as_deref().unwrap().contains("Overloaded"), "{:?}", a.error);

    let b = api::stream_chat(ProviderKind::OpenAI, &format!("{}/b", srv.base), KEY, &api::chat_body(ProviderKind::OpenAI, "m", "", &h), &mut |_| {});
    assert_eq!(b.http_status, Some(401));
    let err = b.error.unwrap();
    assert!(err.contains("401") && err.contains("bad key") && !err.contains(KEY), "{err}");

    let c = api::stream_chat(ProviderKind::OpenAI, &format!("{}/c", srv.base), KEY, &api::chat_body(ProviderKind::OpenAI, "m", "", &h), &mut |_| {});
    assert!(c.error.as_deref().unwrap().contains("boom"), "{:?}", c.error);

    let net = api::stream_chat(ProviderKind::OpenAI, "http://127.0.0.1:1", KEY, &api::chat_body(ProviderKind::OpenAI, "m", "", &h), &mut |_| {});
    assert!(net.error.is_some() && !net.error.unwrap().contains(KEY));
}

// assistant-chat：网关忽略 stream 返回普通 JSON 时仍能取到文本
#[test]
fn chat_non_stream_fallback() {
    let srv = MockServer::start(vec![
        ("/a/messages", Reply::json(200, r#"{"content":[{"type":"text","text":"整段"}],"stop_reason":"end_turn"}"#)),
        ("/o/responses", Reply::json(200, r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"整段O"}]}]}"#)),
    ]);
    let h = hist(&[(Role::User, "x")]);
    let a = api::stream_chat(ProviderKind::Anthropic, &format!("{}/a", srv.base), KEY, &api::chat_body(ProviderKind::Anthropic, "m", "", &h), &mut |_| {});
    assert_eq!((a.error, a.text.as_str()), (None, "整段"));
    let o = api::stream_chat(ProviderKind::OpenAI, &format!("{}/o", srv.base), KEY, &api::chat_body(ProviderKind::OpenAI, "m", "", &h), &mut |_| {});
    assert_eq!((o.error, o.text.as_str()), (None, "整段O"));
}

// assistant-chat AC-013
#[test]
fn title_uses_messages_for_anthropic() {
    let gw = MockServer::start(vec![("/messages", Reply::json(200, r#"{"content":[{"type":"text","text":"晒太阳的猫"}]}"#))]);
    let t = api::generate_title(ProviderKind::Anthropic, &gw.base, KEY, "claude-x", "输入").unwrap();
    assert_eq!(t, "晒太阳的猫");
    let r = &gw.recorded()[0];
    assert_eq!(r.headers.get("x-api-key").map(String::as_str), Some(KEY));
    let sent: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(sent["messages"], serde_json::json!([{"role":"user","content":[{"type":"text","text":"输入"}]}]));
    assert!(sent["max_tokens"].is_number());
}

// assistant-chat AC-008, BN-003
#[test]
fn history_alternates_and_skips_failed() {
    use Role::*;
    use Status::*;
    let stored = [
        (User, "q1", Success),
        (Assistant, "a1", Success),
        (User, "q2", Success),
        (Assistant, "部分", Error), // 失败轮次整体跳过
        (User, "q3", Success),
        (Assistant, "", Interrupted),
        (User, "q4", Success),
        (Assistant, "a4", Success),
        (User, "孤立", Success), // 没有回复的用户消息跳过
    ];
    let h = api::build_history(&stored, "q5");
    assert_eq!(h, hist(&[(User, "q1"), (Assistant, "a1"), (User, "q4"), (Assistant, "a4"), (User, "q5")]));
    assert_eq!(api::build_history(&[], "only"), hist(&[(User, "only")]));
    for w in h.windows(2) {
        assert_ne!(w[0].0, w[1].0, "角色必须交替");
    }
}
