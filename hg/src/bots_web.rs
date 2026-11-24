//! Rust only: the read-only HTTP side of the bots (`web-port` in Bots.cfg, 127.0.0.1 only). The game loop publishes a JSON
//! snapshot of the bots every 2 s (`Game::bots_publish`) and the walkability bitmap of each map that has a bot in it; this task
//! only serves what was published, so it never touches the game state. `Client/Web/tools/serve.mjs` proxies `/bots/api/*`
//! here and serves the page `/bots` (Client/Web/tools/bots.html).
//!   GET /api/bots          the bots (level, guild, online, map, position ...), the maps that have bots and the last chat lines
//!   GET /api/map/<name>    {"name","w","h","bits"}: one bit per tile (row by row, 1 = walkable), base64, and the teleport tiles
use crate::log::log;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
pub struct Shared {
    /// the maximum of connected bots the page asked for, until the game loop takes it (`Game::bots_set_max`)
    pub set_max: Option<usize>,
    /// the `/api/bots` body
    pub bots: String,
    /// the `/api/stats` body (stats.rs)
    pub stats: String,
    /// map name -> the `/api/map/<name>` body
    pub maps: HashMap<String, String>,
}

/// The most bots the page may ask for.
pub const MAX_BOTS: usize = 200;

pub type Web = Arc<Mutex<Shared>>;

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(b.len() / 3 * 4 + 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(T[(n >> 18) as usize & 63] as char);
        o.push(T[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        o.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    o
}

fn reply(status: &str, ctype: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
}

/// `POST /api/max` with `{"max": N}` (or just N): the page sets the maximum of connected bots. The `X-Bots: 1` header is
/// required: a page of another site cannot send it without a preflight that nobody answers.
fn post(path: &str, marked: bool, body: &str, web: &Web) -> Vec<u8> {
    let json = "application/json; charset=utf-8";
    if path != "/api/max" {
        return reply("404 Not Found", json, "{\"error\":\"not found\"}");
    }
    if !marked {
        return reply("403 Forbidden", json, "{\"error\":\"missing X-Bots header\"}");
    }
    let digits: String = body.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    let Some(n) = digits.parse::<usize>().ok().filter(|_| !digits.is_empty() && digits.len() <= 6) else {
        return reply("400 Bad Request", json, "{\"error\":\"a number is expected\"}");
    };
    let n = n.min(MAX_BOTS);
    web.lock().unwrap().set_max = Some(n);
    reply("200 OK", json, &format!("{{\"ok\":true,\"max\":{n}}}"))
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim()))
}

/// The answer to one request line's path.
fn route(path: &str, web: &Web) -> Vec<u8> {
    let w = web.lock().unwrap();
    let json = "application/json; charset=utf-8";
    if path == "/api/bots" {
        return reply("200 OK", json, if w.bots.is_empty() { "{\"bots\":[],\"maps\":[],\"chat\":[]}" } else { &w.bots });
    }
    if path == "/api/stats" {
        return reply("200 OK", json, if w.stats.is_empty() { "{\"error\":\"no snapshot yet\"}" } else { &w.stats });
    }
    if let Some(name) = path.strip_prefix("/api/map/") {
        return match w.maps.get(name) {
            Some(m) => reply("200 OK", json, m),
            None => reply("404 Not Found", json, "{\"error\":\"no such map\"}"),
        };
    }
    reply("404 Not Found", json, "{\"error\":\"not found\"}")
}

pub async fn serve(port: u16, web: Web) {
    let l = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            log(&format!("(!) bots web 127.0.0.1:{port}: {e}"));
            return;
        }
    };
    log(&format!("(*) bots web on http://127.0.0.1:{port}/api/bots"));
    loop {
        let Ok((mut s, _)) = l.accept().await else { continue };
        let web = Arc::clone(&web);
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let mut n = 0;
            // read the headers, and for a POST the body they announce (a small one)
            let mut body_at = None;
            while n < buf.len() {
                match tokio::time::timeout(std::time::Duration::from_secs(3), s.read(&mut buf[n..])).await {
                    Ok(Ok(k)) if k > 0 => n += k,
                    _ => break,
                }
                if body_at.is_none() {
                    body_at = buf[..n].windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
                }
                if let Some(e) = body_at {
                    let len = header(&String::from_utf8_lossy(&buf[..e]), "content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                    if n >= e + len.min(buf.len() - e) {
                        break;
                    }
                }
            }
            let e = body_at.unwrap_or(n);
            let head = String::from_utf8_lossy(&buf[..e]).to_string();
            let body = String::from_utf8_lossy(&buf[e..n]).to_string();
            let mut it = head.lines().next().unwrap_or("").split_whitespace();
            let out = match (it.next(), it.next()) {
                (Some("GET"), Some(p)) => route(p.split('?').next().unwrap_or(p), &web),
                (Some("POST"), Some(p)) => post(p.split('?').next().unwrap_or(p), header(&head, "x-bots") == Some("1"), &body, &web),
                _ => reply("405 Method Not Allowed", "text/plain", "GET or POST only"),
            };
            let _ = s.write_all(&out).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_and_json_escapes() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(json_str("a\"b\\c\nd"), "\"a\\\"b\\\\c\\u000ad\"");
    }

    #[test]
    fn the_page_sets_the_maximum_with_a_marked_post_only() {
        let web: Web = Default::default();
        let s = |marked: bool, body: &str| String::from_utf8(post("/api/max", marked, body, &web)).unwrap();
        assert!(s(false, "{\"max\":5}").starts_with("HTTP/1.1 403"), "no X-Bots header: refused");
        assert!(web.lock().unwrap().set_max.is_none());
        assert!(s(true, "nothing").starts_with("HTTP/1.1 400") && s(true, "").starts_with("HTTP/1.1 400") && s(true, "99999999999").starts_with("HTTP/1.1 400"));
        assert!(s(true, "{\"max\": 37}").ends_with("{\"ok\":true,\"max\":37}"));
        assert_eq!(web.lock().unwrap().set_max, Some(37));
        assert!(s(true, "5000").ends_with(&format!("\"max\":{MAX_BOTS}}}")), "capped");
        assert!(String::from_utf8(post("/api/other", true, "1", &web)).unwrap().starts_with("HTTP/1.1 404"));
        assert_eq!(header("POST /x HTTP/1.1\r\nContent-Length: 12\r\nX-Bots: 1\r\n\r\n", "x-bots"), Some("1"));
    }

    #[test]
    fn routes() {
        let web: Web = Default::default();
        web.lock().unwrap().maps.insert("default".into(), "{\"name\":\"default\"}".into());
        let s = |p: &str| String::from_utf8(route(p, &web)).unwrap();
        assert!(s("/api/bots").starts_with("HTTP/1.1 200") && s("/api/bots").ends_with("\"chat\":[]}"));
        assert!(s("/api/map/default").ends_with("{\"name\":\"default\"}"));
        assert!(s("/api/map/nowhere").starts_with("HTTP/1.1 404") && s("/x").starts_with("HTTP/1.1 404"));
    }
}
