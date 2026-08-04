//! A tiny real HTTP/1.1 server for the tests.
//!
//! Deliberately socket-level rather than mocked: the tests exercise the actual
//! reqwest client, so streaming, chunk boundaries, and header handling are real.
//! Every request is recorded so tests can assert on what postcat actually sent.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// One request as the server saw it on the wire.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    /// Full request target, including the query string.
    pub path: String,
    /// Header names are lowercased; repeated names appear more than once.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn query(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        if let Some((_, q)) = self.path.split_once('?') {
            for pair in q.split('&').filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                out.insert(decode(k), decode(v));
            }
        }
        out
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct TestServer {
    pub addr: SocketAddr,
    log: Arc<Mutex<Vec<Recorded>>>,
}

impl TestServer {
    /// Bind an ephemeral port and serve until the process exits.
    pub fn start() -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let log = Arc::new(Mutex::new(Vec::new()));
        let accept_log = Arc::clone(&log);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let log = Arc::clone(&accept_log);
                thread::spawn(move || handle(stream, log));
            }
        });
        TestServer { addr, log }
    }

    /// Routes:
    /// - anything (default) → JSON echo of method, path, headers, body
    /// - `/sse?n=20&ms=40`  → `text/event-stream`, `n` events `ms` apart
    /// - `/status/<code>`   → that status code
    /// - `/slow?ms=500`     → echo after a delay
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    /// Host:port with no scheme, for testing the `https://` auto-prefix path.
    pub fn hostport(&self) -> String {
        self.addr.to_string()
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.log.lock().unwrap().clone()
    }

    pub fn request_count(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    /// The most recent request; panics if none arrived.
    pub fn last(&self) -> Recorded {
        self.log
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("expected the server to have received a request")
    }
}

fn handle(stream: TcpStream, log: Arc<Mutex<Vec<Recorded>>>) {
    let mut writer = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().to_string();
            if k == "content-length" {
                content_length = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        let _ = reader.read_exact(&mut body);
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    log.lock().unwrap().push(Recorded {
        method: method.clone(),
        path: path.clone(),
        headers: headers.clone(),
        body: body.clone(),
    });

    let route = path.split('?').next().unwrap_or("").to_string();
    if route == "/sse" {
        serve_sse(&mut writer, &path);
    } else if let Some(code) = route.strip_prefix("/status/") {
        let code: u16 = code.parse().unwrap_or(200);
        let payload = format!("{{\"status\":{code}}}");
        respond(&mut writer, code, "application/json", &payload);
    } else {
        if route == "/slow" {
            thread::sleep(Duration::from_millis(param(&path, "ms").unwrap_or(300)));
        }
        let payload = echo_json(&method, &path, &headers, &body);
        respond(&mut writer, 200, "application/json", &payload);
    }
    let _ = writer.flush();
    let _ = writer.shutdown(Shutdown::Both);
}

fn param(path: &str, key: &str) -> Option<u64> {
    let (_, query) = path.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.parse().ok())?
    })
}

fn respond(w: &mut TcpStream, code: u16, content_type: &str, body: &str) {
    let reason = match code {
        200 => "OK",
        201 => "Created",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nX-Test-Server: postcat\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = w.write_all(head.as_bytes());
    let _ = w.write_all(body.as_bytes());
}

/// Server-sent events, flushed one at a time so the client sees real partial
/// bodies. The stream is delimited by EOF (`Connection: close`, no length).
fn serve_sse(w: &mut TcpStream, path: &str) {
    let n = param(path, "n").unwrap_or(20);
    let gap = Duration::from_millis(param(path, "ms").unwrap_or(40));
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    if w.write_all(head.as_bytes()).is_err() {
        return;
    }
    let _ = w.flush();
    for i in 0..n {
        if w.write_all(format!("data: tick {i}\n\n").as_bytes()).is_err() {
            return;
        }
        if w.flush().is_err() {
            return;
        }
        thread::sleep(gap);
    }
}

fn echo_json(method: &str, path: &str, headers: &[(String, String)], body: &str) -> String {
    let header_json = headers
        .iter()
        .map(|(k, v)| format!("\"{}\":\"{}\"", esc(k), esc(v)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"method\":\"{}\",\"path\":\"{}\",\"headers\":{{{}}},\"body\":\"{}\"}}",
        esc(method),
        esc(path),
        header_json,
        esc(body)
    )
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}
