//! The libyggterm side: declaring a web surface over OSC 7717 on our own PTY.
//!
//! The whole contract is a byte format plus two environment variables, which is
//! why this file has no dependency on the yggterm source tree. A path
//! dependency on a sibling checkout is what made the previous `yggdrasil-maker`
//! unbuildable from a fresh clone; we are not repeating it.
//!
//! Wire grammar (yggterm `crates/yggterm-server/src/app_declare.rs`):
//!
//! ```text
//! ESC ] 7717 ; <verb> ; <action> ; <base64-STANDARD-json> BEL
//! ```

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

/// The verb we speak. yggterm also routes `sidebar` and `fido2`; we declare a
/// viewport web surface and nothing else.
const VERB: &str = "web-surface";

/// yggterm expires a surface after 15s of silence
/// (`WEB_SURFACE_STALE_AFTER_MS`), so that a SIGKILLed app can never leave a
/// stuck overlay over someone's terminal. Beat comfortably inside that.
pub const HEARTBEAT: Duration = Duration::from_secs(4);

/// Where the app keeps its own state. Host-resident by contract: yggterm is a
/// pure renderer and persists none of an app's data, so anything we want to
/// survive lives on the host the CLI runs on — which may be the far side of an
/// ssh hop, and that is precisely the point.
pub fn state_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join(".yggterm/yggdrasil-maker"))
}

/// The session key of the yggterm PTY we are running on, if we are on one.
///
/// `YGGTERM_SESSION_ID` is the direct export. `LC_YGGTERM_SESSION_ID` is the
/// iTerm2 trick: a user-typed `ssh <host>` strips the environment, but stock
/// OpenSSH forwards `LC_*`, so an app on the far side of a MANUAL hop can still
/// tell it is inside a surface. Checking only the first is a real, reported bug
/// (yedit said "not inside yggterm" after an ssh, 2026-07-23).
pub fn session_id() -> Option<String> {
    for key in ["YGGTERM_SESSION_ID", "LC_YGGTERM_SESSION_ID"] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// A declared surface: the payload we repeat on every heartbeat.
///
/// The payload is carried in full on each beat rather than trimmed to a
/// keepalive, because a terminal remount replays scrollback and a full payload
/// is what lets the surface heal itself from that replay.
pub struct Surface {
    payload: Value,
}

impl Surface {
    /// `session` MUST be non-empty: the GUI's handler drops the whole message
    /// when it is missing, silently, before anything else is looked at.
    pub fn new(session: String, url: &str, title: &str) -> Self {
        Self {
            payload: json!({
                "session": session,
                "url": url,
                "title": title,
                // An ephemeral jar: we store nothing in the browser, all state
                // is host-resident, and "temp" keeps us out of the shared
                // profile's single-writer lock entirely.
                "profile": "temp",
            }),
        }
    }

    /// `open` is an intent and is emitted exactly once. A heartbeat must never
    /// be able to create or navigate a surface — that is what made an earlier
    /// app clobber the user's own navigation.
    pub fn open(&self) {
        emit("open", &self.payload);
    }

    pub fn heartbeat(&self) {
        emit("heartbeat", &self.payload);
    }

    pub fn close(&self) {
        emit("close", &self.payload);
    }

    /// Block in the foreground, beating, until a signal arrives. A surface is a
    /// foreground program, not a background session: when this returns, the
    /// user is done with the app.
    ///
    /// `close` runs on every exit path, including the signal paths, so the
    /// overlay goes away the moment we do rather than 15s later.
    pub fn hold_until_signalled(&self) -> std::io::Result<()> {
        let stop = Arc::new(AtomicBool::new(false));
        // The overlay's ✕ writes \x03 to the PTY — the terminal-native way to
        // end a foreground program — so SIGINT is how a normal close arrives.
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
        signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;

        self.open();
        while !stop.load(Ordering::Relaxed) {
            // Sleep in slices so a signal is noticed promptly instead of up to
            // a full beat late.
            for _ in 0..HEARTBEAT.as_secs() * 10 {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            self.heartbeat();
        }
        self.close();
        Ok(())
    }
}

/// Write one control sequence to stdout and flush it.
///
/// stdout is the channel because the transport IS the terminal byte stream:
/// that is what makes a local session and a session on the far side of ssh
/// behave identically, and what makes a plain terminal ignore us harmlessly.
fn emit(action: &str, payload: &Value) {
    let blob = base64::engine::general_purpose::STANDARD.encode(
        serde_json::to_vec(payload).expect("the payload is plain JSON and always serialises"),
    );
    let mut out = std::io::stdout().lock();
    let _ = write!(out, "\x1b]7717;{VERB};{action};{blob}\x07");
    let _ = out.flush();
}

/// Record ourselves in yggterm's launcher registry so the app is reachable from
/// the `+` menu and the cwd tree, not only by typing its name.
///
/// Written on EVERY run on purpose: that is what repairs the recorded path
/// after the binary moves or is upgraded. The daemon prunes manifests whose
/// binary no longer resolves, which is the entire uninstall story.
///
/// Registration is a convenience, never a precondition — every failure here is
/// swallowed, because not being in a menu must not stop the app from running.
pub fn write_launcher_manifest() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    // The binary path must be absolute: a bare name would have to resolve
    // against a PATH that a non-interactive ssh session does not have.
    let Ok(binary) = std::env::current_exe() else {
        return;
    };
    let Some(binary) = binary.to_str() else {
        return;
    };
    let dir = std::path::PathBuf::from(home).join(".yggterm/apps");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    // `name` must equal the file stem or the shell treats the manifest as
    // half-written and ignores it.
    let manifest = json!({
        "name": "yggdrasil-maker",
        "label": "Yggdrasil Maker",
        "icon": "🌲",
        "binary": binary,
        "verbs": [ { "id": "open", "label": "Open Yggdrasil Maker", "args": [] } ],
    });
    let Ok(body) = serde_json::to_vec_pretty(&manifest) else {
        return;
    };
    let _ = std::fs::write(dir.join("yggdrasil-maker.json"), body);
}
