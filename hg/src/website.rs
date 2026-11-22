//! UpdateWebsiteStats / OnWebSocketEvent (HG.cpp, 1-minute timer): when GServer.cfg names a website, its script address
//! and a port, the online count is POSTed to it. The C++ opened a non-blocking socket per minute, wrote the request once
//! connected and closed the socket without reading the answer; here a short-lived thread does the same, so the game tick
//! never waits for the connection.
use crate::game::Game;
use crate::log::log;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::thread::JoinHandle;
use std::time::Duration;

/// The request of OnWebSocketEvent (the secret `code` is the C++'s own).
pub fn build_request(host: &[u8], script: &[u8], online: i32) -> Vec<u8> {
    let data = format!("code=L58sk2hK29dks&online={online}");
    let mut r = b"POST ".to_vec();
    r.extend_from_slice(script);
    r.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    r.extend_from_slice(host);
    r.extend_from_slice(format!("\r\nConnection: close\r\nContent-type: application/x-www-form-urlencoded\r\nContent-length: {}\r\n\r\n{data}", data.len()).as_bytes());
    r
}

/// Connect to `host:port` and write `req`; the error says what failed.
pub fn send(host: &str, port: u16, req: &[u8]) -> std::io::Result<()> {
    let timeout = Duration::from_secs(5);
    let addr = (host, port).to_socket_addrs()?.next().ok_or_else(|| std::io::Error::other("the address does not resolve"))?;
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_write_timeout(Some(timeout))?;
    s.write_all(req)
}

/// One POST in its own thread (failures are logged there).
pub fn spawn_post(host: Vec<u8>, script: Vec<u8>, port: u16, online: i32) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let h = String::from_utf8_lossy(&host).to_string();
        if let Err(e) = send(&h, port, &build_request(&host, &script, online)) {
            log(&format!("(!) Website stats: POST to {h}:{port} failed: {e}"));
        }
    })
}

impl Game {
    /// `UpdateWebsiteStats`: nothing unless the address, the script and the port are all set.
    pub fn update_website_stats(&mut self) {
        let c = &self.cfg;
        if c.website_script_addr.is_empty() || c.website_addr.is_empty() || c.website_port == 0 {
            return;
        }
        spawn_post(c.website_addr.clone(), c.website_script_addr.clone(), c.website_port, self.total_game_server_clients + self.online_cnt_add);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn request_bytes() {
        let r = build_request(b"host.net", b"/x.php", 17);
        assert_eq!(
            String::from_utf8(r).unwrap(),
            "POST /x.php HTTP/1.1\r\nHost: host.net\r\nConnection: close\r\nContent-type: application/x-www-form-urlencoded\r\nContent-length: 28\r\n\r\ncode=L58sk2hK29dks&online=17"
        );
    }

    /// Against a listener on this machine only.
    #[test]
    fn posts_to_a_local_listener() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = spawn_post(b"127.0.0.1".to_vec(), b"/online.php".to_vec(), port, 5);
        let (mut s, _) = l.accept().unwrap();
        let mut got = String::new();
        s.read_to_string(&mut got).unwrap();
        t.join().unwrap();
        assert!(got.starts_with("POST /online.php HTTP/1.1\r\nHost: 127.0.0.1\r\n"));
        assert!(got.ends_with("\r\n\r\ncode=L58sk2hK29dks&online=5"));
    }

    #[test]
    fn a_refused_connection_is_an_error_not_a_hang() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(send("127.0.0.1", port, b"x").is_err());
    }
}
