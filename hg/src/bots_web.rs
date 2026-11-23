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
    /// the `/api/bots` body
    pub bots: String,
    /// map name -> the `/api/map/<name>` body
    pub maps: HashMap<String, String>,
}

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

/// The answer to one request line's path.
fn route(path: &str, web: &Web) -> Vec<u8> {
    let w = web.lock().unwrap();
    let json = "application/json; charset=utf-8";
    if path == "/api/bots" {
        return reply("200 OK", json, if w.bots.is_empty() { "{\"bots\":[],\"maps\":[],\"chat\":[]}" } else { &w.bots });
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
            let mut buf = [0u8; 2048];
            let mut n = 0;
            // the request line is all that matters: read until the end of the headers or the buffer is full
            while n < buf.len() {
                match tokio::time::timeout(std::time::Duration::from_secs(3), s.read(&mut buf[n..])).await {
                    Ok(Ok(k)) if k > 0 => n += k,
                    _ => break,
                }
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buf[..n]);
            let mut it = head.lines().next().unwrap_or("").split_whitespace();
            let out = match (it.next(), it.next()) {
                (Some("GET"), Some(p)) => route(p.split('?').next().unwrap_or(p), &web),
                _ => reply("405 Method Not Allowed", "text/plain", "GET only"),
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
    fn routes() {
        let web: Web = Default::default();
        web.lock().unwrap().maps.insert("default".into(), "{\"name\":\"default\"}".into());
        let s = |p: &str| String::from_utf8(route(p, &web)).unwrap();
        assert!(s("/api/bots").starts_with("HTTP/1.1 200") && s("/api/bots").ends_with("\"chat\":[]}"));
        assert!(s("/api/map/default").ends_with("{\"name\":\"default\"}"));
        assert!(s("/api/map/nowhere").starts_with("HTTP/1.1 404") && s("/x").starts_with("HTTP/1.1 404"));
    }
}
