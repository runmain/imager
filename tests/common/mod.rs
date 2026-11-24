//! 本地 Mock 网关：只监听 127.0.0.1，按路径返回预设响应，并记录收到的请求。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    /// 全部请求头，键为小写。
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, body: &str) -> Reply {
        Reply { status, content_type: "application/json", body: body.as_bytes().to_vec() }
    }
    /// SSE 流：每个 `(event, data)` 输出为 `event:` + `data:` + 空行。
    pub fn sse(events: &[(&str, &str)]) -> Reply {
        let body: String = events.iter().map(|(e, d)| format!("event: {e}\ndata: {d}\n\n")).collect();
        Reply { status: 200, content_type: "text/event-stream", body: body.into_bytes() }
    }
    pub fn bytes(content_type: &'static str, body: Vec<u8>) -> Reply {
        Reply { status: 200, content_type, body }
    }
}

pub struct MockServer {
    pub base: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
}

impl MockServer {
    pub fn start(routes: Vec<(&str, Reply)>) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: HashMap<String, Reply> =
            routes.into_iter().map(|(p, r)| (p.to_string(), r)).collect();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let rec = requests.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let mut len = 0usize;
                let mut auth = None;
                let mut headers = HashMap::new();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                    if lower.starts_with("authorization:") {
                        auth = Some(h["authorization:".len()..].trim().to_string());
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                rec.lock().unwrap().push(Recorded {
                    method,
                    path: path.clone(),
                    authorization: auth,
                    headers,
                    body: String::from_utf8_lossy(&body).into_owned(),
                });
                let reply = routes
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| Reply::json(404, r#"{"error":"not found"}"#));
                let head = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    reply.content_type,
                    reply.body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&reply.body);
            }
        });
        MockServer { base, requests }
    }

    pub fn recorded(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

/// 一张 2x3 的真实 PNG。
pub fn tiny_png() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(2, 3, image::Rgb([0, 200, 100]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}
