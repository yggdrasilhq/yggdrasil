use anyhow::{Context, Result, anyhow};
use dioxus::desktop::{
    Config, LogicalSize, WindowBuilder, WindowCloseBehaviour, use_window, window,
};
use dioxus::document;
use dioxus::prelude::*;
use dioxus_core::schedule_update;
use dioxus_desktop::DesktopContext;
use dioxus_desktop::UserWindowEvent;
#[cfg(target_os = "linux")]
use gtk::prelude::GtkWindowExt;
use keyboard_types::{Key, Modifiers};
use maker_app::{BuildInputs, MakerApp, StoredSetupSummary};
use maker_build::{
    ARTIFACT_MANIFEST_NAME, ArtifactKind, ArtifactManifest, ArtifactRecord, BuildMode,
    read_artifact_manifest,
};
use maker_copy::preset_cards;
use maker_model::{BuildProfile, JourneyStage, PresetId, SetupDocument};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tao::event_loop::{EventLoop, EventLoopBuilder};
#[cfg(target_os = "linux")]
use tao::platform::unix::{EventLoopBuilderExtUnix, WindowExtUnix};
use tao::window::ResizeDirection;
use tokio::time::sleep;
use yggterm_core::append_trace_event;
use yggui::{
    ChromePalette, HoveredChromeControl, RailHeader, RailScrollBody, RailSectionTitle,
    SideRailShell, THEME_EDITOR_SWATCHES, TOAST_CSS, TitlebarChrome, ToastItem, ToastPalette,
    ToastTone, ToastViewport, WindowControlsStrip, append_theme_stop, clamp_theme_spec,
    default_theme_editor_spec, dominant_accent, gradient_css, preview_surface_css, shell_tint,
};
use yggui_contract::{UiTheme, YgguiThemeColorStop, YgguiThemeSpec};

use crate::app_capture::{
    capture_visible_app_surface, describe_window, focus_app_window, record_visible_app_surface,
};
use crate::app_control::{
    AppControlCommand, AppControlRequest, AppControlResponse, complete_app_control_request,
    default_recording_output_path, default_screenshot_output_path, register_client_instance,
    take_next_app_control_request,
};
#[cfg(target_os = "linux")]
use crate::linux_desktop::{
    YGGDRASIL_MAKER_DESKTOP_APP_ID, YGGDRASIL_MAKER_WM_CLASS, refresh_dev_desktop_integration,
};
use crate::window_icon;

static BOOTSTRAP: Lazy<Mutex<Option<MakerBootstrap>>> = Lazy::new(|| Mutex::new(None));
static APP_MOUNT_GENERATION: AtomicU64 = AtomicU64::new(0);

const LEFT_RAIL_WIDTH: usize = 248;
const RIGHT_RAIL_WIDTH: usize = 318;
const EDGE_RESIZE_HANDLE: usize = 5;
const CORNER_RESIZE_HANDLE: usize = 10;
const THEME_EDITOR_PAD_SIZE: f64 = 208.0;
const UI_FONT_FAMILY: &str = "\"Inter Variable\", \"Inter\", system-ui, sans-serif";
const MAKER_MOTION_CSS: &str = r#"
@keyframes makerPulseGlow {
  0%, 100% { box-shadow: 0 10px 26px color-mix(in srgb, var(--maker-accent) 28%, transparent); }
  50% { box-shadow: 0 14px 34px color-mix(in srgb, var(--maker-accent) 42%, transparent); }
}
@keyframes makerFloat {
  0%, 100% { transform: translateY(0); }
  50% { transform: translateY(-4px); }
}
@keyframes makerProgressSweep {
  0% { transform: scaleX(0.92); opacity: 0.74; }
  50% { transform: scaleX(1); opacity: 1; }
  100% { transform: scaleX(0.92); opacity: 0.74; }
}
"#;

fn set_bootstrap(bootstrap: MakerBootstrap) {
    if let Ok(mut guard) = BOOTSTRAP.lock() {
        *guard = Some(bootstrap);
    }
}

fn cloned_bootstrap() -> Option<MakerBootstrap> {
    BOOTSTRAP.lock().ok().and_then(|guard| guard.clone())
}

fn sync_bootstrap_from_state(state: &MakerUiState) {
    if let Ok(mut guard) = BOOTSTRAP.lock()
        && let Some(bootstrap) = guard.as_mut()
    {
        bootstrap.shell_settings = state.shell_settings.clone();
        bootstrap.saved_setups = state.saved_setups.clone();
        bootstrap.current_setup = state.current_setup.clone();
        bootstrap.artifacts_dir = state.artifacts_dir.clone();
        bootstrap.repo_root = state.repo_root.clone();
        bootstrap.config_preview = state.config_preview.clone();
        bootstrap.plan_preview = state.plan_preview.clone();
        bootstrap.recent_artifacts = state.recent_artifacts.clone();
    }
}

pub fn launch() -> Result<()> {
    let bootstrap = MakerBootstrap::load()?;
    trace_ui(
        &bootstrap.trace_root,
        "startup",
        "launch_gui",
        json!({
            "saved_setups": bootstrap.saved_setups.len(),
            "current_setup_id": bootstrap.current_setup.setup_id,
        }),
    );
    set_bootstrap(bootstrap);

    #[cfg(target_os = "linux")]
    if let Err(error) = refresh_dev_desktop_integration() {
        if let Some(bootstrap) = cloned_bootstrap() {
            trace_ui(
                &bootstrap.trace_root,
                "startup",
                "refresh_desktop_integration_failed",
                json!({ "error": error.to_string() }),
            );
        }
    }

    #[cfg(target_os = "macos")]
    let window_builder = WindowBuilder::new()
        .with_title("Yggdrasil Maker")
        .with_window_icon(Some(window_icon::load_yggdrasil_maker_window_icon()))
        .with_transparent(false)
        .with_decorations(false)
        .with_resizable(true)
        .with_inner_size(LogicalSize::new(1460.0, 920.0))
        .with_min_inner_size(LogicalSize::new(1120.0, 760.0));

    #[cfg(not(target_os = "macos"))]
    let window_builder = WindowBuilder::new()
        .with_title("Yggdrasil Maker")
        .with_window_icon(Some(window_icon::load_yggdrasil_maker_window_icon()))
        .with_transparent(true)
        .with_decorations(false)
        .with_resizable(true)
        .with_inner_size(LogicalSize::new(1460.0, 920.0))
        .with_min_inner_size(LogicalSize::new(1120.0, 760.0));

    let config = Config::new()
        .with_event_loop(configured_event_loop())
        .with_window(window_builder)
        .with_close_behaviour(WindowCloseBehaviour::WindowCloses)
        .with_exits_when_last_window_closes(true);

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(app);
    Ok(())
}

#[cfg(target_os = "linux")]
fn configured_event_loop() -> EventLoop<UserWindowEvent> {
    let mut builder = EventLoopBuilder::<UserWindowEvent>::with_user_event();
    builder.with_app_id(YGGDRASIL_MAKER_DESKTOP_APP_ID);
    builder.build()
}

#[cfg(not(target_os = "linux"))]
fn configured_event_loop() -> EventLoop<UserWindowEvent> {
    EventLoopBuilder::<UserWindowEvent>::with_user_event().build()
}

#[derive(Clone)]
struct MakerBootstrap {
    app: MakerApp,
    trace_root: PathBuf,
    shell_settings: MakerShellSettings,
    saved_setups: Vec<StoredSetupSummary>,
    current_setup: SetupDocument,
    artifacts_dir: String,
    repo_root: String,
    config_preview: String,
    plan_preview: String,
    recent_artifacts: Vec<RecentArtifactSummary>,
}

impl MakerBootstrap {
    fn load() -> Result<Self> {
        let app = MakerApp::new_for_current_platform()?;
        let trace_root = maker_data_root()?;
        let shell_settings = load_shell_settings().unwrap_or_default();
        let mut saved_setups = app.setup_store().list()?;
        saved_setups.sort_by(|left, right| right.modified_unix_secs.cmp(&left.modified_unix_secs));
        let current_setup = if let Some(first) = saved_setups.first() {
            app.setup_store().load(&first.setup_id)?
        } else {
            app.create_setup_document("Lab NAS".to_owned(), PresetId::Nas, None, None)
        };

        let mut state = MakerUiState::new(app.clone(), trace_root.clone(), shell_settings);
        state.saved_setups = saved_setups.clone();
        state.current_setup = current_setup.clone();
        state.refresh_previews();
        state.refresh_recent_artifacts();

        Ok(Self {
            app,
            trace_root,
            shell_settings: state.shell_settings,
            saved_setups,
            current_setup,
            artifacts_dir: state.artifacts_dir,
            repo_root: state.repo_root,
            config_preview: state.config_preview,
            plan_preview: state.plan_preview,
            recent_artifacts: state.recent_artifacts,
        })
    }
}

#[derive(Clone, PartialEq)]
struct MakerUiState {
    app: MakerApp,
    trace_root: PathBuf,
    shell_settings: MakerShellSettings,
    saved_setups: Vec<StoredSetupSummary>,
    current_setup: SetupDocument,
    artifacts_dir: String,
    repo_root: String,
    config_preview: String,
    plan_preview: String,
    build_log: Vec<String>,
    build_status: String,
    build_result: String,
    build_running: bool,
    right_panel_mode: RightPanelMode,
    sidebar_open: bool,
    collapsed_tree_nodes: BTreeSet<String>,
    utility_pane_open: bool,
    recent_artifacts: Vec<RecentArtifactSummary>,
    recent_artifacts_expanded: bool,
    success_state: Option<BuildSuccessState>,
    notifications: Vec<ToastItem>,
    next_notification_id: u64,
    alt_overlay_active: bool,
    appearance_panel_open: bool,
    theme_editor_draft: YgguiThemeSpec,
    theme_editor_selected_stop: Option<usize>,
    theme_editor_drag_stop: Option<usize>,
    hovered_control: Option<HoveredChromeControl>,
    maximized: bool,
    always_on_top: bool,
    window_width: u32,
}

impl MakerUiState {
    fn new(app: MakerApp, trace_root: PathBuf, shell_settings: MakerShellSettings) -> Self {
        let theme_editor_draft = clamp_theme_spec(&shell_settings.yggui_theme);
        Self {
            app,
            trace_root,
            shell_settings,
            saved_setups: Vec::new(),
            current_setup: SetupDocument::new("New Yggdrasil".to_owned(), PresetId::Nas),
            artifacts_dir: "./artifacts".to_owned(),
            repo_root: String::new(),
            config_preview: String::new(),
            plan_preview: String::new(),
            build_log: Vec::new(),
            build_status: "Ready to build".to_owned(),
            build_result: String::new(),
            build_running: false,
            right_panel_mode: RightPanelMode::Config,
            sidebar_open: true,
            collapsed_tree_nodes: BTreeSet::new(),
            utility_pane_open: true,
            recent_artifacts: Vec::new(),
            recent_artifacts_expanded: false,
            success_state: None,
            notifications: Vec::new(),
            next_notification_id: 1,
            alt_overlay_active: false,
            appearance_panel_open: false,
            theme_editor_draft,
            theme_editor_selected_stop: None,
            theme_editor_drag_stop: None,
            hovered_control: None,
            maximized: false,
            always_on_top: false,
            window_width: 1460,
        }
    }

    fn from_bootstrap(bootstrap: MakerBootstrap) -> Self {
        let mut state = Self::new(
            bootstrap.app,
            bootstrap.trace_root,
            bootstrap.shell_settings.clone(),
        );
        state.saved_setups = bootstrap.saved_setups;
        state.current_setup = bootstrap.current_setup;
        state.artifacts_dir = bootstrap.artifacts_dir;
        state.repo_root = bootstrap.repo_root;
        state.config_preview = bootstrap.config_preview;
        state.plan_preview = bootstrap.plan_preview;
        state.recent_artifacts = bootstrap.recent_artifacts;
        state.recent_artifacts_expanded = !state.recent_artifacts.is_empty();
        state.utility_pane_open = bootstrap.shell_settings.utility_pane_open;
        state.right_panel_mode = bootstrap.shell_settings.right_panel_mode;
        state.sidebar_open = bootstrap.shell_settings.sidebar_open;
        state.collapsed_tree_nodes = BTreeSet::new();
        state.theme_editor_draft = clamp_theme_spec(&state.shell_settings.yggui_theme);
        if bootstrap.shell_settings.right_panel_mode == RightPanelMode::Appearance {
            state.appearance_panel_open = true;
            state.right_panel_mode = RightPanelMode::Appearance;
            state.utility_pane_open = bootstrap.shell_settings.utility_pane_open;
            state.theme_editor_selected_stop = state.theme_editor_draft.colors.first().map(|_| 0);
        } else {
            state.sync_truth_surface_for_stage();
        }
        state
    }

    fn refresh_saved_setups(&mut self) {
        if let Ok(mut setups) = self.app.setup_store().list() {
            setups.sort_by(|left, right| right.modified_unix_secs.cmp(&left.modified_unix_secs));
            self.saved_setups = setups;
        }
    }

    fn toggle_tree_folder(&mut self, key: &str) {
        if !self.collapsed_tree_nodes.insert(key.to_owned()) {
            self.collapsed_tree_nodes.remove(key);
        }
    }

    fn refresh_previews(&mut self) {
        self.config_preview = self
            .app
            .emit_config_toml(&self.current_setup)
            .unwrap_or_else(|error| format!("Config preview unavailable:\n{error}"));
        self.plan_preview = self
            .app
            .plan_build(self.build_inputs())
            .and_then(|plan| serde_json::to_string_pretty(&plan).map_err(|error| error.into()))
            .unwrap_or_else(|error| format!("Build plan unavailable:\n{error}"));
    }

    fn refresh_recent_artifacts(&mut self) {
        self.recent_artifacts = self
            .latest_manifest()
            .map(|manifest| recent_artifact_summaries(&manifest))
            .unwrap_or_default();
    }

    fn latest_manifest(&self) -> Option<ArtifactManifest> {
        let path = Path::new(&self.artifacts_dir).join(ARTIFACT_MANIFEST_NAME);
        if !path.is_file() {
            return None;
        }
        read_artifact_manifest(&path).ok()
    }

    fn build_inputs(&self) -> BuildInputs {
        BuildInputs {
            setup_document: self.current_setup.clone(),
            artifacts_dir: PathBuf::from(self.artifacts_dir.trim()),
            authorized_keys_file: default_authorized_keys_file(&self.current_setup),
            host_keys_dir: None,
            repo_root: if self.repo_root.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(self.repo_root.trim()))
            },
            skip_smoke: false,
        }
    }

    fn persist_shell_settings(&self) {
        let _ = save_shell_settings(&self.shell_settings);
    }

    fn push_notification(
        &mut self,
        tone: ToastTone,
        title: impl Into<String>,
        message: impl Into<String>,
    ) {
        let id = self.next_notification_id;
        self.next_notification_id += 1;
        self.notifications.push(ToastItem {
            id,
            tone,
            title: title.into(),
            message: message.into(),
            created_at_ms: current_millis(),
            job_key: None,
            progress: None,
            persistent: false,
        });
    }

    fn save_current_setup(&mut self) {
        match self.app.setup_store().save(&self.current_setup) {
            Ok(path) => {
                self.build_status = format!("Saved {}", path.display());
                self.right_panel_mode = RightPanelMode::Plan;
                self.utility_pane_open = true;
                self.refresh_saved_setups();
                sync_bootstrap_from_state(self);
                self.push_notification(
                    ToastTone::Success,
                    "Setup Saved",
                    format!("Persisted {}", self.current_setup.setup.name),
                );
                trace_ui(
                    &self.trace_root,
                    "setup",
                    "save",
                    json!({
                        "setup_id": self.current_setup.setup_id,
                        "path": path,
                    }),
                );
            }
            Err(error) => {
                self.build_status = format!("Save failed: {error}");
                self.right_panel_mode = RightPanelMode::Build;
                self.utility_pane_open = true;
                self.push_notification(ToastTone::Error, "Save Failed", error.to_string());
            }
        }
    }

    fn select_setup(&mut self, setup_id: &str) {
        if let Ok(document) = self.app.setup_store().load(setup_id) {
            self.current_setup = document;
            self.success_state = None;
            self.sync_truth_surface_for_stage();
            self.refresh_previews();
            sync_bootstrap_from_state(self);
            trace_ui(
                &self.trace_root,
                "setup",
                "select",
                json!({ "setup_id": setup_id }),
            );
        }
    }

    fn start_another_setup(&mut self) {
        self.current_setup =
            self.app
                .create_setup_document("New Yggdrasil".to_owned(), PresetId::Nas, None, None);
        self.set_journey_stage(JourneyStage::Outcome);
        self.build_status = "Ready to build".to_owned();
        self.build_result.clear();
        self.build_log.clear();
        self.success_state = None;
        self.refresh_previews();
        sync_bootstrap_from_state(self);
        trace_ui(&self.trace_root, "setup", "new", json!({}));
    }

    fn open_build_details(&mut self) {
        self.success_state = None;
        self.appearance_panel_open = false;
        self.right_panel_mode = RightPanelMode::Build;
        self.utility_pane_open = true;
    }

    fn apply_preset(&mut self, preset: PresetId) {
        self.current_setup.setup.preset = preset;
        self.current_setup.setup.profile_override = Some(preset.recommended_profile());
        self.set_journey_stage(JourneyStage::Profile);
        self.success_state = None;
        self.refresh_previews();
        sync_bootstrap_from_state(self);
        trace_ui(
            &self.trace_root,
            "setup",
            "preset",
            json!({
                "setup_id": self.current_setup.setup_id,
                "preset": preset.slug(),
            }),
        );
    }

    fn activate_success_state(&mut self, manifest: &ArtifactManifest) {
        let primary = primary_artifact(manifest)
            .or_else(|| manifest.artifacts.first())
            .cloned();
        let artifact_name = primary
            .as_ref()
            .and_then(|artifact| Path::new(&artifact.path).file_name())
            .and_then(|value| value.to_str())
            .unwrap_or("Artifact")
            .to_owned();
        let artifact_path = primary
            .as_ref()
            .map(|artifact| artifact.path.clone())
            .unwrap_or_else(|| self.artifacts_dir.clone());
        let proof = match manifest.mode {
            BuildMode::LocalDocker => {
                "Verified the build manifest after a local Docker run and copied the resulting artifacts."
            }
            BuildMode::ExportOnly => {
                "Prepared a truthful export bundle for a Linux builder, including native config, persisted setup, and handoff instructions."
            }
        }
        .to_owned();

        self.success_state = Some(BuildSuccessState {
            setup_id: self.current_setup.setup_id.clone(),
            title: if manifest.mode == BuildMode::ExportOnly {
                "Export bundle ready".to_owned()
            } else {
                "Artifact ready".to_owned()
            },
            proof,
            artifact_name,
            artifact_path: artifact_path.clone(),
            profile_label: manifest.build_profile.slug().to_owned(),
            output_path: artifact_path,
        });
        self.set_journey_stage(JourneyStage::Boot);
        self.recent_artifacts_expanded = true;
        sync_bootstrap_from_state(self);
    }

    fn set_journey_stage(&mut self, stage: JourneyStage) {
        self.current_setup.journey_stage = stage;
        self.sync_truth_surface_for_stage();
        sync_bootstrap_from_state(self);
    }

    fn sync_truth_surface_for_stage(&mut self) {
        self.appearance_panel_open = false;
        let mode = default_truth_mode_for_stage(self.current_setup.journey_stage);
        self.right_panel_mode = mode;
        self.utility_pane_open = true;
        self.shell_settings.right_panel_mode = mode;
        self.shell_settings.utility_pane_open = true;
        self.persist_shell_settings();
    }

    fn open_appearance_sidebar(&mut self) {
        self.appearance_panel_open = true;
        self.right_panel_mode = RightPanelMode::Appearance;
        self.utility_pane_open = true;
        self.shell_settings.right_panel_mode = RightPanelMode::Appearance;
        self.shell_settings.utility_pane_open = true;
        self.theme_editor_draft = clamp_theme_spec(&self.shell_settings.yggui_theme);
        self.theme_editor_selected_stop = self.theme_editor_draft.colors.first().map(|_| 0);
        self.theme_editor_drag_stop = None;
        self.persist_shell_settings();
    }

    fn close_appearance_sidebar(&mut self) {
        self.appearance_panel_open = false;
        self.right_panel_mode = default_truth_mode_for_stage(self.current_setup.journey_stage);
        self.shell_settings.right_panel_mode = self.right_panel_mode;
        self.theme_editor_drag_stop = None;
    }

    fn save_theme_editor(&mut self) {
        self.shell_settings.yggui_theme = clamp_theme_spec(&self.theme_editor_draft);
        self.theme_editor_drag_stop = None;
        self.persist_shell_settings();
        self.push_notification(
            ToastTone::Success,
            "Theme Updated",
            "Yggui shell theme applied.".to_owned(),
        );
    }

    fn reset_theme_editor(&mut self) {
        self.theme_editor_draft = default_theme_editor_spec();
        self.theme_editor_selected_stop = self.theme_editor_draft.colors.first().map(|_| 0);
        self.theme_editor_drag_stop = None;
    }

    fn seed_theme_editor(&mut self) {
        self.theme_editor_draft = default_theme_editor_spec();
        self.theme_editor_selected_stop = self.theme_editor_draft.colors.first().map(|_| 0);
        self.theme_editor_drag_stop = None;
    }

    fn add_theme_stop(&mut self, color: Option<&str>) {
        let next = append_theme_stop(&self.theme_editor_draft, color);
        if next.colors.len() == self.theme_editor_draft.colors.len() {
            return;
        }
        self.theme_editor_draft = next;
        self.theme_editor_selected_stop = self.theme_editor_draft.colors.len().checked_sub(1);
    }

    fn add_theme_stop_at(&mut self, x: f32, y: f32) {
        self.add_theme_stop(None);
        if let Some(index) = self.theme_editor_selected_stop
            && let Some(stop) = self.theme_editor_draft.colors.get_mut(index)
        {
            stop.x = x.clamp(0.0, 1.0);
            stop.y = y.clamp(0.0, 1.0);
        }
        self.theme_editor_draft = clamp_theme_spec(&self.theme_editor_draft);
    }

    fn select_theme_stop(&mut self, index: usize) {
        if index < self.theme_editor_draft.colors.len() {
            self.theme_editor_selected_stop = Some(index);
        }
    }

    fn begin_theme_drag(&mut self, index: usize) {
        self.select_theme_stop(index);
        self.theme_editor_drag_stop = Some(index);
    }

    fn move_theme_stop(&mut self, x: f32, y: f32) {
        let Some(index) = self.theme_editor_drag_stop else {
            return;
        };
        if let Some(stop) = self.theme_editor_draft.colors.get_mut(index) {
            stop.x = x.clamp(0.0, 1.0);
            stop.y = y.clamp(0.0, 1.0);
        }
    }

    fn end_theme_drag(&mut self) {
        self.theme_editor_drag_stop = None;
    }

    fn remove_selected_theme_stop(&mut self) {
        let Some(index) = self.theme_editor_selected_stop else {
            return;
        };
        if index >= self.theme_editor_draft.colors.len() {
            return;
        }
        self.theme_editor_draft.colors.remove(index);
        self.theme_editor_selected_stop = if self.theme_editor_draft.colors.is_empty() {
            None
        } else {
            Some(index.min(self.theme_editor_draft.colors.len() - 1))
        };
    }

    fn update_selected_theme_color(&mut self, color: String) {
        let Some(index) = self.theme_editor_selected_stop else {
            return;
        };
        if let Some(stop) = self.theme_editor_draft.colors.get_mut(index) {
            stop.color = color;
        }
        self.theme_editor_draft = clamp_theme_spec(&self.theme_editor_draft);
    }

    fn update_theme_brightness(&mut self, value: f32) {
        self.theme_editor_draft.brightness = value.clamp(0.0, 1.0);
    }

    fn update_theme_grain(&mut self, value: f32) {
        self.theme_editor_draft.grain = value.clamp(0.0, 1.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RightPanelMode {
    Appearance,
    Config,
    Plan,
    Build,
}

impl RightPanelMode {
    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Config => "Config",
            Self::Plan => "Plan",
            Self::Build => "Build",
        }
    }

    fn all() -> [Self; 3] {
        [Self::Config, Self::Plan, Self::Build]
    }
}

impl FromStr for RightPanelMode {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "appearance" => Ok(Self::Appearance),
            "config" => Ok(Self::Config),
            "plan" => Ok(Self::Plan),
            "build" => Ok(Self::Build),
            other => Err(format!("unsupported right panel mode: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BuildMessage {
    Event(String),
    Finished {
        manifest: ArtifactManifest,
        payload: String,
    },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecentArtifactSummary {
    title: String,
    subtitle: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildSuccessState {
    setup_id: String,
    title: String,
    proof: String,
    artifact_name: String,
    artifact_path: String,
    profile_label: String,
    output_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SidebarTreeRow {
    Folder {
        key: String,
        label: String,
        depth: usize,
        expanded: bool,
    },
    Setup {
        key: String,
        setup_id: String,
        label: String,
        depth: usize,
        selected: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct MakerShellSettings {
    theme: UiTheme,
    yggui_theme: YgguiThemeSpec,
    finish: ShellFinish,
    sidebar_open: bool,
    utility_pane_open: bool,
    right_panel_mode: RightPanelMode,
}

impl Default for MakerShellSettings {
    fn default() -> Self {
        Self {
            theme: UiTheme::ZedLight,
            yggui_theme: theme_spec_for_preset(ThemePreset::ArcFrost),
            finish: ShellFinish::Sleek,
            sidebar_open: true,
            utility_pane_open: true,
            right_panel_mode: RightPanelMode::Config,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ShellFinish {
    Sleek,
    Crisp,
}

impl ShellFinish {
    fn label(self) -> &'static str {
        match self {
            Self::Sleek => "Sleek",
            Self::Crisp => "Crisp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemePreset {
    ArcFrost,
    ArcMint,
    ArcSlate,
}

impl ThemePreset {
    fn label(self) -> &'static str {
        match self {
            Self::ArcFrost => "Arc Frost",
            Self::ArcMint => "Arc Mint",
            Self::ArcSlate => "Arc Slate",
        }
    }

    fn all() -> [Self; 3] {
        [Self::ArcFrost, Self::ArcMint, Self::ArcSlate]
    }
}

fn app() -> Element {
    let bootstrap =
        cloned_bootstrap().expect("maker bootstrap should be initialized before launch");
    let mut state = use_signal(|| MakerUiState::from_bootstrap(bootstrap));
    let mount_generation = use_hook(|| APP_MOUNT_GENERATION.fetch_add(1, Ordering::Relaxed) + 1);
    let desktop = use_window();
    let schedule_ui_update = schedule_update();
    let now_ms = use_signal(current_millis);
    let window_icon_applied =
        use_hook(|| Arc::new(std::sync::atomic::AtomicBool::new(false))).clone();

    {
        let desktop = desktop.clone();
        let window_icon_applied = window_icon_applied.clone();
        use_effect(move || {
            if window_icon_applied.swap(true, Ordering::SeqCst) {
                return;
            }
            let desktop = desktop.clone();
            let window_icon_applied = window_icon_applied.clone();
            spawn(async move {
                sleep(Duration::from_millis(250)).await;
                desktop
                    .window
                    .set_window_icon(Some(window_icon::load_yggdrasil_maker_window_icon()));
                #[cfg(target_os = "linux")]
                {
                    let pixbuf = window_icon::load_yggdrasil_maker_pixbuf();
                    gtk::Window::set_default_icon(&pixbuf);
                    gtk::Window::set_default_icon_name(YGGDRASIL_MAKER_WM_CLASS);
                    let gtk_window = desktop.window.gtk_window();
                    gtk_window.set_icon(Some(&pixbuf));
                    gtk_window.set_icon_name(Some(YGGDRASIL_MAKER_WM_CLASS));
                }
                let _ = window_icon_applied;
            });
        });
    }

    {
        let mut now_ms = now_ms;
        use_future(move || async move {
            loop {
                if APP_MOUNT_GENERATION.load(Ordering::Relaxed) != mount_generation {
                    break;
                }
                sleep(Duration::from_millis(250)).await;
                now_ms.set(current_millis());
            }
        });
    }

    {
        use_future(move || async move {
            loop {
                if APP_MOUNT_GENERATION.load(Ordering::Relaxed) != mount_generation {
                    break;
                }
                let maximized = window().is_maximized();
                let window_width = window().inner_size().width;
                state.with_mut(|ui| {
                    if ui.maximized != maximized {
                        ui.maximized = maximized;
                    }
                    if ui.window_width != window_width {
                        ui.window_width = window_width;
                    }
                });
                sleep(Duration::from_millis(160)).await;
            }
        });
    }

    {
        use_future(move || async move {
            loop {
                if APP_MOUNT_GENERATION.load(Ordering::Relaxed) != mount_generation {
                    break;
                }
                let snapshot = state.read().clone();
                sync_bootstrap_from_state(&snapshot);
                sleep(Duration::from_millis(120)).await;
            }
        });
    }

    {
        let desktop = desktop.clone();
        let trace_root = state.read().trace_root.clone();
        let wake_app_control = desktop.poll_waker();
        let schedule_ui_update = schedule_ui_update.clone();
        use_future(move || {
            let desktop = desktop.clone();
            let trace_root = trace_root.clone();
            let wake_app_control = wake_app_control.clone();
            let schedule_ui_update = schedule_ui_update.clone();
            async move {
                let _ = register_client_instance(&trace_root);
                loop {
                    if APP_MOUNT_GENERATION.load(Ordering::Relaxed) != mount_generation {
                        break;
                    }
                    match process_pending_app_control_requests(&trace_root, &desktop, state).await {
                        Ok(true) => {
                            wake_app_control();
                            schedule_ui_update();
                        }
                        Ok(false) => {}
                        Err(error) => {
                            trace_ui(
                                &trace_root,
                                "app-control",
                                "request_error",
                                json!({ "error": error.to_string() }),
                            );
                        }
                    }
                    sleep(Duration::from_millis(80)).await;
                }
            }
        });
    }

    let snapshot = state.read().clone();
    let active_theme_spec = if snapshot.appearance_panel_open {
        clamp_theme_spec(&snapshot.theme_editor_draft)
    } else {
        clamp_theme_spec(&snapshot.shell_settings.yggui_theme)
    };
    let accent = dominant_accent(&active_theme_spec, "#72bef7");
    let shell_gradient = gradient_css(snapshot.shell_settings.theme, &active_theme_spec);
    let shell_tint_fill = shell_tint(snapshot.shell_settings.theme, &active_theme_spec);
    let preview_surface = preview_surface_css(snapshot.shell_settings.theme, &active_theme_spec);
    let is_dark = is_dark_theme(snapshot.shell_settings.theme);
    let blur_supported = supports_live_blur();
    let chrome_palette = chrome_palette(is_dark, &accent);
    let toast_palette = toast_palette(is_dark, &accent);
    let theme_vars = theme_css_variables(snapshot.shell_settings.theme, &accent, blur_supported);
    let sidebar_tree_rows = build_sidebar_tree_rows(&snapshot);

    let titlebar_left = rsx! {
        div {
            style: "display:flex; align-items:center; gap:12px; min-width:0; width:100%; padding-left:4px;",
            button {
                style: titlebar_icon_button_style(snapshot.sidebar_open),
                onmousedown: |evt| evt.stop_propagation(),
                ondoubleclick: |evt| evt.stop_propagation(),
                onclick: move |_| {
                    state.with_mut(|ui| {
                        ui.sidebar_open = !ui.sidebar_open;
                        ui.shell_settings.sidebar_open = ui.sidebar_open;
                        ui.persist_shell_settings();
                    });
                },
                "☰"
            }
            button {
                style: titlebar_setup_button_style(),
                onmousedown: |evt| evt.stop_propagation(),
                ondoubleclick: |evt| evt.stop_propagation(),
                onclick: move |_| {
                    state.with_mut(|ui| {
                        ui.set_journey_stage(JourneyStage::Personalize);
                    });
                    let _ = document::eval("document.getElementById('maker-setup-name')?.focus?.();");
                },
                div {
                    style: "display:flex; align-items:center; justify-content:space-between; gap:8px; width:100%; min-width:0;",
                    span {
                        style: "min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:12px; font-weight:700; color:var(--maker-titlebar-text); text-align:left;",
                        "{sidebar_setup_primary(&snapshot.current_setup.setup.name)}"
                    }
                    span {
                        style: "flex:0 0 auto; font-size:10px; color:var(--maker-titlebar-muted); white-space:nowrap;",
                        "{snapshot.current_setup.setup.preset.slug()} • {profile_title_label(snapshot.current_setup.setup.profile_override.unwrap_or_else(|| snapshot.current_setup.setup.preset.recommended_profile()))}"
                    }
                }
            }
        }
    };

    let titlebar_center = rsx! {
        div {
            style: "display:flex; align-items:center; justify-content:center; gap:10px; min-width:0; width:min(520px, 100%);",
            div {
                style: titlebar_center_field_style(),
                span {
                    style: "font-size:11px; font-weight:800; letter-spacing:0.08em; color:var(--maker-titlebar-muted); white-space:nowrap;",
                    "{snapshot.current_setup.journey_stage.label()}"
                }
                span {
                    style: "font-size:11px; font-weight:700; color:var(--maker-titlebar-text); white-space:nowrap;",
                    "{snapshot.current_setup.setup.profile_override.unwrap_or_else(|| snapshot.current_setup.setup.preset.recommended_profile())}"
                }
                span {
                    style: "font-size:11px; color:var(--maker-titlebar-muted); white-space:nowrap; overflow:hidden; text-overflow:ellipsis;",
                    "{titlebar_status_text(&snapshot)}"
                }
            }
        }
    };

    let titlebar_right = rsx! {
        div {
            style: "display:flex; align-items:center; justify-content:flex-end; gap:8px; min-width:0; width:100%;",
            button {
                title: "Appearance",
                style: utility_icon_button_style(snapshot.right_panel_mode == RightPanelMode::Appearance),
                onmousedown: |evt| evt.stop_propagation(),
                ondoubleclick: |evt| evt.stop_propagation(),
                onclick: move |_| {
                    state.with_mut(|ui| {
                        if ui.appearance_panel_open && ui.utility_pane_open {
                            ui.utility_pane_open = false;
                            ui.shell_settings.utility_pane_open = false;
                            ui.close_appearance_sidebar();
                            ui.persist_shell_settings();
                        } else {
                            ui.open_appearance_sidebar();
                        }
                    });
                },
                svg {
                    width: "14",
                    height: "14",
                    view_box: "0 0 14 14",
                    fill: "none",
                    xmlns: "http://www.w3.org/2000/svg",
                    path { d: "M7 2.2C4.24 2.2 2 4.44 2 7.2C2 9.42 3.47 10.86 5.37 10.86H6.22C6.82 10.86 7.3 11.34 7.3 11.94C7.3 12.24 7.55 12.5 7.86 12.5H8.2C10.96 12.5 13.2 10.26 13.2 7.5C13.2 4.54 10.91 2.2 7.95 2.2H7Z", stroke: "currentColor", stroke_width: "1.05", stroke_linejoin: "round" }
                    circle { cx: "4.5", cy: "6.1", r: "0.7", fill: "currentColor" }
                    circle { cx: "6.9", cy: "4.8", r: "0.7", fill: "currentColor" }
                    circle { cx: "9.4", cy: "5.9", r: "0.7", fill: "currentColor" }
                }
            }
            button {
                title: "Shell Truth",
                style: utility_icon_button_style(snapshot.utility_pane_open && snapshot.right_panel_mode != RightPanelMode::Appearance),
                onmousedown: |evt| evt.stop_propagation(),
                ondoubleclick: |evt| evt.stop_propagation(),
                onclick: move |_| {
                    state.with_mut(|ui| {
                        if ui.utility_pane_open && !ui.appearance_panel_open {
                            ui.utility_pane_open = false;
                            ui.shell_settings.utility_pane_open = false;
                        } else {
                            ui.utility_pane_open = true;
                            ui.shell_settings.utility_pane_open = true;
                            ui.appearance_panel_open = false;
                            ui.right_panel_mode = default_truth_mode_for_stage(ui.current_setup.journey_stage);
                            ui.shell_settings.right_panel_mode = ui.right_panel_mode;
                        }
                        ui.persist_shell_settings();
                    });
                },
                if snapshot.alt_overlay_active {
                    span { style: shortcut_badge_style(), "T" }
                }
                svg {
                    width: "14",
                    height: "14",
                    view_box: "0 0 14 14",
                    fill: "none",
                    xmlns: "http://www.w3.org/2000/svg",
                    rect { x: "2.3", y: "2.5", width: "9.4", height: "8.9", rx: "1.6", stroke: "currentColor", stroke_width: "1.05" }
                    path { d: "M5 2.7V11.2", stroke: "currentColor", stroke_width: "1.05" }
                    path { d: "M6.7 5.2H10.3", stroke: "currentColor", stroke_width: "1.05", stroke_linecap: "round" }
                    path { d: "M6.7 7.1H9.7", stroke: "currentColor", stroke_width: "1.05", stroke_linecap: "round" }
                    path { d: "M6.7 9H10", stroke: "currentColor", stroke_width: "1.05", stroke_linecap: "round" }
                }
            }
            div { style: "flex:1; min-width:14px; max-width:26px; height:28px;" }
            WindowControlsStrip {
                palette: chrome_palette,
                hovered: snapshot.hovered_control,
                maximized: snapshot.maximized,
                fullscreen: false,
                always_on_top: snapshot.always_on_top,
                show_always_on_top_button: true,
                show_fullscreen_button: false,
                show_window_buttons: true,
                overlay: false,
                on_hover_control: move |control: Option<HoveredChromeControl>| {
                    state.with_mut(|ui| ui.hovered_control = control);
                },
                on_toggle_maximized: move |_| toggle_maximized(state),
                on_toggle_fullscreen: move |_| {},
                on_toggle_always_on_top: move |_| {
                    state.with_mut(|ui| {
                        ui.always_on_top = !ui.always_on_top;
                        window().set_always_on_top(ui.always_on_top);
                    });
                },
                on_close_app: move |_| {
                    window().close();
                },
            }
        }
    };

    rsx! {
        style { "{TOAST_CSS}" }
        style { "{MAKER_MOTION_CSS}" }
        style { {format!("html, body, #main {{ margin:0; width:100%; height:100%; background:transparent !important; overflow:hidden; }} body {{ overscroll-behavior:none; font-family:{}; }}", UI_FONT_FAMILY)} }
        div {
            id: "maker-shell-root",
            tabindex: "0",
            onkeydown: move |evt| handle_keydown(evt, state),
            onkeyup: move |evt| handle_keyup(evt, state),
            oncontextmenu: |evt| {
                evt.prevent_default();
                evt.stop_propagation();
            },
            style: format!(
                "position:relative; width:100vw; height:100vh; overflow:hidden; background:transparent; font-family:{}; {};",
                UI_FONT_FAMILY, theme_vars,
            ),
            if !snapshot.maximized {
                WindowResizeHandles {}
            }
            div {
                style: shell_surface_style(
                    snapshot.maximized,
                    snapshot.shell_settings.finish,
                    &shell_tint_fill,
                    &shell_gradient,
                    blur_supported,
                ),
                TitlebarChrome {
                    background: "transparent".to_owned(),
                    zoom_percent: 100.0,
                    left: titlebar_left,
                    center: titlebar_center,
                    right: titlebar_right,
                    on_toggle_maximized: move |_| toggle_maximized(state),
                }
                div {
                    style: "display:flex; flex:1; min-height:0; overflow:hidden;",
                    SideRailShell {
                        visible: snapshot.sidebar_open,
                        width_px: LEFT_RAIL_WIDTH,
                        zoom_percent: 100.0,
                        body: rsx! {
                            div {
                                style: left_rail_container_style(),
                                RailHeader {
                                    title: "Setups".to_owned(),
                                    color: if is_dark {
                                        "#d1dfec".to_owned()
                                    } else {
                                        "#5f748b".to_owned()
                                    },
                                }
                                RailScrollBody {
                                    content: rsx! {
                                        div {
                                            style: "display:flex; flex-direction:column; gap:14px; padding:6px 0 4px 0;",
                                            button {
                                                style: primary_rail_button_style(&accent),
                                                onclick: move |_| {
                                                    state.with_mut(|ui| ui.start_another_setup());
                                                    let _ = document::eval("document.getElementById('maker-setup-name')?.focus?.();");
                                                },
                                                if snapshot.alt_overlay_active {
                                                    span { style: shortcut_badge_style(), "N" }
                                                }
                                                "New Setup"
                                            }
                                        }
                                        div {
                                            style: "display:flex; flex-direction:column; gap:7px;",
                                            for row in sidebar_tree_rows.iter().cloned() {
                                                match row {
                                                    SidebarTreeRow::Folder { key, label, depth, expanded } => rsx! {
                                                        button {
                                                            key: "{key}",
                                                            style: tree_folder_row_style(depth),
                                                            onclick: {
                                                                let folder_key = key.clone();
                                                                move |_| state.with_mut(|ui| ui.toggle_tree_folder(&folder_key))
                                                            },
                                                            FolderTreeIcon { expanded }
                                                            span { style: tree_folder_label_style(), "{label}" }
                                                        }
                                                    },
                                                    SidebarTreeRow::Setup { key, setup_id, label, depth, selected } => rsx! {
                                                        button {
                                                            key: "{key}",
                                                            style: rail_setup_card_style(selected, depth),
                                                            onclick: move |_| state.with_mut(|ui| ui.select_setup(&setup_id)),
                                                            ReleaseLeafIcon { selected }
                                                            span { style: rail_setup_label_style(selected), "{label}" }
                                                        }
                                                    },
                                                }
                                            }
                                        }
                                        div {
                                            style: "display:flex; flex-direction:column; gap:8px;",
                                            button {
                                                style: section_toggle_style(snapshot.recent_artifacts_expanded),
                                                disabled: snapshot.recent_artifacts.is_empty(),
                                                onclick: move |_| {
                                                    state.with_mut(|ui| {
                                                        if !ui.recent_artifacts.is_empty() {
                                                            ui.recent_artifacts_expanded = !ui.recent_artifacts_expanded;
                                                        }
                                                    });
                                                },
                                                span { "Recent Artifacts" }
                                                span {
                                                    style: "font-size:11px; color:var(--maker-muted);",
                                                    if snapshot.recent_artifacts_expanded { "▾" } else { "▸" }
                                                }
                                            }
                                            if snapshot.recent_artifacts_expanded {
                                                if snapshot.recent_artifacts.is_empty() {
                                                    div {
                                                        style: empty_note_style(),
                                                        "No artifact manifests yet."
                                                    }
                                                } else {
                                                    for artifact in snapshot.recent_artifacts.iter().cloned() {
                                                        button {
                                                            style: rail_meta_card_style(),
                                                            onclick: {
                                                                let path = artifact.path.clone();
                                                                move |_| {
                                                                    let _ = reveal_path(&path);
                                                                }
                                                            },
                                                            div { style: "font-size:12px; font-weight:700; color:var(--maker-text-strong); text-align:left;", "{artifact.title}" }
                                                            div { style: "font-size:11px; color:var(--maker-muted); text-align:left;", "{artifact.subtitle}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                div {
                                    style: "position:absolute; left:0; right:0; bottom:0; height:30px; pointer-events:none; \
                                        background:linear-gradient(180deg, rgba(255,255,255,0) 0%, color-mix(in srgb, var(--maker-section-bg) 82%, transparent) 100%);"
                                }
                            }
                        }
                    }
                    div {
                        style: "flex:1; min-width:0; min-height:0; overflow:auto; padding:18px 20px 20px 20px;",
                        if let Some(success) = snapshot.success_state.clone() {
                            if success.setup_id == snapshot.current_setup.setup_id {
                                SuccessScreen {
                                    success: success,
                                    accent: accent.clone(),
                                    preview_surface: preview_surface.clone(),
                                    on_reveal: move |_| {
                                        let success = state.read().success_state.clone();
                                        if let Some(success) = success {
                                            let _ = reveal_path(&success.output_path);
                                            state.with_mut(|ui| {
                                                ui.push_notification(
                                                    ToastTone::Info,
                                                    "Revealed Artifact",
                                                    success.output_path.clone(),
                                                );
                                                trace_ui(
                                                    &ui.trace_root,
                                                    "artifact",
                                                    "reveal",
                                                    json!({ "path": success.output_path }),
                                                );
                                            });
                                        }
                                    },
                                    on_open_details: move |_| state.with_mut(|ui| ui.open_build_details()),
                                    on_start_another: move |_| {
                                        state.with_mut(|ui| ui.start_another_setup());
                                        let _ = document::eval("document.getElementById('maker-setup-name')?.focus?.();");
                                    },
                                }
                            } else {
                                StudioCanvas {
                                    state: snapshot.clone(),
                                    accent: accent.clone(),
                                    on_set_stage: move |stage: JourneyStage| state.with_mut(|ui| ui.set_journey_stage(stage)),
                                    on_update_setup_name: move |value: String| update_setup_name(state, value),
                                    on_update_hostname: move |value: String| update_hostname(state, value),
                                    on_update_artifacts_dir: move |value: String| update_artifacts_dir(state, value),
                                    on_update_repo_root: move |value: String| update_repo_root(state, value),
                                    on_apply_preset: move |preset: PresetId| state.with_mut(|ui| ui.apply_preset(preset)),
                                    on_select_profile: move |profile: BuildProfile| update_profile(state, profile),
                                    on_toggle_nvidia: move |_| toggle_nvidia(state),
                                    on_toggle_lts: move |_| toggle_lts(state),
                                    on_save: move |_| state.with_mut(|ui| ui.save_current_setup()),
                                    on_build: move |_| start_build(state),
                                }
                            }
                        } else {
                            StudioCanvas {
                                state: snapshot.clone(),
                                accent: accent.clone(),
                                on_set_stage: move |stage: JourneyStage| state.with_mut(|ui| ui.set_journey_stage(stage)),
                                on_update_setup_name: move |value: String| update_setup_name(state, value),
                                on_update_hostname: move |value: String| update_hostname(state, value),
                                on_update_artifacts_dir: move |value: String| update_artifacts_dir(state, value),
                                on_update_repo_root: move |value: String| update_repo_root(state, value),
                                on_apply_preset: move |preset: PresetId| state.with_mut(|ui| ui.apply_preset(preset)),
                                on_select_profile: move |profile: BuildProfile| update_profile(state, profile),
                                on_toggle_nvidia: move |_| toggle_nvidia(state),
                                on_toggle_lts: move |_| toggle_lts(state),
                                on_save: move |_| state.with_mut(|ui| ui.save_current_setup()),
                                on_build: move |_| start_build(state),
                            }
                        }
                    }
                    SideRailShell {
                        visible: snapshot.utility_pane_open,
                        width_px: RIGHT_RAIL_WIDTH,
                        zoom_percent: 100.0,
                        body: rsx! {
                            div {
                                style: right_rail_container_style(),
                                if snapshot.right_panel_mode == RightPanelMode::Appearance {
                                    RailHeader {
                                        title: "Appearance".to_owned(),
                                        color: if is_dark {
                                            "#cbd9e6".to_owned()
                                        } else {
                                            "#657b92".to_owned()
                                        },
                                    }
                                    RailScrollBody {
                                        content: rsx! {
                                            AppearanceSidebar {
                                                accent: accent.clone(),
                                                shell_settings: snapshot.shell_settings.clone(),
                                                theme_draft: snapshot.theme_editor_draft.clone(),
                                                selected_stop: snapshot.theme_editor_selected_stop,
                                                preview_surface: preview_surface.clone(),
                                                on_select_preset: move |preset: ThemePreset| state.with_mut(|ui| {
                                                    ui.theme_editor_draft = theme_spec_for_preset(preset);
                                                    ui.theme_editor_selected_stop = ui.theme_editor_draft.colors.first().map(|_| 0);
                                                }),
                                                on_select_finish: move |finish: ShellFinish| state.with_mut(|ui| {
                                                    ui.shell_settings.finish = finish;
                                                    ui.persist_shell_settings();
                                                }),
                                                on_select_theme: move |theme: UiTheme| state.with_mut(|ui| {
                                                    ui.shell_settings.theme = theme;
                                                    ui.persist_shell_settings();
                                                }),
                                                on_begin_drag_stop: move |index: usize| state.with_mut(|ui| ui.begin_theme_drag(index)),
                                                on_drag_stop: move |(x, y): (f32, f32)| state.with_mut(|ui| ui.move_theme_stop(x, y)),
                                                on_end_drag_stop: move |_| state.with_mut(|ui| ui.end_theme_drag()),
                                                on_double_click_pad: move |(x, y): (f32, f32)| state.with_mut(|ui| ui.add_theme_stop_at(x, y)),
                                                on_pick_stop: move |index: usize| state.with_mut(|ui| ui.select_theme_stop(index)),
                                                on_pick_swatch: move |color: String| state.with_mut(|ui| ui.update_selected_theme_color(color)),
                                                on_update_stop_color: move |color: String| state.with_mut(|ui| ui.update_selected_theme_color(color)),
                                                on_set_brightness: move |value: f32| state.with_mut(|ui| ui.update_theme_brightness(value)),
                                                on_set_grain: move |value: f32| state.with_mut(|ui| ui.update_theme_grain(value)),
                                                on_add_stop: move |_| state.with_mut(|ui| ui.add_theme_stop(None)),
                                                on_remove_stop: move |_| state.with_mut(|ui| ui.remove_selected_theme_stop()),
                                                on_reset: move |_| state.with_mut(|ui| ui.reset_theme_editor()),
                                                on_seed: move |_| state.with_mut(|ui| ui.seed_theme_editor()),
                                                on_save: move |_| state.with_mut(|ui| ui.save_theme_editor()),
                                            }
                                        }
                                    }
                                } else {
                                    RailHeader {
                                        title: "Shell Truth".to_owned(),
                                        color: if is_dark {
                                            "#cbd9e6".to_owned()
                                        } else {
                                            "#657b92".to_owned()
                                        },
                                    }
                                    div {
                                        style: "display:flex; gap:12px; padding:0 16px 8px 16px; border-bottom:1px solid var(--maker-card-border);",
                                        for mode in RightPanelMode::all() {
                                            button {
                                                style: utility_tab_style(snapshot.right_panel_mode == mode, &accent),
                                                onclick: move |_| {
                                                    state.with_mut(|ui| {
                                                        ui.right_panel_mode = mode;
                                                        ui.shell_settings.right_panel_mode = mode;
                                                        ui.persist_shell_settings();
                                                    });
                                                },
                                                "{mode.label()}"
                                            }
                                        }
                                    }
                                    RailScrollBody {
                                        content: rsx! {
                                            if snapshot.right_panel_mode == RightPanelMode::Config {
                                                RailSectionTitle {
                                                    title: "Native Config".to_owned(),
                                                    muted_color: if is_dark {
                                                        "#afc0d1".to_owned()
                                                    } else {
                                                        "#75889c".to_owned()
                                                    },
                                                }
                                                pre {
                                                    style: pre_panel_style(),
                                                    "{snapshot.config_preview}"
                                                }
                                            }
                                            if snapshot.right_panel_mode == RightPanelMode::Plan {
                                                RailSectionTitle {
                                                    title: "Build Plan".to_owned(),
                                                    muted_color: if is_dark {
                                                        "#afc0d1".to_owned()
                                                    } else {
                                                        "#75889c".to_owned()
                                                    },
                                                }
                                                pre {
                                                    style: pre_panel_style(),
                                                    "{snapshot.plan_preview}"
                                                }
                                            }
                                            if snapshot.right_panel_mode == RightPanelMode::Build {
                                                RailSectionTitle {
                                                    title: "Build Stream".to_owned(),
                                                    muted_color: if is_dark {
                                                        "#afc0d1".to_owned()
                                                    } else {
                                                        "#75889c".to_owned()
                                                    },
                                                }
                                                div {
                                                    style: rail_status_card_style(),
                                                    div { style: "font-size:12px; font-weight:700; color:var(--maker-status-text);", "{snapshot.build_status}" }
                                                    div { style: "font-size:11px; line-height:1.5; color:var(--maker-status-muted);", "{build_summary(&snapshot)}" }
                                                }
                                                if !snapshot.build_result.trim().is_empty() {
                                                    RailSectionTitle {
                                                        title: "Artifact Manifest".to_owned(),
                                                        muted_color: if is_dark {
                                                            "#afc0d1".to_owned()
                                                        } else {
                                                            "#75889c".to_owned()
                                                        },
                                                    }
                                                    pre {
                                                        style: pre_panel_style(),
                                                        "{snapshot.build_result}"
                                                    }
                                                }
                                                if snapshot.build_log.is_empty() {
                                                    div {
                                                        style: rail_empty_note_style(),
                                                        "Logs will appear here once the build starts."
                                                    }
                                                } else {
                                                    RailSectionTitle {
                                                        title: "Live Output".to_owned(),
                                                        muted_color: if is_dark {
                                                            "#afc0d1".to_owned()
                                                        } else {
                                                            "#75889c".to_owned()
                                                        },
                                                    }
                                                    pre {
                                                        style: pre_panel_style(),
                                                        "{snapshot.build_log.join(\"\\n\")}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                ToastViewport {
                    items: snapshot.notifications.clone(),
                    palette: toast_palette,
                    center_offset: 0,
                    max_age_ms: 6_500,
                    max_visible: 4,
                    now_ms: now_ms(),
                    on_clear: move |id: u64| {
                        state.with_mut(|ui| ui.notifications.retain(|item| item.id != id));
                    },
                }
            }
        }
    }
}

#[component]
fn StudioCanvas(
    state: MakerUiState,
    accent: String,
    on_set_stage: EventHandler<JourneyStage>,
    on_update_setup_name: EventHandler<String>,
    on_update_hostname: EventHandler<String>,
    on_update_artifacts_dir: EventHandler<String>,
    on_update_repo_root: EventHandler<String>,
    on_apply_preset: EventHandler<PresetId>,
    on_select_profile: EventHandler<BuildProfile>,
    on_toggle_nvidia: EventHandler<()>,
    on_toggle_lts: EventHandler<()>,
    on_save: EventHandler<()>,
    on_build: EventHandler<()>,
) -> Element {
    let current_stage = state.current_setup.journey_stage;
    let compact_studio = state.window_width < 1280;
    let stacked_studio = state.window_width < 1340;
    let selected_profile = state
        .current_setup
        .setup
        .profile_override
        .unwrap_or_else(|| state.current_setup.setup.preset.recommended_profile());
    let selected_preset = preset_cards()
        .iter()
        .find(|card| card.id == state.current_setup.setup.preset)
        .copied();
    let previous_stage = previous_journey_stage(current_stage);
    let next_stage = next_journey_stage(current_stage);
    let (stage_title, stage_copy) = stage_headline(current_stage);
    let stage_index = journey_stage_index(current_stage) + 1;
    let stage_total = journey_stages().len();
    let hero_compact = current_stage != JourneyStage::Outcome;
    let header_split_style = if compact_studio {
        "display:grid; grid-template-columns:minmax(0, 1fr); gap:14px; align-items:start;"
    } else {
        "display:grid; grid-template-columns:minmax(0, 1.15fr) minmax(220px, 0.85fr); gap:18px; align-items:center;"
    };
    let outcome_grid_style = if compact_studio {
        "display:grid; grid-template-columns:minmax(0, 1fr); gap:14px; align-items:start;"
    } else {
        "display:grid; grid-template-columns:minmax(0, 1.08fr) minmax(260px, 0.92fr); gap:14px; align-items:start;"
    };
    let stage_split_style = if stacked_studio {
        "display:grid; grid-template-columns:minmax(0, 1fr); gap:14px; align-items:start;"
    } else {
        "display:grid; grid-template-columns:minmax(0, 1fr) minmax(250px, 0.78fr); gap:14px; align-items:start;"
    };
    let build_split_style = if stacked_studio {
        "display:grid; grid-template-columns:minmax(0, 1fr); gap:14px; align-items:start;"
    } else {
        "display:grid; grid-template-columns:minmax(0, 1fr) minmax(250px, 0.76fr); gap:14px; align-items:start;"
    };

    rsx! {
        div {
            style: "display:flex; flex-direction:column; gap:14px; max-width:920px; margin:0 auto;",
            div {
                style: viewport_header_style(hero_compact),
                div {
                    style: header_split_style,
                    div {
                        style: "display:flex; flex-direction:column; gap:0;",
                        div {
                            style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap;",
                            div {
                                style: format!("font-size:11px; font-weight:800; letter-spacing:0.08em; color:{};", accent),
                                "{current_stage.label()} STAGE"
                            }
                            div {
                                style: "display:inline-flex; align-items:center; gap:8px; padding:5px 9px; border-radius:999px; background:color-mix(in srgb, var(--maker-card-bg) 62%, transparent); box-shadow:inset 0 0 0 1px color-mix(in srgb, var(--maker-card-border) 82%, transparent); font-size:11px; font-weight:700; color:var(--maker-copy);",
                                "Step {stage_index} of {stage_total}"
                            }
                        }
                        h1 {
                            style: if hero_compact {
                                "margin:6px 0 4px 0; font-size:28px; line-height:1.08; color:var(--maker-hero-title);"
                            } else {
                                "margin:8px 0 6px 0; font-size:38px; line-height:1.04; color:var(--maker-hero-title);"
                            },
                            "{stage_title}"
                        }
                        p {
                            style: if hero_compact {
                                "margin:0; max-width:760px; font-size:14px; line-height:1.65; color:var(--maker-hero-copy);"
                            } else {
                                "margin:0; max-width:720px; font-size:15px; line-height:1.7; color:var(--maker-hero-copy);"
                            },
                            "{stage_copy}"
                        }
                        div {
                            style: "display:flex; align-items:center; gap:8px; margin-top:10px; font-size:12px; font-weight:700; color:var(--maker-copy);",
                            span {
                                style: format!("display:inline-flex; width:8px; height:8px; border-radius:999px; background:{}; box-shadow:0 0 0 6px color-mix(in srgb, {} 12%, transparent); animation:makerProgressSweep 2.6s ease-in-out infinite; transform-origin:center;", accent, accent),
                            }
                            "{stage_reassurance_copy(current_stage)}"
                        }
                        div {
                            style: format!(
                                "display:flex; flex-wrap:wrap; gap:10px; margin-top:{}px;",
                                if hero_compact { 12 } else { 16 }
                            ),
                            div { style: header_meta_chip_style(), span { style: stat_label_style(), "Setup" } span { style: stat_value_style(), "{state.current_setup.setup.name}" } }
                            div { style: header_meta_chip_style(), span { style: stat_label_style(), "Preset" } span { style: stat_value_style(), "{selected_preset.map(|card| card.title).unwrap_or(\"Unknown\")}" } }
                            div { style: header_meta_chip_style(), span { style: stat_label_style(), "Profile" } span { style: stat_value_style(), "{selected_profile.slug()}" } }
                        }
                    }
                    StageCartoon {
                        stage: current_stage,
                        accent: accent.clone(),
                        compact: compact_studio,
                    }
                }
            }

            div {
                style: "display:flex; flex-wrap:wrap; gap:16px; align-items:center; padding:0 4px 2px 4px; border-bottom:1px solid var(--maker-card-border);",
                for stage in journey_stages() {
                    button {
                        style: stage_pill_style(stage == current_stage, stage_precedes(stage, current_stage), &accent),
                        onclick: move |_| on_set_stage.call(stage),
                        "{stage.label()}"
                    }
                }
            }

            if current_stage == JourneyStage::Outcome {
                div {
                    style: section_card_style(),
                    div {
                        style: "display:flex; align-items:end; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                        div {
                            style: "display:flex; flex-direction:column; gap:6px;",
                            h2 { style: section_title_style(), "Outcome" }
                            p { style: section_copy_style(), "Pick the machine you are trying to make real. The selected intent should feel concrete before you move deeper into the build." }
                        }
                        div {
                            style: "font-size:11px; font-weight:700; color:var(--maker-note);",
                            "Choose once, then tune posture and identity."
                        }
                    }
                    div {
                        style: outcome_grid_style,
                        div {
                            style: selected_intent_card_style(&accent),
                            div {
                                style: "display:flex; align-items:center; justify-content:space-between; gap:8px;",
                                span { style: label_style(), "Selected intent" }
                                span { style: format!("font-size:10px; font-weight:800; color:{};", accent), "{selected_profile.slug()}" }
                            }
                            h3 {
                                style: "margin:0; font-size:28px; line-height:1.04; color:var(--maker-section-title);",
                                "{selected_preset.map(|card| card.title).unwrap_or(\"Unknown\")}"
                            }
                            p {
                                style: "margin:0; font-size:14px; line-height:1.72; color:var(--maker-copy);",
                                "{selected_preset.map(|card| card.summary).unwrap_or(\"No preset copy available.\")}"
                            }
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(120px, 1fr)); gap:10px;",
                                div { style: proof_card_style(), span { style: stat_label_style(), "Profile" } span { style: stat_value_style(), "{selected_profile.slug()}" } }
                                div { style: proof_card_style(), span { style: stat_label_style(), "Hardware" } span { style: stat_value_style(), "{hardware_summary(&state)}" } }
                                div { style: proof_card_style(), span { style: stat_label_style(), "Hostname" } span { style: stat_value_style(), "{state.current_setup.setup.personalization.hostname}" } }
                            }
                        }
                        div {
                            style: "display:flex; flex-direction:column; gap:10px;",
                            div { style: label_style(), "Other intents" }
                            for card in preset_cards()
                                .iter()
                                .copied()
                                .filter(|card| card.id != state.current_setup.setup.preset)
                            {
                                button {
                                    style: secondary_preset_card_style(),
                                    onclick: move |_| on_apply_preset.call(card.id),
                                    div {
                                        style: "display:flex; align-items:center; justify-content:space-between; gap:8px;",
                                        span { style: "font-size:14px; font-weight:700; color:var(--maker-text-strong);", "{card.title}" }
                                        span { style: "font-size:10px; font-weight:800; color:#7ab8ff;", "{card.recommended_profile.slug()}" }
                                    }
                                    p {
                                        style: "margin:0; font-size:12px; line-height:1.6; color:var(--maker-copy);",
                                        "{card.summary}"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if current_stage == JourneyStage::Profile {
                div {
                    style: section_card_style(),
                    div {
                        style: stage_split_style,
                        div {
                            style: "display:flex; flex-direction:column; gap:14px;",
                            h2 { style: section_title_style(), "Profile" }
                            p { style: section_copy_style(), "Set the build posture clearly. This decides whether the artifact lands as server, KDE, or the dual-profile build." }
                            div {
                                style: "display:flex; flex-wrap:wrap; gap:10px;",
                                for profile in [BuildProfile::Server, BuildProfile::Kde, BuildProfile::Both] {
                                    button {
                                        style: option_button_style(selected_profile == profile, &accent),
                                        onclick: move |_| on_select_profile.call(profile),
                                        "{profile.slug()}"
                                    }
                                }
                            }
                            div {
                                style: "display:flex; flex-wrap:wrap; gap:10px;",
                                button {
                                    style: option_button_style(state.current_setup.setup.hardware.with_nvidia, &accent),
                                    onclick: move |_| on_toggle_nvidia.call(()),
                                    "NVIDIA path"
                                }
                                button {
                                    style: option_button_style(state.current_setup.setup.hardware.with_lts, &accent),
                                    onclick: move |_| on_toggle_lts.call(()),
                                    "LTS kernel"
                                }
                            }
                        }
                        div {
                            style: proof_stack_style(),
                            div { style: label_style(), "Posture proof" }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Recommended" } span { style: stat_value_style(), "{state.current_setup.setup.preset.recommended_profile().slug()}" } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Selected" } span { style: stat_value_style(), "{selected_profile.slug()}" } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Hardware" } span { style: stat_value_style(), "{hardware_summary(&state)}" } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Preset intent" } span { style: stat_value_style(), "{selected_preset.map(|card| card.title).unwrap_or(\"Unknown\")}" } }
                        }
                    }
                }
            }

            if current_stage == JourneyStage::Personalize {
                div {
                    style: section_card_style(),
                    div {
                        style: stage_split_style,
                        div {
                            style: "display:flex; flex-direction:column; gap:14px;",
                            h2 { style: section_title_style(), "Personalize" }
                            p { style: section_copy_style(), "Name the setup and give the future host a stable identity before you ask the builder to make it real." }
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(240px, 1fr)); gap:14px;",
                                div {
                                    style: "display:flex; flex-direction:column; gap:6px;",
                                    label { style: label_style(), "Setup" }
                                    input {
                                        id: "maker-setup-name",
                                        r#type: "text",
                                        value: "{state.current_setup.setup.name}",
                                        style: input_style(),
                                        oninput: move |evt| on_update_setup_name.call(evt.value()),
                                    }
                                }
                                div {
                                    style: "display:flex; flex-direction:column; gap:6px;",
                                    label { style: label_style(), "Hostname" }
                                    input {
                                        r#type: "text",
                                        value: "{state.current_setup.setup.personalization.hostname}",
                                        style: input_style(),
                                        oninput: move |evt| on_update_hostname.call(evt.value()),
                                    }
                                }
                            }
                        }
                        div {
                            style: identity_preview_style(),
                            div { style: label_style(), "Identity preview" }
                            h3 { style: "margin:0; font-size:24px; line-height:1.08; color:var(--maker-section-title);", "{state.current_setup.setup.personalization.hostname}" }
                            p { style: "margin:0; font-size:13px; line-height:1.65; color:var(--maker-copy);", "This is the machine identity that will carry through the emitted native config and the saved setup story." }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Setup slug" } span { style: stat_value_style(), "{state.current_setup.setup.slug()}" } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Journey" } span { style: stat_value_style(), "{current_stage.label()}" } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Artifacts root" } span { style: stat_value_style(), "{state.artifacts_dir}" } }
                        }
                    }
                }
            }

            if current_stage == JourneyStage::Review {
                div {
                    style: section_card_style(),
                    div {
                        style: stage_split_style,
                        div {
                            style: "display:flex; flex-direction:column; gap:14px;",
                            h2 { style: section_title_style(), "Review" }
                            p { style: section_copy_style(), "Lock the build inputs before you launch. Shell Truth on the right holds the native config and build plan while you check the last mile." }
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(280px, 1fr)); gap:14px;",
                                div {
                                    style: "display:flex; flex-direction:column; gap:6px;",
                                    label { style: label_style(), "Artifacts directory" }
                                    input {
                                        r#type: "text",
                                        value: "{state.artifacts_dir}",
                                        style: input_style(),
                                        oninput: move |evt| on_update_artifacts_dir.call(evt.value()),
                                    }
                                }
                                div {
                                    style: "display:flex; flex-direction:column; gap:6px;",
                                    label { style: label_style(), "Repo root (optional for repo-local builds)" }
                                    input {
                                        r#type: "text",
                                        value: "{state.repo_root}",
                                        style: input_style(),
                                        oninput: move |evt| on_update_repo_root.call(evt.value()),
                                    }
                                }
                            }
                        }
                        div {
                            style: proof_stack_style(),
                            div { style: label_style(), "Ready check" }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Preset" } span { style: stat_value_style(), "{selected_preset.map(|card| card.title).unwrap_or(\"Unknown\")}" } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Profile" } span { style: stat_value_style(), "{selected_profile.slug()}" } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Hardware" } span { style: stat_value_style(), "{hardware_summary(&state)}" } }
                            div {
                                style: status_card_style(),
                                div { style: "font-size:12px; font-weight:700; color:var(--maker-status-text);", "{state.build_status}" }
                                div { style: "font-size:11px; color:var(--maker-status-muted);", "Save the setup, then continue into Launch when the right rail looks truthful." }
                            }
                        }
                    }
                    div {
                        style: "display:flex; flex-wrap:wrap; gap:12px; align-items:center; justify-content:flex-end;",
                        button {
                            style: tertiary_button_style(),
                            onclick: move |_| on_save.call(()),
                            "Save Setup"
                        }
                        button {
                            style: primary_button_style(&accent),
                            onclick: move |_| on_set_stage.call(JourneyStage::Build),
                            "Continue to Launch"
                        }
                    }
                }
            }

            if current_stage == JourneyStage::Build {
                div {
                    style: section_card_style(),
                    div {
                        style: "display:flex; flex-direction:column; gap:14px;",
                        h2 { style: section_title_style(), "Launch" }
                        p { style: section_copy_style(), "Launch the local Docker build on Linux, or export the truthful handoff bundle on the other platforms. Raw logs stay in Shell Truth; the main canvas stays focused on the outcome." }
                    }
                    div {
                        style: build_split_style,
                        div {
                            style: "display:flex; flex-direction:column; gap:14px;",
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(180px, 1fr)); gap:12px;",
                                div { style: proof_card_style(), span { style: stat_label_style(), "Mode" } span { style: stat_value_style(), "{build_mode_label()}" } }
                                div { style: proof_card_style(), span { style: stat_label_style(), "Status" } span { style: stat_value_style(), "{state.build_status}" } }
                                div { style: proof_card_style(), span { style: stat_label_style(), "Artifacts" } span { style: stat_value_style(), "{state.artifacts_dir}" } }
                            }
                            if !state.build_result.trim().is_empty() {
                                div {
                                    style: status_card_style(),
                                    div { style: "font-size:12px; font-weight:700; color:var(--maker-status-text);", "Latest result" }
                                    div { style: "font-size:11px; line-height:1.6; color:var(--maker-status-muted);", "{latest_result_summary(&state)}" }
                                }
                            }
                        }
                        div {
                            style: info_stack_style(),
                            div { style: label_style(), "Launch" }
                            div { style: info_row_style(), span { style: stat_label_style(), "OS path" } span { style: stat_value_style(), "{build_mode_label()}" } }
                            div { style: info_row_style(), span { style: stat_label_style(), "Truth rail" } span { style: stat_value_style(), "Structured logs and manifest stay on the right." } }
                            div { style: info_row_style(), span { style: stat_label_style(), "After build" } span { style: stat_value_style(), "Dedicated success handoff with artifact actions." } }
                        }
                    }
                    div {
                        style: "display:flex; flex-wrap:wrap; gap:12px; margin-top:8px; align-items:center; justify-content:flex-end;",
                        button {
                            style: tertiary_button_style(),
                            onclick: move |_| on_save.call(()),
                            "Save Setup"
                        }
                        button {
                            style: primary_button_style(&accent),
                            disabled: state.build_running,
                            onclick: move |_| on_build.call(()),
                            if state.build_running { "{launch_running_label()}" } else { "{launch_action_label()}" }
                        }
                    }
                }
            }

            if current_stage == JourneyStage::Boot {
                div {
                    style: section_card_style(),
                    div {
                        style: stage_split_style,
                        div {
                            style: "display:flex; flex-direction:column; gap:14px;",
                            h2 { style: section_title_style(), "Boot" }
                            p { style: section_copy_style(), "This is the handoff moment after a truthful build or export. If the dedicated success surface is not active yet, return to Build and rerun or inspect the latest artifacts." }
                            if state.recent_artifacts.is_empty() {
                                div { style: empty_note_style(), "No recent artifact summary is available yet for this setup." }
                            } else {
                                div {
                                    style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(220px, 1fr)); gap:12px;",
                                    for artifact in state.recent_artifacts.iter().take(3).cloned() {
                                        div {
                                            style: proof_card_style(),
                                            span { style: stat_label_style(), "{artifact.subtitle}" }
                                            span { style: stat_value_style(), "{artifact.title}" }
                                        }
                                    }
                                }
                            }
                        }
                        div {
                            style: proof_stack_style(),
                            div { style: label_style(), "Handoff" }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Primary action" } span { style: stat_value_style(), "Reveal artifact, inspect details, or start the next setup." } }
                            div { style: proof_card_style(), span { style: stat_label_style(), "Current setup" } span { style: stat_value_style(), "{state.current_setup.setup.name}" } }
                        }
                    }
                    button {
                        style: primary_button_style(&accent),
                        onclick: move |_| on_set_stage.call(JourneyStage::Build),
                        "Open Build Stage"
                    }
                }
            }

            div {
                style: stage_footer_bar_style(),
                div {
                    style: "display:flex; flex-direction:column; gap:4px;",
                    div { style: "font-size:11px; font-weight:800; letter-spacing:0.08em; text-transform:uppercase; color:var(--maker-note);", "Next move" }
                    div { style: "font-size:13px; color:var(--maker-copy);", "{stage_footer_copy(current_stage)}" }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:6px; min-width:180px; flex:1 1 180px; max-width:260px;",
                    div {
                        style: "display:flex; align-items:center; justify-content:space-between; gap:10px; font-size:11px; color:var(--maker-note);",
                        span { "Guided progress" }
                        span { "{stage_index}/{stage_total}" }
                    }
                    div {
                        style: "position:relative; height:8px; border-radius:999px; overflow:hidden; background:color-mix(in srgb, var(--maker-card-bg) 72%, transparent); box-shadow:inset 0 0 0 1px color-mix(in srgb, var(--maker-card-border) 82%, transparent);",
                        div {
                            style: format!(
                                "height:100%; width:{:.2}%; border-radius:999px; background:linear-gradient(90deg, color-mix(in srgb, {} 80%, white) 0%, {} 100%); animation:makerProgressSweep 2.6s ease-in-out infinite; transform-origin:left center;",
                                journey_stage_progress_percent(current_stage),
                                accent,
                                accent
                            ),
                        }
                    }
                }
                div {
                    style: "display:flex; flex-wrap:wrap; gap:10px;",
                    if let Some(stage) = previous_stage {
                        button {
                            style: tertiary_button_style(),
                            onclick: move |_| on_set_stage.call(stage),
                            "Back to {stage.label()}"
                        }
                    }
                    if let Some(stage) = next_stage {
                        button {
                            style: guided_primary_button_style(&accent),
                            onclick: move |_| on_set_stage.call(stage),
                            "Continue to {stage.label()}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SuccessScreen(
    success: BuildSuccessState,
    accent: String,
    preview_surface: String,
    on_reveal: EventHandler<()>,
    on_open_details: EventHandler<()>,
    on_start_another: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            style: "display:flex; flex-direction:column; gap:18px; max-width:920px; margin:0 auto;",
            div {
                style: format!(
                    "{} padding:34px 34px 36px 34px; border-radius:30px; box-shadow:0 28px 70px rgba(82,104,130,0.19), inset 0 0 0 1px var(--maker-section-border);",
                    preview_surface
                ),
                div {
                    style: format!("font-size:12px; font-weight:800; letter-spacing:0.1em; color:{};", accent),
                    "SUCCESS"
                }
                h1 {
                    style: "margin:10px 0 8px 0; font-size:42px; line-height:1.02; color:var(--maker-section-title);",
                    "{success.title}"
                }
                p {
                    style: "margin:0 0 18px 0; max-width:700px; font-size:15px; line-height:1.7; color:var(--maker-copy);",
                    "{success.proof}"
                }
                div {
                    style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(220px, 1fr)); gap:12px; margin-bottom:18px;",
                    div { style: success_stat_style(), span { style: stat_label_style(), "Artifact" } span { style: stat_value_style(), "{success.artifact_name}" } }
                    div { style: success_stat_style(), span { style: stat_label_style(), "Profile" } span { style: stat_value_style(), "{success.profile_label}" } }
                    div { style: success_stat_style(), span { style: stat_label_style(), "Path" } span { style: stat_value_style(), "{success.output_path}" } }
                }
                div {
                    style: "display:flex; flex-wrap:wrap; gap:12px;",
                    button { style: primary_button_style(&accent), onclick: move |_| on_reveal.call(()), "Reveal Artifact" }
                    button { style: secondary_button_style(), onclick: move |_| on_open_details.call(()), "Open Build Details" }
                    button { style: tertiary_button_style(), onclick: move |_| on_start_another.call(()), "Start Another Setup" }
                }
            }
        }
    }
}

#[component]
fn AppearanceSidebar(
    accent: String,
    shell_settings: MakerShellSettings,
    theme_draft: YgguiThemeSpec,
    selected_stop: Option<usize>,
    preview_surface: String,
    on_select_preset: EventHandler<ThemePreset>,
    on_select_finish: EventHandler<ShellFinish>,
    on_select_theme: EventHandler<UiTheme>,
    on_begin_drag_stop: EventHandler<usize>,
    on_drag_stop: EventHandler<(f32, f32)>,
    on_end_drag_stop: EventHandler<MouseEvent>,
    on_double_click_pad: EventHandler<(f32, f32)>,
    on_pick_stop: EventHandler<usize>,
    on_pick_swatch: EventHandler<String>,
    on_update_stop_color: EventHandler<String>,
    on_set_brightness: EventHandler<f32>,
    on_set_grain: EventHandler<f32>,
    on_add_stop: EventHandler<MouseEvent>,
    on_remove_stop: EventHandler<MouseEvent>,
    on_reset: EventHandler<MouseEvent>,
    on_seed: EventHandler<MouseEvent>,
    on_save: EventHandler<MouseEvent>,
) -> Element {
    let active_stop = selected_stop.and_then(|index| theme_draft.colors.get(index).cloned());
    let brightness_percent = (theme_draft.brightness * 100.0).round() as i32;
    let grain_percent = (theme_draft.grain * 100.0).round() as i32;
    let preview_has_stops = !theme_draft.colors.is_empty();
    rsx! {
        div {
            style: "display:flex; flex-direction:column; gap:14px; min-width:0;",
            div {
                style: appearance_sidebar_card_style(),
                div {
                    style: "display:flex; align-items:center; justify-content:space-between; gap:12px;",
                    div {
                        style: "display:flex; flex-direction:column; gap:4px;",
                        div {
                            style: format!("font-size:11px; font-weight:800; letter-spacing:0.08em; color:{};", accent),
                            "APPEARANCE"
                        }
                        div {
                            style: "font-size:13px; color:var(--maker-copy); line-height:1.5;",
                            "Shape the shared shell gradient, brightness, and finish from the right rail without leaving the build studio."
                        }
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    div { style: label_style(), "Theme Preset" }
                    div {
                        style: "display:flex; flex-wrap:wrap; gap:8px;",
                        for preset in ThemePreset::all() {
                            button {
                                style: small_chip_style(theme_matches_preset(&shell_settings.yggui_theme, preset), &accent),
                                onclick: move |_| on_select_preset.call(preset),
                                "{preset.label()}"
                            }
                        }
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    div { style: label_style(), "Finish" }
                    div {
                        style: "display:flex; flex-wrap:wrap; gap:8px;",
                        for finish in [ShellFinish::Sleek, ShellFinish::Crisp] {
                            button {
                                style: small_chip_style(shell_settings.finish == finish, &accent),
                                onclick: move |_| on_select_finish.call(finish),
                                "{finish.label()}"
                            }
                        }
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    div { style: label_style(), "Shell Mode" }
                    div {
                        style: appearance_segment_style(),
                        button {
                            style: appearance_segment_button_style(shell_settings.theme == UiTheme::ZedLight),
                            onclick: move |_| on_select_theme.call(UiTheme::ZedLight),
                            "Light"
                        }
                        button {
                            style: appearance_segment_button_style(shell_settings.theme == UiTheme::ZedDark),
                            onclick: move |_| on_select_theme.call(UiTheme::ZedDark),
                            "Dark"
                        }
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:10px;",
                    div { style: label_style(), "Gradient Pad" }
                    div {
                        style: format!(
                            "position:relative; width:100%; aspect-ratio:1 / 1; border-radius:18px; overflow:hidden; background:{}; box-shadow: inset 0 0 0 1px rgba(255,255,255,0.46), 0 18px 38px rgba(84,113,137,0.12);",
                            preview_surface
                        ),
                        onmousemove: move |evt| {
                            let point = evt.element_coordinates();
                            on_drag_stop.call((
                                normalize_theme_editor_axis(point.x),
                                normalize_theme_editor_axis(point.y),
                            ));
                        },
                        onmouseup: move |evt| on_end_drag_stop.call(evt),
                        onmouseleave: move |evt| on_end_drag_stop.call(evt),
                        ondoubleclick: move |evt| {
                            let point = evt.element_coordinates();
                            on_double_click_pad.call((
                                normalize_theme_editor_axis(point.x),
                                normalize_theme_editor_axis(point.y),
                            ));
                        },
                        div {
                            style: "position:absolute; inset:0; background-image: linear-gradient(rgba(144,173,199,0.18) 1px, transparent 1px), linear-gradient(90deg, rgba(144,173,199,0.18) 1px, transparent 1px); background-size: 18px 18px; opacity:0.72; pointer-events:none;",
                        }
                        div {
                            style: "position:absolute; inset:0; background-image: linear-gradient(rgba(255,255,255,0.24) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.24) 1px, transparent 1px); background-size: 72px 72px; opacity:0.46; pointer-events:none;",
                        }
                        if !preview_has_stops {
                            div {
                                style: "position:absolute; inset:0; display:flex; align-items:center; justify-content:center; padding:18px; text-align:center; font-size:12px; font-weight:700; line-height:1.6; color:var(--maker-text-strong);",
                                "Double-click to add a color stop"
                            }
                        }
                        for (index, stop) in theme_draft.colors.iter().enumerate() {
                            button {
                                key: "maker-theme-stop-{index}",
                                style: format!(
                                    "position:absolute; left:calc({:.2}% - 10px); top:calc({:.2}% - 10px); width:20px; height:20px; border-radius:999px; border:{}; background:{}; box-shadow:0 8px 18px rgba(42,67,88,0.16);",
                                    stop.x * 100.0,
                                    stop.y * 100.0,
                                    if selected_stop == Some(index) {
                                        format!("3px solid {}", accent)
                                    } else {
                                        "2px solid rgba(255,255,255,0.88)".to_string()
                                    },
                                    stop.color
                                ),
                                onmousedown: move |evt| {
                                    evt.stop_propagation();
                                    on_begin_drag_stop.call(index);
                                },
                                onclick: move |_| on_pick_stop.call(index),
                            }
                        }
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    div { style: label_style(), "Color Library" }
                    div {
                        style: "display:flex; flex-wrap:wrap; gap:8px;",
                        for swatch in THEME_EDITOR_SWATCHES {
                            button {
                                key: "maker-theme-swatch-{swatch}",
                                style: format!(
                                    "width:22px; height:22px; border-radius:999px; border:2px solid rgba(255,255,255,0.92); background:{}; box-shadow:0 8px 16px rgba(45,67,88,0.12);",
                                    swatch
                                ),
                                onclick: move |_| on_pick_swatch.call(swatch.to_string()),
                            }
                        }
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    div { style: label_style(), "Selected Color" }
                    input {
                        r#type: "color",
                        value: active_stop.as_ref().map(|stop| stop.color.clone()).unwrap_or_else(|| accent.clone()),
                        style: "width:100%; height:40px; border:none; border-radius:12px; background:transparent;",
                        oninput: move |evt| on_update_stop_color.call(evt.value()),
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    div {
                        style: "display:flex; align-items:center; justify-content:space-between; gap:10px;",
                        div { style: label_style(), "Brightness" }
                        div { style: format!("font-size:11px; font-weight:700; color:{};", accent), "{brightness_percent}" }
                    }
                    input {
                        r#type: "range",
                        min: "0",
                        max: "100",
                        value: "{brightness_percent}",
                        style: appearance_range_style(),
                        oninput: move |evt| {
                            let value = evt.value().parse::<f32>().unwrap_or(56.0) / 100.0;
                            on_set_brightness.call(value);
                        },
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:8px;",
                    div {
                        style: "display:flex; align-items:center; justify-content:space-between; gap:10px;",
                        div { style: label_style(), "Grain" }
                        div { style: format!("font-size:11px; font-weight:700; color:{};", accent), "{grain_percent}" }
                    }
                    input {
                        r#type: "range",
                        min: "0",
                        max: "100",
                        value: "{grain_percent}",
                        style: appearance_range_style(),
                        oninput: move |evt| {
                            let value = evt.value().parse::<f32>().unwrap_or(12.0) / 100.0;
                            on_set_grain.call(value);
                        },
                    }
                }
                div {
                    style: "display:flex; flex-wrap:wrap; gap:8px;",
                    button {
                        style: tertiary_button_style(),
                        onclick: move |evt| on_add_stop.call(evt),
                        "+ Stop"
                    }
                    button {
                        style: tertiary_button_style(),
                        onclick: move |evt| on_remove_stop.call(evt),
                        "Remove"
                    }
                    button {
                        style: tertiary_button_style(),
                        onclick: move |evt| on_reset.call(evt),
                        "Reset"
                    }
                    button {
                        style: tertiary_button_style(),
                        onclick: move |evt| on_seed.call(evt),
                        "Starter"
                    }
                }
                button {
                    style: primary_rail_button_style(&accent),
                    onclick: move |evt| on_save.call(evt),
                    "Apply Theme"
                }
            }
        }
    }
}

#[component]
fn WindowResizeHandles() -> Element {
    rsx! {
        ResizeHandle {
            style: format!("position:absolute; top:0; left:0; width:{}px; height:{}px; z-index:120; cursor:nwse-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE),
            direction: ResizeDirection::NorthWest,
        }
        ResizeHandle {
            style: format!("position:absolute; top:0; right:0; width:{}px; height:{}px; z-index:120; cursor:nesw-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE),
            direction: ResizeDirection::NorthEast,
        }
        ResizeHandle {
            style: format!("position:absolute; bottom:0; left:0; width:{}px; height:{}px; z-index:120; cursor:nesw-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE),
            direction: ResizeDirection::SouthWest,
        }
        ResizeHandle {
            style: format!("position:absolute; bottom:0; right:0; width:{}px; height:{}px; z-index:120; cursor:nwse-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE),
            direction: ResizeDirection::SouthEast,
        }
        ResizeHandle {
            style: format!("position:absolute; top:0; left:{}px; right:{}px; height:{}px; z-index:119; cursor:ns-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE, EDGE_RESIZE_HANDLE),
            direction: ResizeDirection::North,
        }
        ResizeHandle {
            style: format!("position:absolute; bottom:0; left:{}px; right:{}px; height:{}px; z-index:119; cursor:ns-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE, EDGE_RESIZE_HANDLE),
            direction: ResizeDirection::South,
        }
        ResizeHandle {
            style: format!("position:absolute; top:{}px; bottom:{}px; left:0; width:{}px; z-index:119; cursor:ew-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE, EDGE_RESIZE_HANDLE),
            direction: ResizeDirection::West,
        }
        ResizeHandle {
            style: format!("position:absolute; top:{}px; bottom:{}px; right:0; width:{}px; z-index:119; cursor:ew-resize;", CORNER_RESIZE_HANDLE, CORNER_RESIZE_HANDLE, EDGE_RESIZE_HANDLE),
            direction: ResizeDirection::East,
        }
    }
}

#[component]
fn ResizeHandle(style: String, direction: ResizeDirection) -> Element {
    rsx! {
        div {
            style: "{style}",
            onmousedown: move |evt| {
                evt.stop_propagation();
                let _ = window().drag_resize_window(direction);
            },
            ondoubleclick: |evt| evt.stop_propagation(),
        }
    }
}

#[component]
fn FolderTreeIcon(expanded: bool) -> Element {
    rsx! {
        svg {
            width: "15",
            height: "15",
            view_box: "0 0 16 16",
            fill: if expanded { "currentColor" } else { "none" },
            xmlns: "http://www.w3.org/2000/svg",
            path {
                d: "M2.1 4.1C2.1 3.27 2.77 2.6 3.6 2.6H6.3L7.45 3.75H12.4C13.23 3.75 13.9 4.42 13.9 5.25V11.2C13.9 12.03 13.23 12.7 12.4 12.7H3.6C2.77 12.7 2.1 12.03 2.1 11.2V4.1Z",
                stroke: "currentColor",
                stroke_width: "1.1",
                stroke_linejoin: "round",
                fill_opacity: if expanded { "0.94" } else { "0" },
            }
        }
    }
}

#[component]
fn ReleaseLeafIcon(selected: bool) -> Element {
    rsx! {
        span {
            style: format!(
                "display:inline-flex; flex:0 0 auto; width:7px; height:7px; border-radius:999px; background:{}; \
                 box-shadow:0 0 0 1px color-mix(in srgb, {} 22%, transparent);",
                if selected {
                    "var(--maker-accent)"
                } else {
                    "color-mix(in srgb, var(--maker-note) 76%, transparent)"
                },
                if selected {
                    "var(--maker-accent)"
                } else {
                    "var(--maker-note)"
                }
            ),
        }
    }
}

#[component]
fn StageCartoon(stage: JourneyStage, accent: String, compact: bool) -> Element {
    let frame_style = if compact {
        "display:none;"
    } else {
        "display:flex; align-items:center; justify-content:center; min-height:150px; padding:10px 0 0 0; animation:makerFloat 4.6s ease-in-out infinite;"
    };
    let stroke = if matches!(stage, JourneyStage::Build | JourneyStage::Boot) {
        "rgba(239,247,253,0.72)"
    } else {
        "rgba(228,239,247,0.66)"
    };
    rsx! {
        div {
            style: frame_style,
            svg {
                width: "250",
                height: "160",
                view_box: "0 0 250 160",
                fill: "none",
                xmlns: "http://www.w3.org/2000/svg",
                ellipse { cx: "126", cy: "136", rx: "84", ry: "18", fill: "rgba(12,18,26,0.12)" }
                rect { x: "44", y: "34", width: "162", height: "78", rx: "18", fill: "rgba(255,255,255,0.08)", stroke: "{stroke}", stroke_width: "1.2" }
                match stage {
                    JourneyStage::Outcome => rsx! {
                        rect { x: "64", y: "54", width: "50", height: "38", rx: "12", fill: "rgba(255,255,255,0.10)", stroke: "{stroke}", stroke_width: "1" }
                        rect { x: "124", y: "54", width: "62", height: "38", rx: "12", fill: "color-mix(in srgb, {accent} 18%, rgba(255,255,255,0.06))", stroke: "{accent}", stroke_width: "1.4" }
                        circle { cx: "156", cy: "73", r: "10", fill: "{accent}" }
                        path { d: "M152 73L155 76L161 69", stroke: "white", stroke_width: "2.4", stroke_linecap: "round", stroke_linejoin: "round" }
                    },
                    JourneyStage::Profile => rsx! {
                        rect { x: "60", y: "56", width: "40", height: "34", rx: "10", fill: "rgba(255,255,255,0.10)", stroke: "{stroke}", stroke_width: "1" }
                        rect { x: "108", y: "48", width: "40", height: "42", rx: "10", fill: "color-mix(in srgb, {accent} 16%, rgba(255,255,255,0.08))", stroke: "{accent}", stroke_width: "1.4" }
                        rect { x: "156", y: "60", width: "34", height: "30", rx: "10", fill: "rgba(255,255,255,0.10)", stroke: "{stroke}", stroke_width: "1" }
                        path { d: "M128 118V94", stroke: "{accent}", stroke_width: "2", stroke_linecap: "round" }
                        path { d: "M118 109L128 119L138 109", stroke: "{accent}", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round" }
                    },
                    JourneyStage::Personalize => rsx! {
                        rect { x: "64", y: "54", width: "122", height: "34", rx: "12", fill: "rgba(255,255,255,0.10)", stroke: "{stroke}", stroke_width: "1" }
                        path { d: "M80 72H142", stroke: "{accent}", stroke_width: "2.4", stroke_linecap: "round" }
                        circle { cx: "166", cy: "71", r: "10", fill: "color-mix(in srgb, {accent} 20%, rgba(255,255,255,0.08))", stroke: "{accent}", stroke_width: "1.3" }
                        path { d: "M166 64V78", stroke: "{accent}", stroke_width: "1.8", stroke_linecap: "round" }
                        path { d: "M159 71H173", stroke: "{accent}", stroke_width: "1.8", stroke_linecap: "round" }
                    },
                    JourneyStage::Review => rsx! {
                        rect { x: "66", y: "50", width: "112", height: "50", rx: "12", fill: "rgba(255,255,255,0.10)", stroke: "{stroke}", stroke_width: "1" }
                        path { d: "M82 66H144", stroke: "rgba(255,255,255,0.62)", stroke_width: "1.8", stroke_linecap: "round" }
                        path { d: "M82 78H138", stroke: "rgba(255,255,255,0.48)", stroke_width: "1.8", stroke_linecap: "round" }
                        path { d: "M188 62L198 72L214 54", stroke: "{accent}", stroke_width: "3", stroke_linecap: "round", stroke_linejoin: "round" }
                    },
                    JourneyStage::Build => rsx! {
                        rect { x: "60", y: "56", width: "70", height: "36", rx: "12", fill: "rgba(255,255,255,0.10)", stroke: "{stroke}", stroke_width: "1" }
                        rect { x: "142", y: "56", width: "46", height: "36", rx: "12", fill: "color-mix(in srgb, {accent} 14%, rgba(255,255,255,0.08))", stroke: "{accent}", stroke_width: "1.4" }
                        path { d: "M116 74H142", stroke: "{accent}", stroke_width: "2.4", stroke_linecap: "round" }
                        path { d: "M134 66L142 74L134 82", stroke: "{accent}", stroke_width: "2.4", stroke_linecap: "round", stroke_linejoin: "round" }
                    },
                    JourneyStage::Boot => rsx! {
                        rect { x: "72", y: "48", width: "98", height: "56", rx: "14", fill: "rgba(255,255,255,0.10)", stroke: "{stroke}", stroke_width: "1" }
                        rect { x: "98", y: "104", width: "46", height: "10", rx: "5", fill: "{accent}" }
                        path { d: "M182 70L192 80L208 62", stroke: "{accent}", stroke_width: "3", stroke_linecap: "round", stroke_linejoin: "round" }
                    },
                }
            }
        }
    }
}

fn start_build(mut state: Signal<MakerUiState>) {
    if state.read().build_running {
        return;
    }

    {
        state.with_mut(|ui| {
            ui.save_current_setup();
            ui.build_running = true;
            ui.build_log.clear();
            ui.build_result.clear();
            ui.build_status = "Building…".to_owned();
            ui.success_state = None;
            ui.appearance_panel_open = false;
            ui.right_panel_mode = RightPanelMode::Build;
            ui.utility_pane_open = true;
            ui.shell_settings.utility_pane_open = true;
            ui.shell_settings.right_panel_mode = RightPanelMode::Build;
            ui.persist_shell_settings();
            ui.set_journey_stage(JourneyStage::Build);
            ui.push_notification(
                ToastTone::Info,
                "Build Started",
                format!("Running {}", ui.current_setup.setup.name),
            );
            trace_ui(
                &ui.trace_root,
                "build",
                "start",
                json!({
                    "setup_id": ui.current_setup.setup_id,
                    "artifacts_dir": ui.artifacts_dir,
                    "repo_root": ui.repo_root,
                }),
            );
        });
    }

    let inputs = state.read().build_inputs();
    let (tx, rx) = mpsc::channel::<BuildMessage>();

    thread::spawn(move || {
        let app = match MakerApp::new_for_current_platform() {
            Ok(app) => app,
            Err(error) => {
                let _ = tx.send(BuildMessage::Failed(error.to_string()));
                return;
            }
        };

        let result = app.run_build(inputs, |event| {
            let line = serde_json::to_string(&event).unwrap_or_else(|_| format!("{event:?}"));
            let _ = tx.send(BuildMessage::Event(line));
        });

        match result {
            Ok(outcome) => {
                let payload = serde_json::to_string_pretty(&outcome.manifest)
                    .unwrap_or_else(|error| error.to_string());
                let _ = tx.send(BuildMessage::Finished {
                    manifest: outcome.manifest,
                    payload,
                });
            }
            Err(error) => {
                let _ = tx.send(BuildMessage::Failed(error.to_string()));
            }
        }
    });

    spawn(async move {
        loop {
            let mut saw_message = false;
            let mut done = false;
            loop {
                match rx.try_recv() {
                    Ok(message) => {
                        saw_message = true;
                        state.with_mut(|ui| match message {
                            BuildMessage::Event(line) => ui.build_log.push(line),
                            BuildMessage::Finished { manifest, payload } => {
                                ui.build_running = false;
                                ui.build_status = "Build finished".to_owned();
                                ui.build_result = payload;
                                ui.activate_success_state(&manifest);
                                ui.refresh_recent_artifacts();
                                ui.push_notification(
                                    ToastTone::Success,
                                    "Artifact Ready",
                                    format!("{} is ready.", manifest.setup_name),
                                );
                                trace_ui(
                                    &ui.trace_root,
                                    "build",
                                    "success",
                                    json!({
                                        "setup_id": ui.current_setup.setup_id,
                                        "mode": manifest.mode,
                                        "profile": manifest.build_profile,
                                    }),
                                );
                                done = true;
                            }
                            BuildMessage::Failed(error) => {
                                ui.build_running = false;
                                ui.build_status = format!("Build failed: {error}");
                                ui.build_result = error.clone();
                                ui.push_notification(
                                    ToastTone::Error,
                                    "Build Failed",
                                    error.clone(),
                                );
                                trace_ui(
                                    &ui.trace_root,
                                    "build",
                                    "failure",
                                    json!({
                                        "setup_id": ui.current_setup.setup_id,
                                        "error": error,
                                    }),
                                );
                                done = true;
                            }
                        });
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        done = true;
                        break;
                    }
                }
            }

            if done {
                break;
            }
            if !saw_message {
                sleep(Duration::from_millis(80)).await;
            }
        }
    });
}

async fn process_pending_app_control_requests(
    home: &Path,
    desktop: &DesktopContext,
    mut state: Signal<MakerUiState>,
) -> Result<bool> {
    let Some((inflight_path, request)) = take_next_app_control_request(home, std::process::id())?
    else {
        return Ok(false);
    };

    trace_ui(
        home,
        "app-control",
        "request_begin",
        json!({
            "request_id": request.request_id,
            "command": request.command.name(),
        }),
    );

    let response = match request.command.clone() {
        AppControlCommand::FocusWindow => {
            response_from_result(&request, focus_app_window(desktop), None)
        }
        AppControlCommand::DescribeState => AppControlResponse {
            request_id: request.request_id.clone(),
            handled_by_pid: std::process::id(),
            completed_at_ms: crate::app_control::current_millis(),
            output_path: None,
            data: Some(describe_app_state_snapshot(&state.read(), desktop)),
            error: None,
        },
        AppControlCommand::DescribeRows => AppControlResponse {
            request_id: request.request_id.clone(),
            handled_by_pid: std::process::id(),
            completed_at_ms: crate::app_control::current_millis(),
            output_path: None,
            data: Some(describe_app_rows_snapshot(&state.read())),
            error: None,
        },
        AppControlCommand::CaptureScreenshot { output_path } => {
            let target = if output_path.trim().is_empty() {
                default_screenshot_output_path(home, &request.request_id)
            } else {
                PathBuf::from(output_path)
            };
            match capture_visible_app_surface(desktop, &target).await {
                Ok(path) => AppControlResponse {
                    request_id: request.request_id.clone(),
                    handled_by_pid: std::process::id(),
                    completed_at_ms: crate::app_control::current_millis(),
                    output_path: Some(path.display().to_string()),
                    data: Some(json!({
                        "window": describe_window(desktop),
                    })),
                    error: None,
                },
                Err(error) => AppControlResponse {
                    request_id: request.request_id.clone(),
                    handled_by_pid: std::process::id(),
                    completed_at_ms: crate::app_control::current_millis(),
                    output_path: None,
                    data: None,
                    error: Some(error.to_string()),
                },
            }
        }
        AppControlCommand::CaptureScreenRecording {
            output_path,
            duration_secs,
        } => {
            let target = if output_path.trim().is_empty() {
                default_recording_output_path(home, &request.request_id)
            } else {
                PathBuf::from(output_path)
            };
            match record_visible_app_surface(desktop, &target, duration_secs) {
                Ok(path) => AppControlResponse {
                    request_id: request.request_id.clone(),
                    handled_by_pid: std::process::id(),
                    completed_at_ms: crate::app_control::current_millis(),
                    output_path: Some(path.display().to_string()),
                    data: Some(json!({
                        "window": describe_window(desktop),
                        "duration_secs": duration_secs,
                    })),
                    error: None,
                },
                Err(error) => AppControlResponse {
                    request_id: request.request_id.clone(),
                    handled_by_pid: std::process::id(),
                    completed_at_ms: crate::app_control::current_millis(),
                    output_path: None,
                    data: None,
                    error: Some(error.to_string()),
                },
            }
        }
        AppControlCommand::NewSetup {
            name,
            preset,
            profile,
            hostname,
        } => {
            state.with_mut(|ui| {
                ui.start_another_setup();
                if let Some(name) = name {
                    ui.current_setup.setup.name = name;
                }
                if let Some(preset) = preset {
                    ui.apply_preset(preset);
                }
                if let Some(profile) = profile {
                    ui.current_setup.setup.profile_override = Some(profile);
                }
                if let Some(hostname) = hostname {
                    ui.current_setup.setup.personalization.hostname = hostname;
                }
                ui.refresh_previews();
            });
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SelectSetup { setup_id } => {
            state.with_mut(|ui| ui.select_setup(&setup_id));
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SaveSetup => {
            state.with_mut(|ui| ui.save_current_setup());
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetJourneyStage { stage } => {
            state.with_mut(|ui| ui.set_journey_stage(stage));
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetSetupName { value } => {
            update_setup_name(state, value);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetHostname { value } => {
            update_hostname(state, value);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetArtifactsDir { value } => {
            update_artifacts_dir(state, value);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetRepoRoot { value } => {
            update_repo_root(state, value);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetBuildContext {
            artifacts_dir,
            repo_root,
        } => {
            state.with_mut(|ui| {
                ui.artifacts_dir = artifacts_dir;
                ui.repo_root = repo_root;
                ui.success_state = None;
                ui.refresh_previews();
                ui.refresh_recent_artifacts();
            });
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::ApplyPreset { preset } => {
            state.with_mut(|ui| ui.apply_preset(preset));
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetProfile { profile } => {
            update_profile(state, profile);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::ToggleNvidia => {
            toggle_nvidia(state);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::ToggleLts => {
            toggle_lts(state);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetSidebarOpen { open } => {
            state.with_mut(|ui| {
                ui.sidebar_open = open;
                ui.shell_settings.sidebar_open = open;
                ui.persist_shell_settings();
            });
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetUtilityPaneOpen { open } => {
            state.with_mut(|ui| {
                ui.utility_pane_open = open;
                ui.shell_settings.utility_pane_open = open;
                ui.persist_shell_settings();
            });
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetRightPanelMode { mode } => {
            state.with_mut(|ui| {
                ui.appearance_panel_open = false;
                ui.right_panel_mode = mode;
                ui.shell_settings.right_panel_mode = mode;
                ui.persist_shell_settings();
            });
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::SetAppearancePanelOpen { open } => {
            state.with_mut(|ui| {
                if open {
                    ui.open_appearance_sidebar();
                } else {
                    ui.close_appearance_sidebar();
                }
            });
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::StartBuild => {
            start_build(state);
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::OpenBuildDetails => {
            state.with_mut(|ui| ui.open_build_details());
            snapshot_response(&request, &state.read(), desktop)
        }
        AppControlCommand::RevealPrimaryArtifact => {
            let result = state.with(|ui| {
                if let Some(success) = ui.success_state.as_ref() {
                    reveal_path(&success.output_path)
                } else if let Some(manifest) = ui.latest_manifest() {
                    let path = primary_artifact(&manifest)
                        .map(|artifact| artifact.path.clone())
                        .unwrap_or_else(|| ui.artifacts_dir.clone());
                    reveal_path(&path)
                } else {
                    Err(anyhow!("no artifact is available to reveal"))
                }
            });
            response_from_result(
                &request,
                result.map(|_| describe_app_state_snapshot(&state.read(), desktop)),
                None,
            )
        }
    };

    complete_app_control_request(home, &inflight_path, &response)?;
    trace_ui(
        home,
        "app-control",
        "request_end",
        json!({
            "request_id": response.request_id,
            "error": response.error,
            "output_path": response.output_path,
        }),
    );
    Ok(true)
}

fn snapshot_response(
    request: &AppControlRequest,
    state: &MakerUiState,
    desktop: &DesktopContext,
) -> AppControlResponse {
    sync_bootstrap_from_state(state);
    AppControlResponse {
        request_id: request.request_id.clone(),
        handled_by_pid: std::process::id(),
        completed_at_ms: crate::app_control::current_millis(),
        output_path: None,
        data: Some(describe_app_state_snapshot(state, desktop)),
        error: None,
    }
}

fn response_from_result(
    request: &AppControlRequest,
    result: Result<Value>,
    output_path: Option<String>,
) -> AppControlResponse {
    match result {
        Ok(data) => AppControlResponse {
            request_id: request.request_id.clone(),
            handled_by_pid: std::process::id(),
            completed_at_ms: crate::app_control::current_millis(),
            output_path,
            data: Some(data),
            error: None,
        },
        Err(error) => AppControlResponse {
            request_id: request.request_id.clone(),
            handled_by_pid: std::process::id(),
            completed_at_ms: crate::app_control::current_millis(),
            output_path: None,
            data: None,
            error: Some(error.to_string()),
        },
    }
}

fn describe_app_state_snapshot(state: &MakerUiState, desktop: &DesktopContext) -> Value {
    json!({
        "window": describe_window(desktop),
        "current_setup": {
            "setup_id": state.current_setup.setup_id,
            "name": state.current_setup.setup.name,
            "slug": state.current_setup.setup.slug(),
            "preset": state.current_setup.setup.preset.slug(),
            "profile": state.current_setup.setup.profile_override.unwrap_or_else(|| state.current_setup.setup.preset.recommended_profile()).slug(),
            "journey_stage": state.current_setup.journey_stage.label(),
            "hostname": state.current_setup.setup.personalization.hostname,
            "with_nvidia": state.current_setup.setup.hardware.with_nvidia,
            "with_lts": state.current_setup.setup.hardware.with_lts,
        },
        "shell": {
            "sidebar_open": state.sidebar_open,
            "right_panel_mode": state.right_panel_mode.label().to_ascii_lowercase(),
            "utility_pane_open": state.utility_pane_open,
            "appearance_panel_open": state.appearance_panel_open,
            "recent_artifacts_expanded": state.recent_artifacts_expanded,
            "maximized": state.maximized,
            "always_on_top": state.always_on_top,
        },
        "build": {
            "running": state.build_running,
            "status": state.build_status,
            "result": state.build_result,
            "log_line_count": state.build_log.len(),
            "log_tail": state.build_log.iter().rev().take(80).cloned().collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>(),
            "artifacts_dir": state.artifacts_dir,
            "repo_root": state.repo_root,
        },
        "success": state.success_state.as_ref().map(|success| json!({
            "title": success.title,
            "artifact_name": success.artifact_name,
            "artifact_path": success.artifact_path,
            "profile_label": success.profile_label,
            "output_path": success.output_path,
        })),
        "rows": describe_app_rows_snapshot(state),
    })
}

fn describe_app_rows_snapshot(state: &MakerUiState) -> Value {
    json!({
        "saved_setups": state.saved_setups.iter().map(|summary| json!({
            "setup_id": summary.setup_id,
            "name": summary.name,
            "slug": summary.slug,
            "journey_stage": summary.journey_stage.label(),
            "selected": summary.setup_id == state.current_setup.setup_id,
            "path": summary.path,
        })).collect::<Vec<_>>(),
        "recent_artifacts": state.recent_artifacts.iter().map(|artifact| json!({
            "title": artifact.title,
            "subtitle": artifact.subtitle,
            "path": artifact.path,
        })).collect::<Vec<_>>(),
    })
}

fn toggle_maximized(mut state: Signal<MakerUiState>) {
    let next = !window().is_maximized();
    window().toggle_maximized();
    state.with_mut(|ui| ui.maximized = next);
}

fn handle_keydown(evt: KeyboardEvent, mut state: Signal<MakerUiState>) {
    if evt.key() == Key::Escape {
        state.with_mut(|ui| {
            ui.alt_overlay_active = false;
            ui.appearance_panel_open = false;
        });
    }
    if evt.modifiers().contains(Modifiers::ALT) {
        state.with_mut(|ui| ui.alt_overlay_active = true);
        match evt.key() {
            Key::Character(ref key) if key.eq_ignore_ascii_case("n") => {
                evt.prevent_default();
                state.with_mut(|ui| ui.start_another_setup());
                let _ = document::eval("document.getElementById('maker-setup-name')?.focus?.();");
            }
            Key::Character(ref key) if key.eq_ignore_ascii_case("b") => {
                evt.prevent_default();
                start_build(state);
            }
            Key::Character(ref key) if key.eq_ignore_ascii_case("t") => {
                evt.prevent_default();
                state.with_mut(|ui| {
                    ui.utility_pane_open = !ui.utility_pane_open;
                    ui.shell_settings.utility_pane_open = ui.utility_pane_open;
                    ui.persist_shell_settings();
                });
            }
            Key::Character(ref key) if key.eq_ignore_ascii_case("s") => {
                evt.prevent_default();
                let _ = document::eval("document.getElementById('maker-setup-name')?.focus?.();");
            }
            _ => {}
        }
    }
}

fn handle_keyup(evt: KeyboardEvent, mut state: Signal<MakerUiState>) {
    if evt.key() == Key::Alt || !evt.modifiers().contains(Modifiers::ALT) {
        state.with_mut(|ui| ui.alt_overlay_active = false);
    }
}

fn update_setup_name(mut state: Signal<MakerUiState>, value: String) {
    state.with_mut(|ui| {
        ui.current_setup.setup.name = value;
        ui.set_journey_stage(JourneyStage::Personalize);
        ui.success_state = None;
        ui.refresh_previews();
    });
}

fn update_hostname(mut state: Signal<MakerUiState>, value: String) {
    state.with_mut(|ui| {
        ui.current_setup.setup.personalization.hostname = value;
        ui.set_journey_stage(JourneyStage::Personalize);
        ui.success_state = None;
        ui.refresh_previews();
    });
}

fn update_artifacts_dir(mut state: Signal<MakerUiState>, value: String) {
    state.with_mut(|ui| {
        ui.artifacts_dir = value;
        ui.set_journey_stage(JourneyStage::Review);
        ui.success_state = None;
        ui.refresh_previews();
        ui.refresh_recent_artifacts();
    });
}

fn update_repo_root(mut state: Signal<MakerUiState>, value: String) {
    state.with_mut(|ui| {
        ui.repo_root = value;
        ui.set_journey_stage(JourneyStage::Review);
        ui.success_state = None;
        ui.refresh_previews();
    });
}

fn update_profile(mut state: Signal<MakerUiState>, value: BuildProfile) {
    state.with_mut(|ui| {
        ui.current_setup.setup.profile_override = Some(value);
        ui.set_journey_stage(JourneyStage::Profile);
        ui.success_state = None;
        ui.refresh_previews();
    });
}

fn toggle_nvidia(mut state: Signal<MakerUiState>) {
    state.with_mut(|ui| {
        ui.current_setup.setup.hardware.with_nvidia = !ui.current_setup.setup.hardware.with_nvidia;
        ui.set_journey_stage(JourneyStage::Profile);
        ui.success_state = None;
        ui.refresh_previews();
    });
}

fn toggle_lts(mut state: Signal<MakerUiState>) {
    state.with_mut(|ui| {
        ui.current_setup.setup.hardware.with_lts = !ui.current_setup.setup.hardware.with_lts;
        ui.set_journey_stage(JourneyStage::Profile);
        ui.success_state = None;
        ui.refresh_previews();
    });
}

fn build_summary(state: &MakerUiState) -> String {
    if state.build_running {
        "Structured events stream here while the build is active.".to_owned()
    } else if let Some(success) = state.success_state.as_ref() {
        format!("{} · {}", success.title, success.profile_label)
    } else {
        "No build in flight.".to_owned()
    }
}

fn titlebar_status_text(state: &MakerUiState) -> String {
    if state.build_running {
        "Build in flight".to_owned()
    } else if let Some(success) = state.success_state.as_ref() {
        success.title.clone()
    } else if state.build_status.to_ascii_lowercase().contains("failed") {
        "Build needs attention".to_owned()
    } else if !state.build_result.trim().is_empty() {
        "Needs attention".to_owned()
    } else {
        match state.current_setup.journey_stage {
            JourneyStage::Outcome => "Choose the machine intent".to_owned(),
            JourneyStage::Profile => "Set the build posture".to_owned(),
            JourneyStage::Personalize => "Personalize the machine".to_owned(),
            JourneyStage::Review => "Review the native plan".to_owned(),
            JourneyStage::Build => "Ready to build".to_owned(),
            JourneyStage::Boot => "Artifact ready to boot".to_owned(),
        }
    }
}

fn profile_title_label(profile: BuildProfile) -> &'static str {
    match profile {
        BuildProfile::Server => "Server",
        BuildProfile::Kde => "KDE",
        BuildProfile::Both => "Both",
    }
}

fn default_truth_mode_for_stage(stage: JourneyStage) -> RightPanelMode {
    match stage {
        JourneyStage::Outcome | JourneyStage::Profile | JourneyStage::Personalize => {
            RightPanelMode::Config
        }
        JourneyStage::Review => RightPanelMode::Plan,
        JourneyStage::Build | JourneyStage::Boot => RightPanelMode::Build,
    }
}

fn previous_journey_stage(stage: JourneyStage) -> Option<JourneyStage> {
    match stage {
        JourneyStage::Outcome => None,
        JourneyStage::Profile => Some(JourneyStage::Outcome),
        JourneyStage::Personalize => Some(JourneyStage::Profile),
        JourneyStage::Review => Some(JourneyStage::Personalize),
        JourneyStage::Build => Some(JourneyStage::Review),
        JourneyStage::Boot => Some(JourneyStage::Build),
    }
}

fn next_journey_stage(stage: JourneyStage) -> Option<JourneyStage> {
    match stage {
        JourneyStage::Outcome => Some(JourneyStage::Profile),
        JourneyStage::Profile => Some(JourneyStage::Personalize),
        JourneyStage::Personalize => Some(JourneyStage::Review),
        JourneyStage::Review => Some(JourneyStage::Build),
        JourneyStage::Build | JourneyStage::Boot => None,
    }
}

fn journey_stages() -> [JourneyStage; 6] {
    [
        JourneyStage::Outcome,
        JourneyStage::Profile,
        JourneyStage::Personalize,
        JourneyStage::Review,
        JourneyStage::Build,
        JourneyStage::Boot,
    ]
}

fn stage_precedes(candidate: JourneyStage, current: JourneyStage) -> bool {
    journey_stages()
        .iter()
        .position(|stage| *stage == candidate)
        .zip(journey_stages().iter().position(|stage| *stage == current))
        .map(|(candidate_idx, current_idx)| candidate_idx < current_idx)
        .unwrap_or(false)
}

fn stage_headline(stage: JourneyStage) -> (&'static str, &'static str) {
    match stage {
        JourneyStage::Outcome => (
            "What do you want to build?",
            "Start with the thing you are actually trying to make. This first choice is not permanent, it just gives the build studio the right posture before you tune the details.",
        ),
        JourneyStage::Profile => (
            "Set the build posture.",
            "Choose whether this artifact lands as server, KDE, or both, then decide whether the hardware path needs NVIDIA or LTS bias before you continue.",
        ),
        JourneyStage::Personalize => (
            "Give the machine a stable identity.",
            "Name the setup, set the hostname, and make the future artifact feel deliberate before you save or build anything.",
        ),
        JourneyStage::Review => (
            "Review the truthful inputs.",
            "Check the artifact destination and optional repo root while Shell Truth keeps the native config and build plan visible on the right.",
        ),
        JourneyStage::Build => (
            "Launch the artifact path.",
            "Build locally on Linux or export a truthful handoff bundle elsewhere. The main canvas stays calm while the structured build truth streams in the utility rail.",
        ),
        JourneyStage::Boot => (
            "Hand off the artifact.",
            "This is the moment after a truthful build or export, where the app should help you reveal what was created and move toward the next machine.",
        ),
    }
}

fn stage_reassurance_copy(stage: JourneyStage) -> &'static str {
    match stage {
        JourneyStage::Outcome => {
            "Start simple. You can change the intent, profile, and hardware choices later."
        }
        JourneyStage::Profile => {
            "You are only choosing the landing posture here. The emitted config will stay visible on the right."
        }
        JourneyStage::Personalize => {
            "Give the machine a stable name now, then review the exact inputs before you launch anything."
        }
        JourneyStage::Review => {
            "This is the calm truth check. If it looks honest here, the build step becomes straightforward."
        }
        JourneyStage::Build => {
            "This is the launch moment. The next screen is the handoff, not another maze of settings."
        }
        JourneyStage::Boot => {
            "You made the artifact. From here the app should help you reveal it and move on cleanly."
        }
    }
}

fn stage_footer_copy(stage: JourneyStage) -> &'static str {
    match stage {
        JourneyStage::Outcome => {
            "Pick the goal first, then continue into posture. Nothing is locked yet."
        }
        JourneyStage::Profile => {
            "Set the artifact profile clearly, then move into identity and naming."
        }
        JourneyStage::Personalize => {
            "Keep the setup and hostname clean, then review the exact emitted inputs."
        }
        JourneyStage::Review => "Save if needed, then continue when the right rail looks honest.",
        JourneyStage::Build => {
            "Run the build when ready. After that, the app should switch into artifact handoff."
        }
        JourneyStage::Boot => {
            "Boot is the handoff moment. Return to Build if you need to inspect or regenerate the output."
        }
    }
}

fn journey_stage_index(stage: JourneyStage) -> usize {
    journey_stages()
        .iter()
        .position(|candidate| *candidate == stage)
        .unwrap_or(0)
}

fn journey_stage_progress_percent(stage: JourneyStage) -> f32 {
    let total = journey_stages().len().max(1) as f32;
    ((journey_stage_index(stage) as f32 + 1.0) / total) * 100.0
}

fn hardware_summary(state: &MakerUiState) -> String {
    let mut parts = Vec::new();
    if state.current_setup.setup.hardware.with_nvidia {
        parts.push("NVIDIA");
    }
    if state.current_setup.setup.hardware.with_lts {
        parts.push("LTS");
    }
    if parts.is_empty() {
        "Standard".to_owned()
    } else {
        parts.join(" + ")
    }
}

fn build_mode_label() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Local Docker build",
        _ => "Export-only handoff",
    }
}

fn launch_action_label() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Run Build",
        _ => "Export Bundle",
    }
}

fn launch_running_label() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Building…",
        _ => "Exporting…",
    }
}

fn split_release_suffix(name: &str) -> Option<(&str, &str)> {
    let (prefix, suffix) = name.rsplit_once(' ')?;
    let bytes = suffix.as_bytes();
    if bytes.len() != 15 || bytes[8] != b'-' {
        return None;
    }
    let is_digits = |slice: &[u8]| slice.iter().all(|byte| byte.is_ascii_digit());
    if is_digits(&bytes[..8]) && is_digits(&bytes[9..]) {
        Some((prefix, suffix))
    } else {
        None
    }
}

fn sidebar_setup_primary(name: &str) -> String {
    split_release_suffix(name)
        .map(|(prefix, _)| prefix.to_owned())
        .unwrap_or_else(|| name.to_owned())
}

fn sidebar_setup_secondary(name: &str, fallback: &str) -> String {
    split_release_suffix(name)
        .map(|(_, suffix)| suffix.to_owned())
        .unwrap_or_else(|| fallback.to_owned())
}

fn sidebar_setup_leaf_label(name: &str, fallback: &str) -> String {
    let secondary = sidebar_setup_secondary(name, fallback);
    if secondary == fallback {
        sidebar_setup_primary(name)
    } else {
        secondary
    }
}

fn build_sidebar_tree_rows(state: &MakerUiState) -> Vec<SidebarTreeRow> {
    let mut entries = Vec::new();
    let mut visible_per_path = std::collections::BTreeMap::<String, usize>::new();
    for summary in state.saved_setups.iter().cloned() {
        let Ok(document) = state.app.setup_store().load(&summary.setup_id) else {
            continue;
        };
        let effective_profile = document
            .setup
            .profile_override
            .unwrap_or_else(|| document.setup.preset.recommended_profile());
        let mut path = vec![
            document.setup.preset.slug().to_owned(),
            effective_profile.slug().to_owned(),
        ];
        let mut flags = Vec::new();
        if document.setup.hardware.with_nvidia {
            flags.push("with-nvidia".to_owned());
        }
        if document.setup.hardware.enable_intel_arc_sriov {
            flags.push("arc-sriov".to_owned());
        }
        if document.setup.hardware.with_lts {
            flags.push("lts".to_owned());
        }
        if flags.is_empty() {
            path.push("base".to_owned());
        } else {
            path.extend(flags);
        }
        let path_key = path.join("/");
        let selected = summary.setup_id == state.current_setup.setup_id;
        let visible_count = visible_per_path.entry(path_key).or_insert(0);
        if !selected && *visible_count >= 3 {
            continue;
        }
        *visible_count += 1;
        entries.push((
            path,
            summary.modified_unix_secs,
            summary.setup_id.clone(),
            sidebar_setup_leaf_label(&summary.name, &summary.slug),
            selected,
        ));
    }
    entries.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
    });

    let mut rows = Vec::new();
    let mut previous_path: Vec<String> = Vec::new();
    for (path, _modified, setup_id, label, selected) in entries {
        let shared = previous_path
            .iter()
            .zip(path.iter())
            .take_while(|(left, right)| left == right)
            .count();
        for depth in shared..path.len() {
            if has_collapsed_ancestor(&state.collapsed_tree_nodes, &path, depth) {
                break;
            }
            let folder_path = path[..=depth].join("/");
            rows.push(SidebarTreeRow::Folder {
                key: format!("folder:{folder_path}"),
                label: path[depth].clone(),
                depth,
                expanded: !state.collapsed_tree_nodes.contains(&folder_path),
            });
        }
        if has_collapsed_ancestor(&state.collapsed_tree_nodes, &path, path.len()) {
            previous_path = path;
            continue;
        }
        rows.push(SidebarTreeRow::Setup {
            key: format!("setup:{setup_id}"),
            setup_id,
            label,
            depth: path.len(),
            selected,
        });
        previous_path = path;
    }
    rows
}

fn has_collapsed_ancestor(collapsed: &BTreeSet<String>, path: &[String], depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    (0..depth).any(|index| collapsed.contains(&path[..=index].join("/")))
}

fn latest_result_summary(state: &MakerUiState) -> String {
    if let Some(success) = state.success_state.as_ref() {
        format!("{} at {}", success.artifact_name, success.output_path)
    } else if state.build_running {
        "Build events are streaming in Shell Truth.".to_owned()
    } else {
        state
            .build_result
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("No result recorded yet.")
            .to_owned()
    }
}

fn load_shell_settings() -> Result<MakerShellSettings> {
    let path = shell_settings_path()?;
    if !path.is_file() {
        return Ok(MakerShellSettings::default());
    }
    let payload = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_slice(&payload)?)
}

fn save_shell_settings(settings: &MakerShellSettings) -> Result<()> {
    let path = shell_settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_vec_pretty(settings)?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, payload)?;
    fs::rename(&temp_path, &path)?;
    Ok(())
}

fn default_authorized_keys_file(document: &SetupDocument) -> Option<PathBuf> {
    let remembered = document
        .setup
        .ssh
        .authorized_keys_file
        .build_value()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if remembered.is_some() {
        return None;
    }

    let home = std::env::var("HOME").ok()?;
    let candidate = PathBuf::from(home).join(".ssh/authorized_keys");
    candidate.is_file().then_some(candidate)
}

fn shell_settings_path() -> Result<PathBuf> {
    Ok(maker_data_root()?.join("shell-settings.json"))
}

fn maker_data_root() -> Result<PathBuf> {
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
        other => Err(anyhow!("unsupported platform for shell settings: {other}")),
    }
}

fn recent_artifact_summaries(manifest: &ArtifactManifest) -> Vec<RecentArtifactSummary> {
    manifest
        .artifacts
        .iter()
        .map(|artifact| RecentArtifactSummary {
            title: artifact_title(artifact),
            subtitle: format!(
                "{} • {}",
                artifact_kind_label(artifact.kind),
                manifest.build_profile.slug()
            ),
            path: artifact.path.clone(),
        })
        .collect()
}

fn artifact_title(artifact: &ArtifactRecord) -> String {
    Path::new(&artifact.path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(match artifact.kind {
            ArtifactKind::Iso => "ISO artifact",
            ArtifactKind::NativeConfig => "Native config",
            ArtifactKind::SetupDocument => "Setup document",
            ArtifactKind::HandoffReadme => "Handoff README",
        })
        .to_owned()
}

fn artifact_kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Iso => "ISO",
        ArtifactKind::NativeConfig => "Config",
        ArtifactKind::SetupDocument => "Setup",
        ArtifactKind::HandoffReadme => "Handoff",
    }
}

fn primary_artifact(manifest: &ArtifactManifest) -> Option<&ArtifactRecord> {
    manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::Iso)
        .or_else(|| manifest.artifacts.first())
}

fn reveal_path(path: &str) -> Result<()> {
    let target = PathBuf::from(path);
    let reveal_target = if target.is_file() {
        target.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        target
    };

    let status = match std::env::consts::OS {
        "linux" => Command::new("xdg-open").arg(&reveal_target).status(),
        "macos" => Command::new("open").arg("-R").arg(path).status(),
        "windows" => Command::new("explorer")
            .arg(format!("/select,{}", path))
            .status(),
        other => {
            return Err(anyhow!("unsupported platform for reveal: {other}"));
        }
    }?;

    if !status.success() {
        return Err(anyhow!("failed to reveal {}", reveal_target.display()));
    }

    Ok(())
}

fn trace_ui(root: &Path, area: &str, action: &str, payload: serde_json::Value) {
    let _ = append_trace_event(root, "maker-ui", area, action, payload);
}

fn current_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as u64
}

fn theme_spec_for_preset(preset: ThemePreset) -> YgguiThemeSpec {
    let mut spec = default_theme_editor_spec();
    match preset {
        ThemePreset::ArcFrost => spec,
        ThemePreset::ArcMint => {
            spec.colors = vec![
                stop("#9fe3d3", 0.16, 0.24, 0.88),
                stop("#7cc8ff", 0.58, 0.22, 0.78),
                stop("#dce7ef", 0.82, 0.78, 0.56),
            ];
            spec
        }
        ThemePreset::ArcSlate => {
            spec.colors = vec![
                stop("#8fa7d4", 0.18, 0.18, 0.84),
                stop("#a8c6c6", 0.72, 0.24, 0.74),
                stop("#d8dde7", 0.78, 0.8, 0.6),
            ];
            spec.brightness = 0.52;
            spec
        }
    }
}

fn theme_matches_preset(spec: &YgguiThemeSpec, preset: ThemePreset) -> bool {
    let reference = theme_spec_for_preset(preset);
    let to_signature = |theme: &YgguiThemeSpec| {
        theme
            .colors
            .iter()
            .map(|stop| stop.color.clone())
            .collect::<Vec<_>>()
    };
    to_signature(spec) == to_signature(&reference)
}

fn stop(color: &str, x: f32, y: f32, alpha: f32) -> YgguiThemeColorStop {
    YgguiThemeColorStop {
        color: color.to_owned(),
        x,
        y,
        alpha,
    }
}

fn normalize_theme_editor_axis(value: f64) -> f32 {
    ((value / THEME_EDITOR_PAD_SIZE).clamp(0.0, 1.0)) as f32
}

fn chrome_palette(is_dark: bool, _accent: &str) -> ChromePalette {
    if is_dark {
        ChromePalette {
            titlebar: "rgba(21,28,35,0.68)",
            text: "#edf4fb",
            muted: "#9fb4c7",
            accent: "#7cc8ff",
            close_hover: "#cf5d5d",
            control_hover: "rgba(255,255,255,0.10)",
            is_dark: true,
        }
    } else {
        ChromePalette {
            titlebar: "rgba(248,251,253,0.76)",
            text: "#25384c",
            muted: "#607489",
            accent: "#5fa8ff",
            close_hover: "#cf5d5d",
            control_hover: "#ebf1f6",
            is_dark: false,
        }
    }
}

fn toast_palette(is_dark: bool, _accent: &str) -> ToastPalette {
    if is_dark {
        ToastPalette {
            text: "#e7eff8",
            muted: "#a3b6c8",
            accent: "#7cc8ff",
            is_dark: true,
        }
    } else {
        ToastPalette {
            text: "#315066",
            muted: "#6b7b8d",
            accent: "#5fa8ff",
            is_dark: false,
        }
    }
}

fn is_dark_theme(theme: UiTheme) -> bool {
    matches!(theme, UiTheme::ZedDark)
}

fn supports_live_blur() -> bool {
    std::env::var_os("WAYLAND_DISPLAY")
        .and_then(|value| (!value.is_empty()).then_some(value))
        .is_some()
}

fn theme_css_variables(theme: UiTheme, accent: &str, blur_supported: bool) -> String {
    if is_dark_theme(theme) {
        let section_bg = if blur_supported {
            "rgba(24,31,39,0.74)"
        } else {
            "rgba(18,24,31,0.92)"
        };
        let card_bg = if blur_supported {
            "rgba(30,39,48,0.78)"
        } else {
            "rgba(29,38,47,0.94)"
        };
        let proof_bg = if blur_supported {
            "rgba(23,31,39,0.58)"
        } else {
            "rgba(23,31,39,0.88)"
        };
        let panel_bg = if blur_supported {
            "rgba(23,31,39,0.82)"
        } else {
            "rgba(20,28,35,0.96)"
        };
        let footer_bg = if blur_supported {
            "rgba(23,31,39,0.74)"
        } else {
            "rgba(19,27,34,0.94)"
        };
        format!(
            "--maker-accent:{accent};\
             --maker-accent-soft:color-mix(in srgb, {accent} 16%, transparent);\
             --maker-titlebar-text:#ecf4fb;\
             --maker-titlebar-muted:#d6e2ee;\
             --maker-titlebar-field-bg:rgba(29,37,46,0.74);\
             --maker-titlebar-field-border:rgba(123,145,165,0.32);\
             --maker-hero-title:#f3f8fc;\
             --maker-hero-copy:#bfd0df;\
             --maker-section-title:#eef5fb;\
             --maker-text-strong:#ebf3fb;\
             --maker-copy:#bacada;\
             --maker-muted:#a9b8c8;\
             --maker-note:#97abbe;\
             --maker-label:#8fa8bc;\
             --maker-stat-label:#8ca3b8;\
             --maker-stat-value:#edf4fb;\
             --maker-section-bg:{section_bg};\
             --maker-section-border:rgba(255,255,255,0.08);\
             --maker-section-shadow:0 18px 42px rgba(0,0,0,0.24);\
             --maker-card-bg:{card_bg};\
             --maker-card-border:rgba(137,157,177,0.22);\
             --maker-proof-bg:{proof_bg};\
             --maker-proof-border:rgba(132,151,170,0.20);\
             --maker-input-bg:rgba(20,26,33,0.80);\
             --maker-input-border:rgba(133,152,170,0.24);\
             --maker-input-text:#edf4fb;\
             --maker-empty-bg:rgba(27,35,43,0.72);\
             --maker-empty-border:rgba(132,151,170,0.22);\
             --maker-status-bg:rgba(31,46,58,0.68);\
             --maker-status-border:rgba(95,133,161,0.26);\
             --maker-status-text:#edf4fb;\
             --maker-status-muted:#adc0d1;\
             --maker-panel-bg:{panel_bg};\
             --maker-panel-border:rgba(132,151,170,0.26);\
             --maker-panel-text:#deebf7;\
             --maker-secondary-bg:rgba(255,255,255,0.06);\
             --maker-secondary-border:rgba(161,179,196,0.24);\
             --maker-secondary-text:#e2edf7;\
             --maker-tertiary-bg:transparent;\
             --maker-tertiary-border:rgba(161,179,196,0.22);\
             --maker-tertiary-text:#bfd1e3;\
             --maker-stage-complete-bg:rgba(255,255,255,0.08);\
             --maker-stage-complete-text:#e2edf7;\
             --maker-stage-inactive-bg:rgba(255,255,255,0.08);\
             --maker-stage-inactive-text:#dce7f1;\
             --maker-rail-selected-bg:color-mix(in srgb, {accent} 26%, rgba(24,31,39,0.78));\
             --maker-rail-selected-border:color-mix(in srgb, {accent} 70%, rgba(128,154,178,0.20));\
             --maker-rail-card-bg:rgba(22,29,36,0.72);\
             --maker-rail-card-border:rgba(128,154,178,0.20);\
             --maker-rail-meta-bg:rgba(22,29,36,0.62);\
             --maker-rail-gradient:linear-gradient(90deg, rgba(255,255,255,0.02) 0%, rgba(33,43,53,0.18) 14%, rgba(22,29,36,0.58) 100%);\
             --maker-footer-bg:{footer_bg};\
             --maker-footer-border:rgba(255,255,255,0.08);"
        )
    } else {
        let section_bg = if blur_supported {
            "rgba(250,252,254,0.92)"
        } else {
            "rgba(248,250,252,0.98)"
        };
        let card_bg = if blur_supported {
            "rgba(255,255,255,0.90)"
        } else {
            "rgba(255,255,255,0.98)"
        };
        let proof_bg = if blur_supported {
            "rgba(255,255,255,0.68)"
        } else {
            "rgba(250,252,254,0.94)"
        };
        let panel_bg = if blur_supported {
            "rgba(255,255,255,0.95)"
        } else {
            "rgba(252,253,254,0.99)"
        };
        let footer_bg = if blur_supported {
            "rgba(250,252,254,0.94)"
        } else {
            "rgba(248,250,252,0.99)"
        };
        format!(
            "--maker-accent:{accent};\
             --maker-accent-soft:color-mix(in srgb, {accent} 14%, transparent);\
             --maker-titlebar-text:#30465d;\
             --maker-titlebar-muted:#768ba0;\
             --maker-titlebar-field-bg:rgba(255,255,255,0.90);\
             --maker-titlebar-field-border:rgba(188,204,220,0.60);\
             --maker-hero-title:#1f3347;\
             --maker-hero-copy:#405568;\
             --maker-section-title:#1f3346;\
             --maker-text-strong:#294158;\
             --maker-copy:#52677d;\
             --maker-muted:#73869a;\
             --maker-note:#6f8398;\
             --maker-label:#61758b;\
             --maker-stat-label:#667b90;\
             --maker-stat-value:#243a50;\
             --maker-section-bg:{section_bg};\
             --maker-section-border:rgba(255,255,255,0.76);\
             --maker-section-shadow:0 18px 42px rgba(88,107,129,0.09);\
             --maker-card-bg:{card_bg};\
             --maker-card-border:rgba(192,206,220,0.54);\
             --maker-proof-bg:{proof_bg};\
             --maker-proof-border:rgba(198,210,222,0.42);\
             --maker-input-bg:rgba(255,255,255,0.96);\
             --maker-input-border:rgba(194,206,218,0.60);\
             --maker-input-text:#30475f;\
             --maker-empty-bg:rgba(255,255,255,0.84);\
             --maker-empty-border:rgba(194,206,220,0.50);\
             --maker-status-bg:rgba(241,247,252,0.96);\
             --maker-status-border:rgba(186,203,219,0.58);\
             --maker-status-text:#30475f;\
             --maker-status-muted:#7b8da1;\
             --maker-panel-bg:{panel_bg};\
             --maker-panel-border:rgba(190,204,218,0.62);\
             --maker-panel-text:#33495f;\
             --maker-secondary-bg:rgba(255,255,255,0.86);\
             --maker-secondary-border:rgba(188,203,217,0.52);\
             --maker-secondary-text:#35516a;\
             --maker-tertiary-bg:transparent;\
             --maker-tertiary-border:rgba(188,203,217,0.48);\
             --maker-tertiary-text:#4d657d;\
             --maker-stage-complete-bg:rgba(236,243,249,0.96);\
             --maker-stage-complete-text:#39546c;\
             --maker-stage-inactive-bg:rgba(255,255,255,0.82);\
             --maker-stage-inactive-text:#5d7187;\
             --maker-rail-selected-bg:color-mix(in srgb, {accent} 22%, rgba(255,255,255,0.98));\
             --maker-rail-selected-border:color-mix(in srgb, {accent} 68%, rgba(159,186,215,0.34));\
             --maker-rail-card-bg:rgba(255,255,255,0.90);\
             --maker-rail-card-border:rgba(198,210,222,0.52);\
             --maker-rail-meta-bg:rgba(255,255,255,0.80);\
             --maker-rail-gradient:linear-gradient(90deg, rgba(255,255,255,0.02) 0%, rgba(245,249,253,0.38) 16%, rgba(245,249,253,0.76) 100%);\
             --maker-footer-bg:{footer_bg};\
             --maker-footer-border:rgba(255,255,255,0.76);"
        )
    }
}

fn shell_surface_style(
    maximized: bool,
    finish: ShellFinish,
    shell_tint_fill: &str,
    shell_gradient: &str,
    blur_supported: bool,
) -> String {
    let radius = if maximized { 0 } else { 10 };
    let blur = match finish {
        ShellFinish::Sleek if blur_supported => 10,
        ShellFinish::Crisp => 0,
        ShellFinish::Sleek => 0,
    };
    let saturation = match finish {
        ShellFinish::Sleek => 135,
        ShellFinish::Crisp => 100,
    };
    let frame_outline = if maximized {
        "none"
    } else {
        "inset 0 0 0 1px rgba(204,214,226,0.84)"
    };
    let shadow = if maximized {
        "0 24px 52px rgba(72,102,118,0.16)".to_owned()
    } else {
        format!("0 24px 52px rgba(72,102,118,0.16), {}", frame_outline)
    };
    let backdrop = if blur == 0 {
        "none".to_owned()
    } else {
        format!("blur({blur}px) saturate({saturation}%)")
    };
    format!(
        "position:absolute; inset:{}px; display:flex; flex-direction:column; overflow:hidden; \
         border-radius:{}px; background-color:{}; background-image:{}; box-shadow:{}; \
         backdrop-filter:{}; -webkit-backdrop-filter:{};",
        if maximized { 0 } else { 8 },
        radius,
        shell_tint_fill,
        shell_gradient,
        shadow,
        backdrop,
        backdrop,
    )
}

fn left_rail_container_style() -> &'static str {
    "display:flex; flex-direction:column; position:relative; height:100%; overflow:hidden; background:transparent;"
}

fn right_rail_container_style() -> &'static str {
    "display:flex; flex-direction:column; height:100%; margin-left:0; padding-left:0; background:transparent; box-shadow:none;"
}

fn small_chip_style(selected: bool, accent: &str) -> String {
    if selected {
        format!(
            "height:28px; padding:0 10px; border:none; border-radius:10px; background:{}; color:white; font-size:11px; font-weight:700;",
            accent
        )
    } else {
        utility_button_style(false)
    }
}

fn utility_button_style(active: bool) -> String {
    format!(
        "display:inline-flex; align-items:center; gap:8px; height:28px; padding:0 11px; border:none; border-radius:10px; \
         background:{}; color:{}; font-size:11px; font-weight:700; white-space:nowrap; box-shadow:{};",
        if active {
            "var(--maker-secondary-bg)"
        } else {
            "transparent"
        },
        if active {
            "var(--maker-accent)"
        } else {
            "var(--maker-titlebar-muted)"
        },
        if active {
            "inset 0 0 0 1px var(--maker-secondary-border)"
        } else {
            "none"
        }
    )
}

fn utility_icon_button_style(active: bool) -> String {
    format!(
        "display:inline-flex; align-items:center; justify-content:center; width:28px; height:28px; border:none; border-radius:8px; \
         background:{}; color:{}; box-shadow:{};",
        if active {
            "color-mix(in srgb, var(--maker-accent) 10%, transparent)"
        } else {
            "transparent"
        },
        if active {
            "var(--maker-accent)"
        } else {
            "var(--maker-titlebar-muted)"
        },
        if active {
            "inset 0 0 0 1px var(--maker-secondary-border)"
        } else {
            "none"
        }
    )
}

fn titlebar_icon_button_style(active: bool) -> String {
    format!(
        "display:inline-flex; align-items:center; justify-content:center; width:28px; height:28px; border:none; border-radius:8px; \
         background:{}; color:{}; opacity:{}; font-size:15px; font-weight:800; box-shadow:{};",
        if active {
            "color-mix(in srgb, var(--maker-accent) 10%, transparent)"
        } else {
            "transparent"
        },
        if active {
            "var(--maker-accent)"
        } else {
            "var(--maker-titlebar-muted)"
        },
        if active {
            "inset 0 0 0 1px var(--maker-secondary-border)"
        } else {
            "none"
        },
        if active { "1" } else { "0.98" }
    )
}

fn titlebar_setup_button_style() -> &'static str {
    "display:flex; align-items:center; width:min(360px, 100%); min-width:0; height:32px; padding:0 4px; border:none; border-radius:0; background:transparent; box-shadow:none;"
}

fn titlebar_center_field_style() -> &'static str {
    "display:flex; align-items:center; justify-content:center; gap:10px; width:100%; min-width:0; height:32px; padding:0 8px; border-radius:0; background:transparent; box-shadow:none; overflow:hidden;"
}

fn utility_tab_style(selected: bool, accent: &str) -> String {
    if selected {
        format!(
            "flex:1; height:30px; border:none; border-radius:0; background:transparent; color:var(--maker-text-strong); font-size:11px; font-weight:700; box-shadow:inset 0 -2px 0 {};",
            accent
        )
    } else {
        "flex:1; height:30px; border:none; border-radius:0; background:transparent; color:var(--maker-secondary-text); font-size:11px; font-weight:700; box-shadow:none;".to_owned()
    }
}

fn viewport_header_style(compact: bool) -> &'static str {
    if compact {
        "display:flex; flex-direction:column; gap:0; padding:4px 4px 2px 4px;"
    } else {
        "display:flex; flex-direction:column; gap:0; padding:6px 4px 4px 4px;"
    }
}

fn header_meta_chip_style() -> &'static str {
    "display:flex; flex-direction:column; gap:3px; min-width:110px; padding:10px 12px; border-radius:12px; background:color-mix(in srgb, var(--maker-card-bg) 66%, transparent); box-shadow:inset 0 0 0 1px color-mix(in srgb, var(--maker-card-border) 78%, transparent);"
}

fn stage_pill_style(active: bool, complete: bool, accent: &str) -> String {
    if active {
        format!(
            "display:inline-flex; align-items:center; justify-content:center; height:30px; padding:0 2px; border:none; border-radius:0; background:transparent; color:var(--maker-text-strong); font-size:11px; font-weight:800; box-shadow:inset 0 -2px 0 {};",
            accent
        )
    } else if complete {
        "display:inline-flex; align-items:center; justify-content:center; height:30px; padding:0 2px; border:none; border-radius:0; background:transparent; color:var(--maker-stage-complete-text); font-size:11px; font-weight:800; box-shadow:none;".to_owned()
    } else {
        "display:inline-flex; align-items:center; justify-content:center; height:30px; padding:0 2px; border:none; border-radius:0; background:transparent; color:var(--maker-stage-inactive-text); font-size:11px; font-weight:700; box-shadow:none;".to_owned()
    }
}

fn shortcut_badge_style() -> &'static str {
    "display:inline-flex; align-items:center; justify-content:center; min-width:16px; height:16px; padding:0 4px; border-radius:6px; background:color-mix(in srgb, var(--maker-accent) 16%, transparent); color:var(--maker-accent); font-size:9px; font-weight:800;"
}

fn primary_button_style(accent: &str) -> String {
    format!(
        "display:inline-flex; align-items:center; gap:8px; height:34px; padding:0 14px; border:none; border-radius:10px; background:{}; color:white; font-size:11px; font-weight:800; box-shadow:0 10px 22px color-mix(in srgb, {} 32%, transparent);",
        accent, accent
    )
}

fn guided_primary_button_style(accent: &str) -> String {
    format!(
        "display:inline-flex; align-items:center; gap:8px; height:36px; padding:0 16px; border:none; border-radius:10px; background:{}; color:white; font-size:11px; font-weight:800; box-shadow:0 10px 26px color-mix(in srgb, {} 30%, transparent); animation:makerPulseGlow 2.6s ease-in-out infinite;",
        accent, accent
    )
}

fn secondary_button_style() -> &'static str {
    "display:inline-flex; align-items:center; gap:8px; height:38px; padding:0 16px; border:none; border-radius:12px; background:var(--maker-secondary-bg); color:var(--maker-secondary-text); font-size:12px; font-weight:800; box-shadow:inset 0 0 0 1px var(--maker-secondary-border);"
}

fn tertiary_button_style() -> &'static str {
    "display:inline-flex; align-items:center; gap:8px; height:38px; padding:0 16px; border:none; border-radius:10px; background:var(--maker-tertiary-bg); color:var(--maker-tertiary-text); font-size:12px; font-weight:800; box-shadow:inset 0 0 0 1px var(--maker-tertiary-border);"
}

fn stage_footer_bar_style() -> &'static str {
    "display:flex; flex-wrap:wrap; gap:12px; justify-content:space-between; align-items:center; padding:12px 16px; border-radius:18px; background:var(--maker-footer-bg); box-shadow:0 12px 28px rgba(88,107,129,0.08), inset 0 0 0 1px var(--maker-footer-border);"
}

fn primary_rail_button_style(accent: &str) -> String {
    format!(
        "display:inline-flex; align-items:center; gap:8px; justify-content:center; width:100%; height:38px; border:none; border-radius:10px; background:{}; color:white; font-size:12px; font-weight:800; box-shadow:0 10px 26px color-mix(in srgb, {} 36%, transparent);",
        accent, accent
    )
}

fn rail_setup_card_style(selected: bool, depth: usize) -> String {
    let indent = 12 + depth.saturating_sub(1) * 14;
    if selected {
        format!(
            "appearance:none; -webkit-appearance:none; display:flex; align-items:center; gap:8px; width:100%; min-height:24px; \
             border:none; border-radius:0; padding:0 10px 0 {}px; background:transparent; color:var(--maker-accent); \
             box-shadow:none; outline:none;",
            indent
        )
    } else {
        format!(
            "appearance:none; -webkit-appearance:none; display:flex; align-items:center; gap:8px; width:100%; min-height:24px; \
             border:none; border-radius:0; padding:0 10px 0 {}px; background:transparent; color:var(--maker-note); \
             box-shadow:none; outline:none;",
            indent
        )
    }
}

fn tree_folder_row_style(depth: usize) -> String {
    let indent = 6 + depth * 14;
    format!(
        "appearance:none; -webkit-appearance:none; display:flex; align-items:center; gap:8px; width:100%; min-height:24px; \
         border:none; border-radius:0; background:transparent; padding:0 6px 0 {}px; color:var(--maker-note); \
         font-size:11px; font-weight:700; text-transform:none; box-shadow:none; outline:none;",
        indent
    )
}

fn tree_folder_label_style() -> &'static str {
    "color:var(--maker-note); font-size:11px; font-weight:700; text-transform:none;"
}

fn rail_setup_label_style(selected: bool) -> &'static str {
    if selected {
        "min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:11px; font-weight:700; color:var(--maker-accent); text-align:left;"
    } else {
        "min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:11px; font-weight:700; color:var(--maker-note); text-align:left;"
    }
}

fn rail_meta_card_style() -> &'static str {
    "display:flex; flex-direction:column; gap:4px; width:100%; border:none; border-radius:12px; padding:10px 11px; background:var(--maker-rail-meta-bg); box-shadow:inset 0 0 0 1px var(--maker-rail-card-border);"
}

fn section_toggle_style(expanded: bool) -> String {
    if expanded {
        "display:flex; align-items:center; justify-content:space-between; gap:8px; width:100%; border:none; border-radius:10px; padding:10px 12px; background:var(--maker-rail-selected-bg); color:var(--maker-text-strong); font-size:12px; font-weight:800;".to_owned()
    } else {
        "display:flex; align-items:center; justify-content:space-between; gap:8px; width:100%; border:none; border-radius:10px; padding:10px 12px; background:var(--maker-rail-meta-bg); color:var(--maker-text-strong); font-size:12px; font-weight:800; box-shadow:inset 0 0 0 1px var(--maker-rail-card-border);".to_owned()
    }
}

fn section_card_style() -> &'static str {
    "display:flex; flex-direction:column; gap:12px; padding:18px 20px 18px 20px; border-radius:18px; background:var(--maker-section-bg); box-shadow:var(--maker-section-shadow), inset 0 0 0 1px var(--maker-section-border); backdrop-filter:blur(10px); -webkit-backdrop-filter:blur(10px);"
}

fn selected_intent_card_style(accent: &str) -> String {
    format!(
        "display:flex; flex-direction:column; gap:14px; padding:18px 18px 18px 18px; border-radius:18px; \
         background:radial-gradient(circle at top right, color-mix(in srgb, {} 14%, white) 0%, rgba(255,255,255,0) 36%), \
         linear-gradient(180deg, color-mix(in srgb, var(--maker-section-bg) 82%, white) 0%, var(--maker-section-bg) 100%); \
         box-shadow:0 18px 44px rgba(88,107,129,0.10), inset 0 0 0 1px var(--maker-card-border), inset 0 1px 0 rgba(255,255,255,0.16);",
        accent,
    )
}

fn secondary_preset_card_style() -> &'static str {
    "display:flex; flex-direction:column; gap:8px; padding:14px 14px 15px 14px; border:none; border-radius:14px; background:var(--maker-card-bg); box-shadow:inset 0 0 0 1px var(--maker-card-border); text-align:left;"
}

fn proof_stack_style() -> &'static str {
    "display:flex; flex-direction:column; gap:10px; padding:14px; border-radius:14px; background:var(--maker-proof-bg); box-shadow:inset 0 0 0 1px var(--maker-proof-border);"
}

fn info_stack_style() -> &'static str {
    "display:flex; flex-direction:column; gap:10px; padding:0;"
}

fn info_row_style() -> &'static str {
    "display:flex; flex-direction:column; gap:8px; padding:16px 2px 16px 2px; border-radius:0; background:transparent; box-shadow:inset 0 -1px 0 var(--maker-card-border);"
}

fn identity_preview_style() -> &'static str {
    "display:flex; flex-direction:column; gap:12px; padding:16px; border-radius:16px; background:linear-gradient(180deg, color-mix(in srgb, var(--maker-section-bg) 86%, white) 0%, var(--maker-section-bg) 100%); box-shadow:0 18px 42px rgba(88,107,129,0.10), inset 0 0 0 1px var(--maker-card-border);"
}

fn proof_card_style() -> &'static str {
    "display:flex; flex-direction:column; gap:6px; padding:13px 14px; border-radius:12px; background:var(--maker-card-bg); box-shadow:inset 0 0 0 1px var(--maker-card-border);"
}

fn option_button_style(selected: bool, accent: &str) -> String {
    if selected {
        format!(
            "height:34px; padding:0 14px; border:none; border-radius:11px; background:{}; color:white; font-size:12px; font-weight:700;",
            accent
        )
    } else {
        "height:34px; padding:0 14px; border:none; border-radius:11px; background:var(--maker-secondary-bg); color:var(--maker-secondary-text); font-size:12px; font-weight:700; box-shadow:inset 0 0 0 1px var(--maker-secondary-border);".to_owned()
    }
}

fn input_style() -> &'static str {
    "height:40px; padding:0 12px; border:none; border-radius:10px; background:var(--maker-input-bg); color:var(--maker-input-text); font-size:13px; box-shadow:inset 0 0 0 1px var(--maker-input-border);"
}

fn label_style() -> &'static str {
    "font-size:11px; font-weight:800; letter-spacing:0.05em; color:var(--maker-label); text-transform:uppercase;"
}

fn section_title_style() -> &'static str {
    "margin:0; font-size:24px; line-height:1.08; font-weight:800; color:var(--maker-section-title);"
}

fn section_copy_style() -> &'static str {
    "margin:0; font-size:13px; line-height:1.58; color:var(--maker-copy);"
}

fn empty_note_style() -> &'static str {
    "padding:12px 13px; border-radius:10px; background:var(--maker-empty-bg); color:var(--maker-muted); font-size:12px; line-height:1.58; box-shadow:inset 0 0 0 1px var(--maker-empty-border);"
}

fn rail_empty_note_style() -> &'static str {
    "padding:10px 12px; border-radius:10px; background:transparent; color:var(--maker-muted); font-size:12px; line-height:1.5; box-shadow:inset 0 0 0 1px var(--maker-empty-border);"
}

fn pre_panel_style() -> &'static str {
    "margin:0; padding:14px 16px 16px 16px; border-radius:12px; background:var(--maker-panel-bg); color:var(--maker-panel-text); font-size:11px; line-height:1.58; white-space:pre-wrap; overflow-wrap:anywhere; box-shadow:inset 0 0 0 1px var(--maker-panel-border);"
}

fn appearance_sidebar_card_style() -> &'static str {
    "display:flex; flex-direction:column; gap:14px; padding:2px 0 0 0;"
}

fn appearance_segment_style() -> &'static str {
    "display:flex; align-items:center; gap:4px; padding:4px; border:none; border-radius:999px; background:var(--maker-secondary-bg); box-shadow: inset 0 0 0 1px var(--maker-secondary-border);"
}

fn appearance_segment_button_style(selected: bool) -> &'static str {
    if selected {
        "flex:1; height:26px; border:none; border-radius:999px; background:var(--maker-card-bg); color:var(--maker-text-strong); font-size:11px; font-weight:700; box-shadow: inset 0 0 0 1px var(--maker-card-border);"
    } else {
        "flex:1; height:26px; border:none; border-radius:999px; background:transparent; color:var(--maker-muted); font-size:11px; font-weight:700;"
    }
}

fn appearance_range_style() -> &'static str {
    "width:100%; height:34px; appearance:none; background:transparent;"
}

fn status_card_style() -> &'static str {
    "display:flex; flex-direction:column; gap:4px; padding:10px 12px; border-radius:12px; background:var(--maker-status-bg); box-shadow:inset 0 0 0 1px var(--maker-status-border);"
}

fn rail_status_card_style() -> &'static str {
    "display:flex; flex-direction:column; gap:6px; padding:12px 12px 12px 12px; border-radius:10px; background:transparent; box-shadow:inset 0 0 0 1px var(--maker-status-border);"
}

fn success_stat_style() -> &'static str {
    "display:flex; flex-direction:column; gap:6px; padding:14px 15px; border-radius:12px; background:var(--maker-card-bg); box-shadow:inset 0 0 0 1px var(--maker-card-border);"
}

fn stat_label_style() -> &'static str {
    "font-size:10px; font-weight:800; letter-spacing:0.08em; text-transform:uppercase; color:var(--maker-stat-label);"
}

fn stat_value_style() -> &'static str {
    "font-size:13px; font-weight:700; line-height:1.45; color:var(--maker-stat-value); overflow-wrap:anywhere;"
}
