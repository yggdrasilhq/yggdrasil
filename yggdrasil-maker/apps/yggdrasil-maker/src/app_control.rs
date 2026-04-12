use anyhow::{Context, Result, bail};
use maker_model::{BuildProfile, JourneyStage, PresetId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::gui::RightPanelMode;

const APP_CONTROL_REQUESTS_DIR: &str = "app-control-requests";
const APP_CONTROL_RESPONSES_DIR: &str = "app-control-responses";
const APP_CONTROL_CAPTURES_DIR: &str = "screenshots";
const APP_CONTROL_RECORDINGS_DIR: &str = "recordings";
const CLIENT_INSTANCES_DIR: &str = "client-instances";
const APP_CONTROL_TARGET_PID_ENV: &str = "YGGDRASIL_MAKER_APP_PID";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppControlCommand {
    FocusWindow,
    DescribeState,
    DescribeRows,
    CaptureScreenshot {
        output_path: String,
    },
    CaptureScreenRecording {
        output_path: String,
        duration_secs: u64,
    },
    NewSetup {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        preset: Option<PresetId>,
        #[serde(default)]
        profile: Option<BuildProfile>,
        #[serde(default)]
        hostname: Option<String>,
    },
    SelectSetup {
        setup_id: String,
    },
    SaveSetup,
    SetJourneyStage {
        stage: JourneyStage,
    },
    SetSetupName {
        value: String,
    },
    SetHostname {
        value: String,
    },
    SetArtifactsDir {
        value: String,
    },
    SetRepoRoot {
        value: String,
    },
    SetBuildContext {
        artifacts_dir: String,
        repo_root: String,
    },
    ApplyPreset {
        preset: PresetId,
    },
    SetProfile {
        profile: BuildProfile,
    },
    ToggleNvidia,
    ToggleLts,
    SetSidebarOpen {
        open: bool,
    },
    SetUtilityPaneOpen {
        open: bool,
    },
    SetRightPanelMode {
        mode: RightPanelMode,
    },
    SetAppearancePanelOpen {
        open: bool,
    },
    StartBuild,
    OpenBuildDetails,
    RevealPrimaryArtifact,
}

impl AppControlCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FocusWindow => "focus_window",
            Self::DescribeState => "describe_state",
            Self::DescribeRows => "describe_rows",
            Self::CaptureScreenshot { .. } => "capture_screenshot",
            Self::CaptureScreenRecording { .. } => "capture_screen_recording",
            Self::NewSetup { .. } => "new_setup",
            Self::SelectSetup { .. } => "select_setup",
            Self::SaveSetup => "save_setup",
            Self::SetJourneyStage { .. } => "set_journey_stage",
            Self::SetSetupName { .. } => "set_setup_name",
            Self::SetHostname { .. } => "set_hostname",
            Self::SetArtifactsDir { .. } => "set_artifacts_dir",
            Self::SetRepoRoot { .. } => "set_repo_root",
            Self::SetBuildContext { .. } => "set_build_context",
            Self::ApplyPreset { .. } => "apply_preset",
            Self::SetProfile { .. } => "set_profile",
            Self::ToggleNvidia => "toggle_nvidia",
            Self::ToggleLts => "toggle_lts",
            Self::SetSidebarOpen { .. } => "set_sidebar_open",
            Self::SetUtilityPaneOpen { .. } => "set_utility_pane_open",
            Self::SetRightPanelMode { .. } => "set_right_panel_mode",
            Self::SetAppearancePanelOpen { .. } => "set_appearance_panel_open",
            Self::StartBuild => "start_build",
            Self::OpenBuildDetails => "open_build_details",
            Self::RevealPrimaryArtifact => "reveal_primary_artifact",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppControlRequest {
    pub request_id: String,
    pub created_at_ms: u128,
    #[serde(default)]
    pub preferred_pid: Option<u32>,
    pub command: AppControlCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppControlResponse {
    pub request_id: String,
    pub handled_by_pid: u32,
    pub completed_at_ms: u128,
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInstanceRecord {
    pub pid: u32,
    pub started_at_ms: u128,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub wayland_display: Option<String>,
    #[serde(default)]
    pub xdg_session_id: Option<String>,
    #[serde(default)]
    pub xdg_runtime_dir: Option<String>,
    #[serde(default)]
    pub xauthority: Option<String>,
}

pub fn resolve_home_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("YGGDRASIL_MAKER_SETUP_ROOT") {
        let root = PathBuf::from(path);
        return Ok(root.parent().map(Path::to_path_buf).unwrap_or(root));
    }

    match std::env::consts::OS {
        "linux" => {
            let base = std::env::var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|_| {
                    std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/share"))
                })
                .context("unable to resolve Linux data directory")?;
            Ok(base.join("yggdrasil-maker"))
        }
        "macos" => {
            let home = std::env::var("HOME").context("unable to resolve HOME")?;
            Ok(PathBuf::from(home)
                .join("Library/Application Support")
                .join("yggdrasil-maker"))
        }
        "windows" => {
            let appdata = std::env::var("APPDATA").context("unable to resolve APPDATA")?;
            Ok(PathBuf::from(appdata).join("yggdrasil-maker"))
        }
        other => bail!("unsupported platform for app control: {other}"),
    }
}

pub fn app_control_requests_dir(home: &Path) -> PathBuf {
    home.join(APP_CONTROL_REQUESTS_DIR)
}

pub fn app_control_responses_dir(home: &Path) -> PathBuf {
    home.join(APP_CONTROL_RESPONSES_DIR)
}

pub fn app_control_captures_dir(home: &Path) -> PathBuf {
    home.join(APP_CONTROL_CAPTURES_DIR)
}

pub fn app_control_recordings_dir(home: &Path) -> PathBuf {
    home.join(APP_CONTROL_RECORDINGS_DIR)
}

pub fn client_instances_dir(home: &Path) -> PathBuf {
    home.join(CLIENT_INSTANCES_DIR)
}

pub fn default_screenshot_output_path(home: &Path, request_id: &str) -> PathBuf {
    app_control_captures_dir(home).join(format!("app-{request_id}.png"))
}

pub fn default_recording_output_path(home: &Path, request_id: &str) -> PathBuf {
    app_control_recordings_dir(home).join(format!("app-{request_id}.mp4"))
}

pub fn current_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub fn register_client_instance(home: &Path) -> Result<PathBuf> {
    let dir = client_instances_dir(home);
    fs::create_dir_all(&dir)
        .with_context(|| format!("creating client instances dir {}", dir.display()))?;
    cleanup_stale_client_instances(home)?;
    let path = dir.join(format!("{}.json", std::process::id()));
    let temp = path.with_extension("json.tmp");
    let record = ClientInstanceRecord {
        pid: std::process::id(),
        started_at_ms: current_millis(),
        display: std::env::var("DISPLAY").ok(),
        wayland_display: std::env::var("WAYLAND_DISPLAY").ok(),
        xdg_session_id: std::env::var("XDG_SESSION_ID").ok(),
        xdg_runtime_dir: std::env::var("XDG_RUNTIME_DIR").ok(),
        xauthority: std::env::var("XAUTHORITY").ok(),
    };
    fs::write(&temp, serde_json::to_vec_pretty(&record)?)
        .with_context(|| format!("writing client instance {}", temp.display()))?;
    fs::rename(&temp, &path)
        .with_context(|| format!("publishing client instance {}", path.display()))?;
    Ok(path)
}

pub fn cleanup_stale_client_instances(home: &Path) -> Result<()> {
    let dir = client_instances_dir(home);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(pid) = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.parse::<u32>().ok())
        else {
            let _ = fs::remove_file(&path);
            continue;
        };
        if !process_is_alive(pid) {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

pub fn active_client_instance_records(home: &Path) -> Result<Vec<ClientInstanceRecord>> {
    cleanup_stale_client_instances(home)?;
    let dir = client_instances_dir(home);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<ClientInstanceRecord>(&bytes) else {
            let _ = fs::remove_file(&path);
            continue;
        };
        if process_is_alive(record.pid) {
            records.push(record);
        }
    }
    records.sort_by_key(|record| std::cmp::Reverse(record.started_at_ms));
    Ok(records)
}

pub fn enqueue_app_control_request(
    home: &Path,
    command: AppControlCommand,
    preferred_pid: Option<u32>,
) -> Result<AppControlRequest> {
    let requests_dir = app_control_requests_dir(home);
    fs::create_dir_all(&requests_dir)
        .with_context(|| format!("creating {}", requests_dir.display()))?;
    fs::create_dir_all(app_control_captures_dir(home))
        .with_context(|| format!("creating {}", app_control_captures_dir(home).display()))?;
    fs::create_dir_all(app_control_recordings_dir(home))
        .with_context(|| format!("creating {}", app_control_recordings_dir(home).display()))?;
    fs::create_dir_all(app_control_responses_dir(home))
        .with_context(|| format!("creating {}", app_control_responses_dir(home).display()))?;
    let request = AppControlRequest {
        request_id: format!("{}-{}", std::process::id(), current_millis()),
        created_at_ms: current_millis(),
        preferred_pid,
        command,
    };
    let final_path = requests_dir.join(format!("{}.json", request.request_id));
    let temp_path = requests_dir.join(format!("{}.json.tmp", request.request_id));
    fs::write(&temp_path, serde_json::to_vec_pretty(&request)?)
        .with_context(|| format!("writing app control request {}", temp_path.display()))?;
    fs::rename(&temp_path, &final_path)
        .with_context(|| format!("publishing app control request {}", final_path.display()))?;
    Ok(request)
}

pub fn take_next_app_control_request(
    home: &Path,
    worker_pid: u32,
) -> Result<Option<(PathBuf, AppControlRequest)>> {
    let requests_dir = app_control_requests_dir(home);
    fs::create_dir_all(&requests_dir)
        .with_context(|| format!("creating {}", requests_dir.display()))?;
    recover_stale_inflight_requests(&requests_dir)?;
    let mut entries = fs::read_dir(&requests_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|value| value.to_str()) == Some("json")
                && !path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with("inflight-"))
        })
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let request = serde_json::from_slice::<AppControlRequest>(&bytes)
            .with_context(|| format!("parsing {}", path.display()))?;
        if let Some(preferred_pid) = request.preferred_pid
            && preferred_pid != worker_pid
        {
            continue;
        }
        let inflight_path = requests_dir.join(format!("inflight-{}.json", request.request_id));
        match fs::rename(&path, &inflight_path) {
            Ok(()) => return Ok(Some((inflight_path, request))),
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("marking {} inflight", path.display()));
            }
        }
    }
    Ok(None)
}

pub fn complete_app_control_request(
    home: &Path,
    inflight_path: &Path,
    response: &AppControlResponse,
) -> Result<()> {
    let responses_dir = app_control_responses_dir(home);
    fs::create_dir_all(&responses_dir)
        .with_context(|| format!("creating {}", responses_dir.display()))?;
    let final_path = responses_dir.join(format!("{}.json", response.request_id));
    let temp_path = responses_dir.join(format!("{}.json.tmp", response.request_id));
    fs::write(&temp_path, serde_json::to_vec_pretty(response)?)
        .with_context(|| format!("writing app control response {}", temp_path.display()))?;
    fs::rename(&temp_path, &final_path)
        .with_context(|| format!("publishing app control response {}", final_path.display()))?;
    let _ = fs::remove_file(inflight_path);
    Ok(())
}

pub fn wait_for_app_control_response(
    home: &Path,
    request_id: &str,
    timeout: Duration,
) -> Result<AppControlResponse> {
    let deadline = Instant::now() + timeout;
    let path = app_control_responses_dir(home).join(format!("{}.json", request_id));
    loop {
        match fs::read(&path) {
            Ok(bytes) => {
                let response = serde_json::from_slice::<AppControlResponse>(&bytes)
                    .with_context(|| format!("parsing {}", path.display()))?;
                let _ = fs::remove_file(&path);
                return Ok(response);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for app control response: {request_id}");
        }
        thread::sleep(Duration::from_millis(40));
    }
}

pub fn request_app_control(
    home: &Path,
    command: AppControlCommand,
    timeout_ms: u64,
) -> Result<AppControlResponse> {
    let preferred_pid = ensure_live_client_pid(home, timeout_ms)?;
    let request = enqueue_app_control_request(home, command, preferred_pid)?;
    wait_for_app_control_response(
        home,
        &request.request_id,
        Duration::from_millis(timeout_ms.max(250)),
    )
}

fn ensure_live_client_pid(home: &Path, timeout_ms: u64) -> Result<Option<u32>> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(750).max(100));
    let targeted_pid = std::env::var(APP_CONTROL_TARGET_PID_ENV)
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    loop {
        let records = active_client_instance_records(home)?;
        if let Some(pid) = targeted_pid {
            if records.iter().any(|record| record.pid == pid) {
                return Ok(Some(pid));
            }
        } else if let Some(record) = records.first() {
            return Ok(Some(record.pid));
        }
        if Instant::now() >= deadline {
            if let Some(pid) = targeted_pid {
                bail!("targeted Yggdrasil Maker GUI client is not live for app control: {pid}");
            }
            bail!("no live Yggdrasil Maker GUI client is registered for app control");
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn recover_stale_inflight_requests(requests_dir: &Path) -> Result<()> {
    let entries = match fs::read_dir(requests_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", requests_dir.display()));
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with("inflight-") || !file_name.ends_with(".json") {
            continue;
        }
        let final_name = file_name.trim_start_matches("inflight-");
        let final_path = requests_dir.join(final_name);
        match fs::rename(&path, &final_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("recovering inflight request {}", path.display()));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

#[cfg(not(unix))]
fn process_is_alive(pid: u32) -> bool {
    pid != 0
}
