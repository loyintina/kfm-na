//! tests/http1_spec.rs — A 档考题：手写 HTTP/1.1 客户端（src/http1.rs）
//!
//! 契约真相源：docs/active/ai-presence.md §四B（direct-api-brain 手写 HTTP/1.1）。
//! 判卷点：响应头解析 / chunked 分帧（SSE 的生命线）/ content-length / EOF 收尾。
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::http1::{BodyKind, BodyReader, BufIo, Request, serialize_request};
use std::io::Read;

/// 慢速喂字节器：每次 read 最多吐 n 字节，模拟网络碎包。
struct SlowReader {
    data: std::io::Cursor<Vec<u8>>,
    max: usize,
}
impl SlowReader {
    fn new(data: &str, max: usize) -> Self {
        Self {
            data: std::io::Cursor::new(data.as_bytes().to_vec()),
            max,
        }
    }
}
impl Read for SlowReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let n = out.len().min(self.max);
        self.data.read(&mut out[..n])
    }
}

fn read_all_body<R: Read>(mut br: BodyReader<R>) -> String {
    let mut body = Vec::new();
    let mut buf = [0u8; 7]; // 故意用小缓冲逼出多次 read
    loop {
        let n = br.read_body(&mut buf).expect("body 读取失败");
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    String::from_utf8(body).unwrap()
}

// ========== 请求序列化 ==========

#[test]
fn request_serialization_post_json() {
    let req = Request {
        method: "POST".into(),
        path: "/coding/v1/chat/completions".into(),
        headers: vec![
            ("Host".into(), "api.kimi.com".into()),
            ("Authorization".into(), "Bearer k".into()),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: b"{\"stream\":true}".to_vec(),
    };
    let bytes = serialize_request(&req);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("POST /coding/v1/chat/completions HTTP/1.1\r\n"));
    assert!(text.contains("Host: api.kimi.com\r\n"));
    assert!(text.contains("Authorization: Bearer k\r\n"));
    assert!(
        text.contains("Content-Length: 15\r\n"),
        "必须自动补 Content-Length"
    );
    assert!(text.ends_with("\r\n\r\n{\"stream\":true}"));
}

// ========== 响应头 ==========

#[test]
fn head_parse_basic_and_case_insensitive_lookup() {
    let raw =
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
    let mut io = BufIo::new(SlowReader::new(raw, 3));
    let head = io.read_head().expect("head 解析失败");
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-type"), Some("text/event-stream"));
    assert_eq!(head.header("TRANSFER-ENCODING"), Some("chunked"));
    assert_eq!(head.header("missing"), None);
    assert_eq!(head.body_kind(), BodyKind::Chunked);
}

#[test]
fn head_parse_error_status() {
    let raw = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 4\r\n\r\nbody";
    let mut io = BufIo::new(SlowReader::new(raw, 2));
    let head = io.read_head().unwrap();
    assert_eq!(head.status, 401);
    assert_eq!(head.body_kind(), BodyKind::Len(4));
}

// ========== chunked 分帧（SSE 的生命线：块界就是事件推送节奏） ==========

#[test]
fn chunked_body_normal() {
    let raw = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
    let mut io = BufIo::new(SlowReader::new(raw, 4));
    let head = io.read_head().unwrap();
    assert_eq!(
        read_all_body(io.body_reader(head.body_kind())),
        "hello world"
    );
}

#[test]
fn chunked_body_split_mid_frame_and_trailers() {
    // 块尺寸行/数据/CRLF 全被切散 + 结尾带 trailer 字段（必须吃掉不吐出）
    let raw = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
a\r\n0123456789\r\n1\r\nX\r\n0\r\nX-Trailer: yes\r\n\r\n";
    let mut io = BufIo::new(SlowReader::new(raw, 1)); // 逐字节喂
    let head = io.read_head().unwrap();
    assert_eq!(
        read_all_body(io.body_reader(head.body_kind())),
        "0123456789X",
        "逐字节碎喂下 chunked 重组不许错"
    );
}

#[test]
fn chunked_body_sse_like_multibyte_utf8() {
    // SSE 实景：data: 行内含多字节 UTF-8（中文跨块界切断是常态）
    let payload = "data: {\"delta\":\"你好\"}\n\n";
    let raw = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
        payload.len(),
        payload
    );
    let mut io = BufIo::new(SlowReader::new(&raw, 5));
    let head = io.read_head().unwrap();
    assert_eq!(read_all_body(io.body_reader(head.body_kind())), payload);
}

// ========== content-length / EOF 收尾 ==========

#[test]
fn content_length_body() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello worldextra";
    let mut io = BufIo::new(SlowReader::new(raw, 3));
    let head = io.read_head().unwrap();
    assert_eq!(head.body_kind(), BodyKind::Len(11));
    // 只准读 11 字节，后面的 extra 不属于 body
    assert_eq!(
        read_all_body(io.body_reader(head.body_kind())),
        "hello world"
    );
}

#[test]
fn eof_body() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nuntil close";
    let mut io = BufIo::new(SlowReader::new(raw, 4));
    let head = io.read_head().unwrap();
    assert_eq!(head.body_kind(), BodyKind::Eof);
    assert_eq!(
        read_all_body(io.body_reader(head.body_kind())),
        "until close"
    );
}
