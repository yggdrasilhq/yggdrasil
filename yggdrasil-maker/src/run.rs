//! The run flow: execute a real stage and stream its real log.
//!
//! v0 runs the pipeline's cheap end — `./mkconfig.sh --config <c> --profile <p>
//! --dry-run --skip-smoke`. That is a genuine execution of the repository's own
//! entry point: it resolves the config file, converts the TOML through
//! `toml-to-env.sh`, sources the result, validates the profile, and prints the
//! resolved build command lines. Everything it reports is something the shell
//! really did.
//!
//! What it does NOT do is unpack a chroot, and the UI says so plainly. The
//! image stages need root and take tens of minutes; running them from a
//! terminal surface is a later flow, specced in the ADR. The rule this file
//! exists to honour: whatever ships is real. There is no simulated progress
//! here — the percentage of a stage that has not run is not shown, because we
//! do not know it.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Idle,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub seq: usize,
    pub stream: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub state: State,
    /// The argv actually spawned, for the record.
    pub command: Vec<String>,
    pub lines: Vec<Line>,
    pub exit_code: Option<i32>,
    /// Set when the process could not be spawned at all.
    pub error: Option<String>,
    pub started_unix_ms: Option<u128>,
    pub finished_unix_ms: Option<u128>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            state: State::Idle,
            command: Vec::new(),
            lines: Vec::new(),
            exit_code: None,
            error: None,
            started_unix_ms: None,
            finished_unix_ms: None,
        }
    }
}

/// The single source of truth for what the run flow is doing.
///
/// One runner, one status: the UI polls this and renders it, and there is no
/// second copy of the state anywhere that could disagree with it.
#[derive(Clone)]
pub struct Runner {
    root: PathBuf,
    status: Arc<Mutex<Status>>,
}

impl Runner {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            status: Arc::new(Mutex::new(Status::default())),
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().expect("run status lock").clone()
    }

    /// Start the cheap real stage. Refuses while one is already running rather
    /// than interleaving two logs into one buffer.
    pub fn start(&self, config: &str, profile: &str) -> Result<(), String> {
        {
            let status = self.status.lock().expect("run status lock");
            if status.state == State::Running {
                return Err("a stage is already running".to_string());
            }
        }

        let mut argv = vec!["./mkconfig.sh".to_string(), "--config".to_string(), config.to_string()];
        if !profile.is_empty() {
            argv.push("--profile".to_string());
            argv.push(profile.to_string());
        }
        // --dry-run is what keeps this cheap and rootless; --skip-smoke keeps
        // the dry run from also printing a smoke command that we are not the
        // ones deciding to skip.
        argv.push("--dry-run".to_string());
        argv.push("--skip-smoke".to_string());

        {
            let mut status = self.status.lock().expect("run status lock");
            *status = Status {
                state: State::Running,
                command: argv.clone(),
                started_unix_ms: Some(now_ms()),
                ..Status::default()
            };
        }

        let root = self.root.clone();
        let handle = Arc::clone(&self.status);
        std::thread::spawn(move || execute(&root, argv, handle));
        Ok(())
    }
}

fn execute(root: &Path, argv: Vec<String>, status: Arc<Mutex<Status>>) {
    let mut command = Command::new("bash");
    command
        .arg(&argv[0])
        .args(&argv[1..])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let mut status = status.lock().expect("run status lock");
            status.state = State::Failed;
            status.error = Some(format!("could not start {}: {err}", argv[0]));
            status.finished_unix_ms = Some(now_ms());
            return;
        }
    };

    // Both streams are pumped concurrently. Reading stdout to completion first
    // deadlocks the moment a chatty build fills the stderr pipe buffer.
    let mut pumps = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        pumps.push(pump(stdout, "stdout", Arc::clone(&status)));
    }
    if let Some(stderr) = child.stderr.take() {
        pumps.push(pump(stderr, "stderr", Arc::clone(&status)));
    }

    let wait = child.wait();
    for pump in pumps {
        let _ = pump.join();
    }

    let mut status = status.lock().expect("run status lock");
    status.finished_unix_ms = Some(now_ms());
    match wait {
        Ok(exit) => {
            status.exit_code = exit.code();
            // The exit code is the verdict. We do not scan the log for the word
            // "error" and second-guess it.
            status.state = if exit.success() { State::Done } else { State::Failed };
        }
        Err(err) => {
            status.state = State::Failed;
            status.error = Some(format!("could not wait for the stage: {err}"));
        }
    }
}

fn pump<R>(reader: R, stream: &'static str, status: Arc<Mutex<Status>>) -> std::thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(text) = line else { break };
            let mut status = status.lock().expect("run status lock");
            let seq = status.lines.len();
            status.lines.push(Line { seq, stream, text });
        }
    })
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
