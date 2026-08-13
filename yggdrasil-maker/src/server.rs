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

use crate::{discover, plan, run, schema};
use std::sync::Mutex;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_CSS: &str = include_str!("../assets/app.css");
const APP_JS: &str = include_str!("../assets/app.js");

pub struct Ui {
    pub root: PathBuf,
    pub runner: run::Runner,
    /// What the user has chosen. Held by the APP, never inferred from the
    /// rendered schema: the host echoes a widget's draft back on an action and
    /// nothing more, so a selection that lived only in the pane would reset
    /// every time a refresh landed mid-decision.
    pub view: Mutex<schema::View>,
    /// Bumped whenever the painted content could have changed. Shared with
    /// the heartbeat thread, which is what carries it to the GUI.
    pub stamp: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Ui {
    fn touch(&self) {
        self.stamp.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// The pane the GUI asked for, rendered from the app's own state.
    fn schema_for(&self, tab: &str) -> serde_json::Value {
        let view = self.view.lock().expect("view lock");
        match tab {
            schema::TAB_PLAN => {
                if view.config.is_empty() {
                    schema::plan(&view, None, None)
                } else {
                    match plan::build(&self.root, &view.config, &view.profile, view.skip_smoke) {
                        Ok(built) => match serde_json::to_value(&built) {
                            Ok(value) => schema::plan(&view, Some(&value), None),
                            Err(err) => schema::plan(&view, None, Some(&err.to_string())),
                        },
                        Err(err) => schema::plan(&view, None, Some(&err)),
                    }
                }
            }
            schema::TAB_BUILD => schema::build(
                &view,
                &serde_json::to_value(self.runner.status()).unwrap_or_else(|_| json!({})),
            ),
            _ => schema::repo(
                &view,
                &serde_json::to_value(discover::survey(&self.root)).unwrap_or_else(|_| json!({})),
            ),
        }
    }

    /// One action from the pane. Returns the schema to repaint with.
    ///
    /// ⛔ THE OUTCOME IS REPORTED IN THE PERFORMER'S OWN WORDS. The runner
    /// refuses to start a second stage rather than interleaving two logs into
    /// one buffer; showing that refusal is the point of it existing. A button
    /// that silently does nothing is indistinguishable from a broken one.
    fn act(&self, action: &str, value: &str) -> serde_json::Value {
        let mut outcome: Option<String> = None;
        {
            let mut view = self.view.lock().expect("view lock");
            view.notice = None;
            if let Some(name) = action.strip_prefix("pick-config:") {
                view.config = name.to_string();
                outcome = Some(format!("config: {name}"));
            } else if let Some(name) = action.strip_prefix("pick-profile:") {
                // Picking the profile already chosen clears it, so "let the
                // config decide" stays reachable without a second control.
                if view.profile == name {
                    view.profile.clear();
                    outcome = Some("profile: the config decides".to_string());
                } else {
                    view.profile = name.to_string();
                    outcome = Some(format!("profile: {name}"));
                }
            } else {
                match action {
                    "tab" => view.tab = value.to_string(),
                    "tab-plan" => view.tab = schema::TAB_PLAN.to_string(),
                    "filter" => view.filter = value.to_string(),
                    "skip-smoke" => {
                        view.skip_smoke = matches!(value, "true" | "1" | "on" | "yes");
                    }
                    _ => {}
                }
            }
        }
        if action == "start" {
            let (config, profile) = {
                let view = self.view.lock().expect("view lock");
                (view.config.clone(), view.profile.clone())
            };
            outcome = Some(match self.runner.start(&config, &profile) {
                Ok(()) => "started".to_string(),
                Err(err) => format!("⛔ {err}"),
            });
            self.view.lock().expect("view lock").tab = schema::TAB_BUILD.to_string();
        }
        {
            let mut view = self.view.lock().expect("view lock");
            view.notice = outcome;
        }
        self.touch();
        let tab = self.view.lock().expect("view lock").tab.clone();
        self.schema_for(&tab)
    }
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
            // ── the libyggterm panes ──────────────────────────────────────
            ("GET", "/ping") => json_ok(&json!({
                "ok": true,
                "app_name": "Yggdrasil Maker",
                "document_version": ui.stamp.load(std::sync::atomic::Ordering::SeqCst).to_string(),
            })),
            // Both panes render the same view. One app, one state: a rail
            // showing a different tab from the viewport beside it would be two
            // answers to "what am I looking at".
            ("GET", "/pane/maker") | ("GET", "/pane/doc") => {
                let tab = ui.view.lock().expect("view lock").tab.clone();
                json_ok(&ui.schema_for(&tab))
            }
            ("POST", "/action") => {
                let action = param(&query, "action").unwrap_or_default();
                let value = param(&query, "value").unwrap_or_default();
                json_ok(&ui.act(&action, &value))
            }
            // ── the standalone web UI, for use outside yggterm ────────────
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
