//! 从上游响应体中提取图片来源候选（base64 或链接）。
//!
//! 顺序：`data[]` 已知字段 → 递归扫描整个 JSON → 非 JSON 文本中的 data URI / Markdown 图片 / 链接。

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    Base64,
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 来源字段名（如 `b64_json`、`response_url`），纯文本提取时为 `text`。
    pub field: String,
    pub kind: SourceKind,
    /// 原始字符串值（base64 可能带 `data:` 前缀）。
    pub value: String,
}

const B64_FIELDS: &[&str] = &["b64_json", "b64", "base64", "image_base64"];
const URL_FIELDS: &[&str] = &["url", "response_url", "image_url"];

static DATA_URI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"data:image/[A-Za-z0-9.+-]+;base64,[A-Za-z0-9+/=_-]+").unwrap());
static MD_IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\((https?://[^)\s]+)\)").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[^\s"'<>()\[\]]+"#).unwrap());

pub fn extract_candidates(body: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    match serde_json::from_str::<Value>(body) {
        Ok(json) => {
            if let Some(items) = json.get("data").and_then(Value::as_array) {
                for item in items {
                    if let Some(c) = from_data_item(item) {
                        push_unique(&mut out, c);
                    }
                }
            }
            if out.is_empty() {
                scan_json("", &json, &mut out);
            }
        }
        Err(_) => scan_text(body, &mut out),
    }
    out
}

fn push_unique(out: &mut Vec<Candidate>, c: Candidate) {
    if !out.iter().any(|o| o.kind == c.kind && o.value == c.value) {
        out.push(c);
    }
}

fn is_http(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn is_data_image(s: &str) -> bool {
    s.trim_start().starts_with("data:image/")
}

/// 按值判断类型：data URI 一律按 base64 处理；`as_url` 字段只接受 http(s)。
fn classify(field: &str, value: &str, as_url: bool) -> Option<Candidate> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let kind = if is_data_image(value) {
        SourceKind::Base64
    } else if as_url {
        if !is_http(value) {
            return None;
        }
        SourceKind::Url
    } else {
        SourceKind::Base64
    };
    Some(Candidate { field: field.to_string(), kind, value: value.to_string() })
}

fn from_data_item(item: &Value) -> Option<Candidate> {
    let obj = item.as_object()?;
    let b64 = B64_FIELDS.iter().find_map(|f| {
        obj.get(*f).and_then(Value::as_str).and_then(|v| classify(f, v, false))
    });
    b64.or_else(|| {
        URL_FIELDS.iter().find_map(|f| {
            let v = obj.get(*f)?;
            let s = v.as_str().or_else(|| v.get("url").and_then(Value::as_str))?;
            classify(f, s, true)
        })
    })
}

fn scan_json(key: &str, v: &Value, out: &mut Vec<Candidate>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                scan_json(k, child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                scan_json(key, child, out);
            }
        }
        Value::String(s) => {
            let lower = key.to_ascii_lowercase();
            if lower.contains("b64") || lower.contains("base64") {
                if let Some(c) = classify(key, s, false) {
                    push_unique(out, c);
                }
            } else if lower.contains("url") && is_http(s.trim()) {
                if let Some(c) = classify(key, s, true) {
                    push_unique(out, c);
                }
            } else {
                for m in DATA_URI.find_iter(s) {
                    push_unique(
                        out,
                        Candidate { field: key.to_string(), kind: SourceKind::Base64, value: m.as_str().into() },
                    );
                }
            }
        }
        _ => {}
    }
}

fn scan_text(body: &str, out: &mut Vec<Candidate>) {
    let text = |kind, value: &str| Candidate { field: "text".into(), kind, value: value.to_string() };
    for m in DATA_URI.find_iter(body) {
        push_unique(out, text(SourceKind::Base64, m.as_str()));
    }
    for cap in MD_IMAGE.captures_iter(body) {
        push_unique(out, text(SourceKind::Url, &cap[1]));
    }
    for m in LINK.find_iter(body) {
        let link = m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?', '。', '，']);
        push_unique(out, text(SourceKind::Url, link));
    }
}

/// 把响应体中的字符串值里出现的 `from` 替换为 `to`；JSON 按字符串值替换并保持字段顺序，非 JSON 按文本替换。
pub fn redact(body: &str, replacements: &[(String, String)]) -> String {
    if replacements.is_empty() {
        return body.to_string();
    }
    match serde_json::from_str::<Value>(body) {
        Ok(mut json) => {
            redact_value(&mut json, replacements);
            serde_json::to_string(&json).unwrap_or_else(|_| body.to_string())
        }
        Err(_) => replacements.iter().fold(body.to_string(), |acc, (from, to)| acc.replace(from, to)),
    }
}

fn redact_value(v: &mut Value, replacements: &[(String, String)]) {
    match v {
        Value::Object(map) => map.values_mut().for_each(|c| redact_value(c, replacements)),
        Value::Array(items) => items.iter_mut().for_each(|c| redact_value(c, replacements)),
        Value::String(s) => {
            for (from, to) in replacements {
                if s.contains(from.as_str()) {
                    *s = s.replace(from.as_str(), to);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(field: &str, kind: SourceKind, value: &str) -> Candidate {
        Candidate { field: field.into(), kind, value: value.into() }
    }

    // AC-007
    #[test]
    fn data_array_known_fields() {
        let body = r#"{"data":[
            {"b64_json":"AAAA","url":"http://x/ignored.png"},
            {"b64":"BBBB"},
            {"url":"http://x/a.png"},
            {"response_url":"https://x/r"},
            {"image_url":{"url":"https://x/i.png"}},
            {"url":"data:image/png;base64,CCCC"}
        ]}"#;
        assert_eq!(
            extract_candidates(body),
            vec![
                c("b64_json", SourceKind::Base64, "AAAA"),
                c("b64", SourceKind::Base64, "BBBB"),
                c("url", SourceKind::Url, "http://x/a.png"),
                c("response_url", SourceKind::Url, "https://x/r"),
                c("image_url", SourceKind::Url, "https://x/i.png"),
                c("url", SourceKind::Base64, "data:image/png;base64,CCCC"),
            ]
        );
    }

    // AC-020
    #[test]
    fn nested_json_scan_when_data_empty() {
        let body = r#"{"result":{"images":[{"image_base64":"QUJD"},{"download_url":"https://cdn/x.webp"}],
            "note":"see data:image/jpeg;base64,REVG end","homepage":"not a url"}}"#;
        let got = extract_candidates(body);
        assert!(got.contains(&c("image_base64", SourceKind::Base64, "QUJD")), "{got:?}");
        assert!(got.contains(&c("download_url", SourceKind::Url, "https://cdn/x.webp")), "{got:?}");
        assert!(got.contains(&c("note", SourceKind::Base64, "data:image/jpeg;base64,REVG")), "{got:?}");
        assert_eq!(got.len(), 3, "{got:?}");
    }

    // AC-020
    #[test]
    fn plain_text_sources() {
        let body = "done ![img](https://h/a.png) and https://h/b.jpg, data:image/png;base64,QUJD";
        let got = extract_candidates(body);
        assert_eq!(
            got,
            vec![
                c("text", SourceKind::Base64, "data:image/png;base64,QUJD"),
                c("text", SourceKind::Url, "https://h/a.png"),
                c("text", SourceKind::Url, "https://h/b.jpg"),
            ]
        );
    }

    #[test]
    fn dedupes_and_empty() {
        let body = r#"{"data":[{"url":"http://x/a.png"},{"url":"http://x/a.png"}]}"#;
        assert_eq!(extract_candidates(body).len(), 1);
        assert!(extract_candidates(r#"{"result":"ok"}"#).is_empty());
        assert!(extract_candidates("<!DOCTYPE html><html>bad gateway</html>").is_empty());
        assert!(extract_candidates("").is_empty());
    }

    // Q-4
    #[test]
    fn redact_replaces_values() {
        let body = r#"{"created":1,"data":[{"b64_json":"AAAA","revised_prompt":"p"}]}"#;
        let out = redact(body, &[("AAAA".into(), "<base64 3 bytes -> /p.png>".into())]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["data"][0]["b64_json"], "<base64 3 bytes -> /p.png>");
        assert_eq!(v["data"][0]["revised_prompt"], "p");
        assert_eq!(v["created"], 1);
        assert_eq!(redact("x AAAA y", &[("AAAA".into(), "<b>".into())]), "x <b> y");
        assert_eq!(redact(body, &[]), body);
    }
}
