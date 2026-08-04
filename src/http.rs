//! Request execution: turns a `RequestModel` into a reqwest call and streams
//! the response back to the UI chunk by chunk (SSE-friendly).

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use base64::Engine as _;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::app::AppEvent;
use crate::model::{substitute, AuthType, BodyType, KV, Method, RequestModel};

#[derive(Debug, Clone)]
pub struct ResponseData {
    pub status: u16,
    pub reason: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub elapsed_ms: u64,
}

/// Stop reading a runaway stream once it exceeds this many bytes.
const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;

pub fn build_client() -> reqwest::Client {
    // No whole-request timeout: streaming responses (SSE, chunked) may stay
    // open for a long time. Instead: bounded connect + per-read idle timeout.
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(120))
        .user_agent(concat!("postcat/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build http client")
}

fn map_method(m: Method) -> reqwest::Method {
    match m {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Options => reqwest::Method::OPTIONS,
    }
}

fn active<'a>(rows: &'a [KV], env: &'a [KV]) -> impl Iterator<Item = (String, String)> + 'a {
    rows.iter()
        .filter(|r| r.enabled && !r.key.is_empty())
        .map(|r| (substitute(&r.key, env), substitute(&r.value, env)))
}

/// Run the request, emitting Started / Chunk / Finished (or Failed) events.
/// `seq` tags every event so the UI can drop stale ones after a cancel.
pub async fn send_streaming(
    client: reqwest::Client,
    req: RequestModel,
    env: Vec<KV>,
    seq: u64,
    tx: Sender<AppEvent>,
) {
    if let Err(error) = run(client, req, env, seq, &tx).await {
        let _ = tx.send(AppEvent::Failed { seq, error });
    }
}

async fn run(
    client: reqwest::Client,
    req: RequestModel,
    env: Vec<KV>,
    seq: u64,
    tx: &Sender<AppEvent>,
) -> Result<(), String> {
    let mut raw_url = substitute(req.url.trim(), &env);
    if raw_url.is_empty() {
        return Err("empty URL — press i to edit it".into());
    }
    if !raw_url.contains("://") {
        raw_url = format!("https://{raw_url}");
    }
    let url = reqwest::Url::parse(&raw_url).map_err(|e| format!("invalid URL: {e}"))?;

    let mut builder = client.request(map_method(req.method), url);

    let params: Vec<(String, String)> = active(&req.params, &env).collect();
    if !params.is_empty() {
        builder = builder.query(&params);
    }

    let mut headers = HeaderMap::new();
    for (k, v) in active(&req.headers, &env) {
        let name = HeaderName::try_from(k.as_str())
            .map_err(|_| format!("invalid header name: {k:?}"))?;
        let value = HeaderValue::try_from(v.as_str())
            .map_err(|_| format!("invalid value for header {k:?}"))?;
        headers.append(name, value);
    }

    match req.auth.typ {
        AuthType::None => {}
        AuthType::Bearer => {
            let token = substitute(req.auth.token.trim(), &env);
            let value = HeaderValue::try_from(format!("Bearer {token}"))
                .map_err(|_| "invalid bearer token".to_string())?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        AuthType::Basic => {
            let user = substitute(&req.auth.user, &env);
            let pass = substitute(&req.auth.pass, &env);
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
            let value = HeaderValue::try_from(format!("Basic {encoded}"))
                .map_err(|_| "invalid basic credentials".to_string())?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }

    match req.body_type {
        BodyType::None => {}
        BodyType::Json => {
            if !headers.contains_key(reqwest::header::CONTENT_TYPE) {
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
            }
            builder = builder.body(substitute(&req.body, &env));
        }
        BodyType::Text => {
            if !headers.contains_key(reqwest::header::CONTENT_TYPE) {
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; charset=utf-8"),
                );
            }
            builder = builder.body(substitute(&req.body, &env));
        }
        BodyType::Form => {
            let fields: Vec<(String, String)> = active(&req.form, &env).collect();
            builder = builder.form(&fields);
        }
    }

    builder = builder.headers(headers);

    let started = Instant::now();
    let mut resp = builder.send().await.map_err(|e| friendly_error(&e))?;

    let status = resp.status();
    let version = format!("{:?}", resp.version());
    let resp_headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let _ = tx.send(AppEvent::Started {
        seq,
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or("").to_string(),
        version,
        headers: resp_headers,
        ttfb_ms: started.elapsed().as_millis() as u64,
    });

    let mut total = 0usize;
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                total += chunk.len();
                let _ = tx.send(AppEvent::Chunk { seq, bytes: chunk.to_vec() });
                if total >= MAX_STREAM_BYTES {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => return Err(friendly_error(&e)),
        }
    }

    let _ = tx.send(AppEvent::Finished {
        seq,
        total_ms: started.elapsed().as_millis() as u64,
    });
    Ok(())
}

fn friendly_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        return "timed out — no data for 120s".into();
    }
    if e.is_connect() {
        // The inner source usually carries the useful part (dns, refused, tls…).
        let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
        while let Some(s) = src {
            if s.source().is_none() {
                return format!("connection failed: {s}");
            }
            src = s.source();
        }
        return "connection failed".into();
    }
    format!("{e}")
}
