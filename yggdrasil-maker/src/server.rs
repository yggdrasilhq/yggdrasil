//! The loopback UI server.
//!
//! Bound to 127.0.0.1 only. That is not a limitation to work around: a surface
//! declared from a session on a remote host is fetched by the webview through
//! that session's own ssh tunnel, so `127.0.0.1` resolves on the machine the
//! CLI is running on — the right one, by construction. Binding 0.0.0.0 would
//! buy nothing and publish the UI to the LAN.

use std::net::TcpListener;
use std::path::PathBuf;

use serde_json::json;
use tiny_http::{Header, Response, Server};

use crate::{discover, plan, run};

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_CSS: &str = include_str!("../assets/app.css");
const APP_JS: &str = include_str!("../assets/app.js");

pub struct Ui {
    pub root: PathBuf,
    pub runner: run::Runner,
}

/// Serve until the process ends. Runs on its own thread; the main thread owns
/// the surface heartbeat.
pub fn serve(listener: TcpListener, ui: Ui) {
    let server = match Server::from_listener(listener, None) {
        Ok(server) => server,
        Err(err) => {
            eprintln!("[yggdrasil-maker] the UI server could not start: {err}");
            return;
        }
    };

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("/").to_string();
        let query = url.split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();

        let response = match (request.method().as_str(), path.as_str()) {
            ("GET", "/") => html(INDEX_HTML),
            ("GET", "/app.css") => text(APP_CSS, "text/css; charset=utf-8"),
            ("GET", "/app.js") => text(APP_JS, "text/javascript; charset=utf-8"),
            ("GET", "/api/repo") => json_ok(&discover::survey(&ui.root)),
            ("GET", "/api/config") => match param(&query, "name") {
                Some(name) => match std::fs::read_to_string(ui.root.join(&name)) {
                    Ok(text) => json_ok(&json!({
                        "name": name,
                        "knobs": discover::parse_knobs(&text),
                    })),
                    Err(err) => json_err(404, &format!("{name}: {err}")),
                },
                None => json_err(400, "name is required"),
            },
            ("GET", "/api/plan") => {
                let config = param(&query, "config").unwrap_or_default();
                let profile = param(&query, "profile").unwrap_or_default();
                let skip_smoke = param(&query, "skip_smoke").as_deref() == Some("1");
                match plan::build(&ui.root, &config, &profile, skip_smoke) {
                    Ok(plan) => json_ok(&plan),
                    Err(err) => json_err(400, &err),
                }
            }
            ("GET", "/api/run") => json_ok(&ui.runner.status()),
            ("POST", "/api/run") => {
                let config = param(&query, "config").unwrap_or_default();
                let profile = param(&query, "profile").unwrap_or_default();
                match ui.runner.start(&config, &profile) {
                    Ok(()) => json_ok(&ui.runner.status()),
                    Err(err) => json_err(409, &err),
                }
            }
            _ => json_err(404, "no such route"),
        };

        let _ = request.respond(response);
    }
}

type Body = Response<std::io::Cursor<Vec<u8>>>;

fn html(body: &str) -> Body {
    text(body, "text/html; charset=utf-8")
}

fn text(body: &str, content_type: &str) -> Body {
    let header = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("a static content type header is always valid");
    Response::from_data(body.as_bytes().to_vec()).with_header(header)
}

fn json_ok<T: serde::Serialize>(value: &T) -> Body {
    match serde_json::to_vec(value) {
        Ok(body) => {
            let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                .expect("a static content type header is always valid");
            Response::from_data(body).with_header(header)
        }
        Err(err) => json_err(500, &format!("could not serialise the response: {err}")),
    }
}

fn json_err(status: u16, message: &str) -> Body {
    let body = serde_json::to_vec(&json!({ "error": message }))
        .unwrap_or_else(|_| br#"{"error":"unserialisable"}"#.to_vec());
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("a static content type header is always valid");
    Response::from_data(body)
        .with_header(header)
        .with_status_code(status)
}

/// Read one key out of a query string, percent-decoding the value.
fn param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, value)| percent_decode(value))
        .filter(|value| !value.is_empty())
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_are_decoded() {
        assert_eq!(param("config=ygg.local.toml", "config").as_deref(), Some("ygg.local.toml"));
        assert_eq!(param("a=1&name=ygg%2Eexample%2Etoml", "name").as_deref(), Some("ygg.example.toml"));
        assert_eq!(param("config=", "config"), None);
        assert_eq!(param("other=1", "config"), None);
    }
}
