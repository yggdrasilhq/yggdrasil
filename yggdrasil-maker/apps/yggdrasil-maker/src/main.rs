use anyhow::{Context, Result};
use build_job::DetachedBuildCompletionRecord;
use clap::{Args, Parser, Subcommand};
use maker_app::{BuildInputs, MakerApp};
use maker_build::{
    ArtifactKind, ArtifactManifest, BuildErrorCode, BuildEvent, BuildMode, BuildStage,
};
use maker_copy::preset_cards;
use maker_model::{BuildProfile, PresetId};
#[cfg(feature = "desktop-ui")]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
#[cfg(feature = "desktop-ui")]
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "desktop-ui")]
mod app_capture;
#[cfg(feature = "desktop-ui")]
mod app_control;
mod build_job;
#[cfg(feature = "desktop-ui")]
mod gui;
#[cfg(all(feature = "desktop-ui", target_os = "linux"))]
mod linux_desktop;
#[cfg(feature = "desktop-ui")]
mod window_icon;

#[derive(Parser, Debug)]
#[command(name = "yggdrasil-maker")]
#[command(about = "GUI-first maker app with a stable automation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Preset(PresetCommand),
    Setup {
        #[command(subcommand)]
        command: SetupCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Build {
        #[command(subcommand)]
        command: BuildCommand,
    },
    #[cfg(feature = "desktop-ui")]
    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },
}

#[derive(Args, Debug)]
struct PresetCommand {
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum SetupCommand {
    New(NewSetupArgs),
    Show(ShowSetupArgs),
}

#[derive(Args, Debug)]
struct NewSetupArgs {
    #[arg(long)]
    name: String,
    #[arg(long)]
    preset: PresetId,
    #[arg(long)]
    profile: Option<BuildProfile>,
    #[arg(long)]
    hostname: Option<String>,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct ShowSetupArgs {
    #[arg(long)]
    input: PathBuf,
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    Emit(EmitConfigArgs),
}

#[derive(Args, Debug)]
struct EmitConfigArgs {
    #[arg(long)]
    setup: PathBuf,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum BuildCommand {
    Plan(PlanBuildArgs),
    Run(RunBuildArgs),
}

#[cfg(feature = "desktop-ui")]
#[derive(Subcommand, Debug)]
enum ServerCommand {
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
}

#[cfg(feature = "desktop-ui")]
#[derive(Subcommand, Debug)]
enum AppCommand {
    Clients,
    State(TimeoutArgs),
    Rows(TimeoutArgs),
    Screenshot(CaptureArgs),
    Screenrecord(ScreenrecordArgs),
    Focus(TimeoutArgs),
    NewSetup(AppNewSetupArgs),
    SelectSetup(AppSelectSetupArgs),
    SaveSetup(TimeoutArgs),
    SetStage(AppSetStageArgs),
    SetSetupName(AppValueArgs),
    SetHostname(AppValueArgs),
    SetArtifactsDir(AppValueArgs),
    SetRepoRoot(AppValueArgs),
    SetBuildContext(AppBuildContextArgs),
    ApplyPreset(AppPresetArgs),
    SetProfile(AppProfileArgs),
    ToggleNvidia(TimeoutArgs),
    ToggleLts(TimeoutArgs),
    SetSidebar(AppBoolArgs),
    SetUtilityPane(AppBoolArgs),
    SetRightPanel(AppRightPanelArgs),
    SetAppearancePanel(AppBoolArgs),
    StartBuild(TimeoutArgs),
    WaitBuild(AppWaitBuildArgs),
    TraceTail(AppTraceTailArgs),
    OpenBuildDetails(TimeoutArgs),
    RevealPrimaryArtifact(TimeoutArgs),
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct TimeoutArgs {
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct CaptureArgs {
    output: Option<PathBuf>,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct ScreenrecordArgs {
    output: Option<PathBuf>,
    #[arg(long, default_value_t = 12)]
    duration_sec: u64,
    #[arg(long, default_value_t = 20_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppNewSetupArgs {
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    preset: Option<PresetId>,
    #[arg(long)]
    profile: Option<BuildProfile>,
    #[arg(long)]
    hostname: Option<String>,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppSelectSetupArgs {
    setup_id: String,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppSetStageArgs {
    stage: maker_model::JourneyStage,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppValueArgs {
    value: String,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppBuildContextArgs {
    #[arg(long)]
    artifacts_dir: PathBuf,
    #[arg(long)]
    repo_root: PathBuf,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppPresetArgs {
    preset: PresetId,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppProfileArgs {
    profile: BuildProfile,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppBoolArgs {
    #[arg(action = clap::ArgAction::Set)]
    open: bool,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppRightPanelArgs {
    mode: gui::RightPanelMode,
    #[arg(long, default_value_t = 8_000)]
    timeout_ms: u64,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppWaitBuildArgs {
    #[arg(long, default_value_t = 900_000)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 250)]
    poll_ms: u64,
    #[arg(long, default_value_t = 120)]
    trace_lines: usize,
    #[arg(long)]
    allow_failure: bool,
}

#[cfg(feature = "desktop-ui")]
#[derive(Args, Debug)]
struct AppTraceTailArgs {
    #[arg(long, default_value_t = 120)]
    lines: usize,
}

#[derive(Args, Debug, Clone)]
struct BuildInputArgs {
    #[arg(long)]
    setup: PathBuf,
    #[arg(long, default_value = "./artifacts")]
    artifacts_dir: PathBuf,
    #[arg(long)]
    authorized_keys_file: Option<PathBuf>,
    #[arg(long)]
    host_keys_dir: Option<PathBuf>,
    #[arg(long)]
    repo_root: Option<PathBuf>,
    #[arg(long)]
    skip_smoke: bool,
}

#[derive(Args, Debug)]
struct PlanBuildArgs {
    #[command(flatten)]
    input: BuildInputArgs,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct RunBuildArgs {
    #[command(flatten)]
    input: BuildInputArgs,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    completion_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = std::env::args_os().collect::<Vec<_>>();
    #[cfg(feature = "desktop-ui")]
    if should_launch_gui(&args) {
        return gui::launch();
    }

    let cli = Cli::parse_from(args);
    let app = MakerApp::new_for_current_platform()?;
    match cli.command {
        Command::Preset(args) => run_preset(args)?,
        Command::Setup { command } => run_setup(&app, command)?,
        Command::Config { command } => run_config(&app, command)?,
        Command::Build { command } => run_build(&app, command)?,
        #[cfg(feature = "desktop-ui")]
        Command::Server { command } => run_server(command)?,
    }
    Ok(())
}

#[cfg(feature = "desktop-ui")]
fn should_launch_gui(args: &[std::ffi::OsString]) -> bool {
    args.len() == 1 || args.get(1).and_then(|arg| arg.to_str()) == Some("gui")
}

fn run_preset(args: PresetCommand) -> Result<()> {
    if args.json {
        println!("{}", serde_json::to_string_pretty(preset_cards())?);
        return Ok(());
    }

    for card in preset_cards() {
        println!(
            "{} ({}) -> {}",
            card.title, card.id, card.recommended_profile
        );
        println!("  {}", card.summary);
    }
    Ok(())
}

fn run_setup(app: &MakerApp, command: SetupCommand) -> Result<()> {
    match command {
        SetupCommand::New(args) => {
            let document =
                app.create_setup_document(args.name, args.preset, args.profile, args.hostname);
            let output_path = args
                .output
                .unwrap_or_else(|| PathBuf::from(document.storage_filename()));
            app.save_setup_path(&output_path, &document)?;
            println!("{}", output_path.display());
        }
        SetupCommand::Show(args) => {
            let document = app.load_setup_path(&args.input)?;
            println!("{}", serde_json::to_string_pretty(&document)?);
        }
    }
    Ok(())
}

fn run_config(app: &MakerApp, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Emit(args) => {
            let document = app.load_setup_path(&args.setup)?;
            let toml = app.emit_config_toml(&document)?;
            if let Some(output) = args.output {
                std::fs::write(&output, toml.as_bytes())
                    .with_context(|| format!("failed to write {}", output.display()))?;
                println!("{}", output.display());
            } else {
                print!("{toml}");
            }
        }
    }
    Ok(())
}

fn run_build(app: &MakerApp, command: BuildCommand) -> Result<()> {
    match command {
        BuildCommand::Plan(args) => {
            let plan = app.plan_build(load_build_inputs(app, args.input)?)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!("mode: {}", display_mode(plan.mode));
                println!("image: {}", plan.image_ref);
                if let Some(repo_root) = plan.repo_root.as_ref() {
                    println!("repo-local mode: {}", repo_root.display());
                }
                println!("input bundle: {}", plan.input_bundle_dir.display());
                println!("config: {}", plan.host_config_path.display());
                println!(
                    "persisted setup: {}",
                    plan.host_persisted_setup_path.display()
                );
                println!("invocation: {}", plan.host_invocation_path.display());
                println!(
                    "artifact manifest: {}",
                    plan.host_artifact_manifest_path.display()
                );
                println!("artifacts: {}", plan.artifacts_dir.display());
                if !plan.docker_command.is_empty() {
                    println!("docker command:");
                    println!("  {}", shell_join(&plan.docker_command));
                }
            }
        }
        BuildCommand::Run(args) => {
            let inputs = load_build_inputs(app, args.input)?;
            let as_json = args.json;
            let completion_file = args.completion_file.clone();
            match app.run_build(inputs.clone(), |event| {
                let _ = emit_local_event(&event, as_json);
            }) {
                Ok(result) => {
                    if let Err(error) =
                        run_host_qemu_smoke_checks(&inputs, &result.manifest, as_json)
                    {
                        emit_local_event(
                            &BuildEvent::Failure {
                                code: BuildErrorCode::SmokeTestFailed,
                                message_key: "host_qemu_smoke_failed".to_owned(),
                                detail: error.to_string(),
                            },
                            as_json,
                        )?;
                        write_completion_file(
                            completion_file.as_ref(),
                            &DetachedBuildCompletionRecord {
                                success: false,
                                completed_at_ms: current_millis_u128(),
                                manifest_path: None,
                                error: Some(error.to_string()),
                            },
                        )?;
                        std::process::exit(1);
                    }
                    write_completion_file(
                        completion_file.as_ref(),
                        &DetachedBuildCompletionRecord {
                            success: true,
                            completed_at_ms: current_millis_u128(),
                            manifest_path: Some(
                                result
                                    .plan
                                    .host_artifact_manifest_path
                                    .display()
                                    .to_string(),
                            ),
                            error: None,
                        },
                    )?;
                    if as_json {
                        println!("{}", serde_json::to_string_pretty(&result.manifest)?);
                    } else {
                        println!(
                            "artifact manifest: {}",
                            result.plan.host_artifact_manifest_path.display()
                        );
                    }
                }
                Err(error) => {
                    if error.to_string().starts_with("Failure { code:") {
                        write_completion_file(
                            completion_file.as_ref(),
                            &DetachedBuildCompletionRecord {
                                success: false,
                                completed_at_ms: current_millis_u128(),
                                manifest_path: None,
                                error: Some(error.to_string()),
                            },
                        )?;
                        std::process::exit(1);
                    }
                    emit_local_event(
                        &BuildEvent::Failure {
                            code: BuildErrorCode::BuildProcessFailed,
                            message_key: "build_failed".to_owned(),
                            detail: error.to_string(),
                        },
                        as_json,
                    )?;
                    write_completion_file(
                        completion_file.as_ref(),
                        &DetachedBuildCompletionRecord {
                            success: false,
                            completed_at_ms: current_millis_u128(),
                            manifest_path: None,
                            error: Some(error.to_string()),
                        },
                    )?;
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(())
}

fn run_host_qemu_smoke_checks(
    inputs: &BuildInputs,
    manifest: &ArtifactManifest,
    as_json: bool,
) -> Result<()> {
    if std::env::consts::OS != "linux"
        || inputs.skip_smoke
        || !inputs.setup_document.setup.smoke.enable_qemu_smoke
        || manifest.mode != BuildMode::LocalDocker
    {
        return Ok(());
    }

    let repo_root = inputs
        .repo_root
        .clone()
        .unwrap_or(std::env::current_dir().context("failed to resolve current repo root")?);
    let ssh_key = resolve_qemu_smoke_private_key()?;
    let smoke_targets = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == ArtifactKind::Iso)
        .filter_map(|artifact| {
            artifact
                .profile
                .map(|profile| (profile, artifact.path.clone()))
        })
        .collect::<Vec<(BuildProfile, String)>>();

    if smoke_targets.is_empty() {
        return Err(anyhow::anyhow!(
            "host QEMU smoke could not find any ISO artifacts to test"
        ));
    }

    emit_local_event(
        &BuildEvent::StageStarted {
            stage: BuildStage::Smoke,
        },
        as_json,
    )?;
    for (profile, iso_path) in smoke_targets {
        emit_local_event(
            &BuildEvent::LogLine {
                stream: "stdout".to_owned(),
                line: format!(
                    "Running host QEMU smoke for {} via {}",
                    profile.slug(),
                    iso_path
                ),
            },
            as_json,
        )?;
        run_host_qemu_smoke_check(&repo_root, &ssh_key, profile, &iso_path, as_json)?;
    }
    emit_local_event(
        &BuildEvent::StageFinished {
            stage: BuildStage::Smoke,
        },
        as_json,
    )?;
    Ok(())
}

fn run_host_qemu_smoke_check(
    repo_root: &PathBuf,
    ssh_key: &PathBuf,
    profile: BuildProfile,
    iso_path: &str,
    as_json: bool,
) -> Result<()> {
    let mode = match profile {
        BuildProfile::Server => "server",
        BuildProfile::Kde => "kde",
        BuildProfile::Both => {
            return Err(anyhow::anyhow!(
                "host QEMU smoke cannot run against combined profile value"
            ));
        }
    };

    let can_sudo = ProcessCommand::new("sudo")
        .arg("-n")
        .arg("true")
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    let mut command = if can_sudo {
        let mut command = ProcessCommand::new("sudo");
        command.arg("-n");
        command.arg("./tests/smoke/boot-qemu.sh");
        command
    } else {
        ProcessCommand::new("./tests/smoke/boot-qemu.sh")
    };
    let output = command
        .current_dir(repo_root)
        .arg("--iso")
        .arg(iso_path)
        .arg("--mode")
        .arg(mode)
        .arg("--ssh-private-key")
        .arg(ssh_key)
        .output()
        .with_context(|| format!("failed to run host smoke for {}", iso_path))?;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if !line.trim().is_empty() {
            emit_local_event(
                &BuildEvent::LogLine {
                    stream: "stdout".to_owned(),
                    line: line.to_owned(),
                },
                as_json,
            )?;
        }
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        if !line.trim().is_empty() {
            emit_local_event(
                &BuildEvent::LogLine {
                    stream: "stderr".to_owned(),
                    line: line.to_owned(),
                },
                as_json,
            )?;
        }
    }

    if output.status.success() {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "host QEMU smoke failed for {} (status {})",
        profile.slug(),
        output.status
    ))
}

fn resolve_qemu_smoke_private_key() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("YGG_QEMU_SSH_PRIVATE_KEY") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(".ssh/id_ed25519");
        if path.is_file() {
            return Ok(path);
        }
    }

    Err(anyhow::anyhow!(
        "host QEMU smoke requires YGG_QEMU_SSH_PRIVATE_KEY or ~/.ssh/id_ed25519"
    ))
}

fn write_completion_file(
    path: Option<&PathBuf>,
    completion: &DetachedBuildCompletionRecord,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let payload = serde_json::to_vec_pretty(completion)?;
    std::fs::write(path, payload).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn current_millis_u128() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(feature = "desktop-ui")]
fn run_server(command: ServerCommand) -> Result<()> {
    match command {
        ServerCommand::App { command } => run_server_app(command),
    }
}

#[cfg(feature = "desktop-ui")]
fn run_server_app(command: AppCommand) -> Result<()> {
    use app_control::{
        AppControlCommand, active_client_instance_records, default_recording_output_path,
        default_screenshot_output_path, request_app_control, resolve_home_dir,
    };
    use yggterm_core::{event_trace_path, read_trace_tail};

    let home = resolve_home_dir()?;
    match command {
        AppCommand::Clients => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "count": active_client_instance_records(&home)?.len(),
                    "clients": active_client_instance_records(&home)?,
                }))?
            );
            Ok(())
        }
        AppCommand::State(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::DescribeState,
            args.timeout_ms,
        )?),
        AppCommand::Rows(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::DescribeRows,
            args.timeout_ms,
        )?),
        AppCommand::Screenshot(args) => {
            let response = request_app_control(
                &home,
                AppControlCommand::CaptureScreenshot {
                    output_path: args
                        .output
                        .unwrap_or_else(|| default_screenshot_output_path(&home, "manual"))
                        .display()
                        .to_string(),
                },
                args.timeout_ms,
            )?;
            emit_app_response(response)
        }
        AppCommand::Screenrecord(args) => {
            let response = request_app_control(
                &home,
                AppControlCommand::CaptureScreenRecording {
                    output_path: args
                        .output
                        .unwrap_or_else(|| default_recording_output_path(&home, "manual"))
                        .display()
                        .to_string(),
                    duration_secs: args.duration_sec,
                },
                args.timeout_ms,
            )?;
            emit_app_response(response)
        }
        AppCommand::Focus(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::FocusWindow,
            args.timeout_ms,
        )?),
        AppCommand::NewSetup(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::NewSetup {
                name: args.name,
                preset: args.preset,
                profile: args.profile,
                hostname: args.hostname,
            },
            args.timeout_ms,
        )?),
        AppCommand::SelectSetup(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SelectSetup {
                setup_id: args.setup_id,
            },
            args.timeout_ms,
        )?),
        AppCommand::SaveSetup(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SaveSetup,
            args.timeout_ms,
        )?),
        AppCommand::SetStage(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetJourneyStage { stage: args.stage },
            args.timeout_ms,
        )?),
        AppCommand::SetSetupName(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetSetupName { value: args.value },
            args.timeout_ms,
        )?),
        AppCommand::SetHostname(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetHostname { value: args.value },
            args.timeout_ms,
        )?),
        AppCommand::SetArtifactsDir(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetArtifactsDir { value: args.value },
            args.timeout_ms,
        )?),
        AppCommand::SetRepoRoot(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetRepoRoot { value: args.value },
            args.timeout_ms,
        )?),
        AppCommand::SetBuildContext(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetBuildContext {
                artifacts_dir: args.artifacts_dir.display().to_string(),
                repo_root: args.repo_root.display().to_string(),
            },
            args.timeout_ms,
        )?),
        AppCommand::ApplyPreset(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::ApplyPreset {
                preset: args.preset,
            },
            args.timeout_ms,
        )?),
        AppCommand::SetProfile(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetProfile {
                profile: args.profile,
            },
            args.timeout_ms,
        )?),
        AppCommand::ToggleNvidia(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::ToggleNvidia,
            args.timeout_ms,
        )?),
        AppCommand::ToggleLts(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::ToggleLts,
            args.timeout_ms,
        )?),
        AppCommand::SetSidebar(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetSidebarOpen { open: args.open },
            args.timeout_ms,
        )?),
        AppCommand::SetUtilityPane(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetUtilityPaneOpen { open: args.open },
            args.timeout_ms,
        )?),
        AppCommand::SetRightPanel(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetRightPanelMode { mode: args.mode },
            args.timeout_ms,
        )?),
        AppCommand::SetAppearancePanel(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::SetAppearancePanelOpen { open: args.open },
            args.timeout_ms,
        )?),
        AppCommand::StartBuild(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::StartBuild,
            args.timeout_ms,
        )?),
        AppCommand::WaitBuild(args) => {
            let payload =
                wait_for_build_snapshot(&home, args.timeout_ms, args.poll_ms, args.trace_lines)?;
            println!("{}", serde_json::to_string_pretty(&payload)?);
            let failed = payload
                .get("build")
                .and_then(|build| build.get("failed"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let timed_out = payload
                .get("wait")
                .and_then(|wait| wait.get("timed_out"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if (failed || timed_out) && !args.allow_failure {
                std::process::exit(1);
            }
            Ok(())
        }
        AppCommand::TraceTail(args) => {
            let payload = serde_json::json!({
                "path": event_trace_path(&home),
                "lines": read_trace_tail(&event_trace_path(&home), args.lines),
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
            Ok(())
        }
        AppCommand::OpenBuildDetails(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::OpenBuildDetails,
            args.timeout_ms,
        )?),
        AppCommand::RevealPrimaryArtifact(args) => emit_app_response(request_app_control(
            &home,
            AppControlCommand::RevealPrimaryArtifact,
            args.timeout_ms,
        )?),
    }
}

#[cfg(feature = "desktop-ui")]
fn emit_app_response(response: app_control::AppControlResponse) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

#[cfg(feature = "desktop-ui")]
fn wait_for_build_snapshot(
    home: &Path,
    timeout_ms: u64,
    poll_ms: u64,
    trace_lines: usize,
) -> Result<serde_json::Value> {
    use app_control::{AppControlCommand, request_app_control};
    use yggterm_core::{event_trace_path, read_trace_tail};

    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(250));
    let poll_duration = Duration::from_millis(poll_ms.max(50));
    let target_pid = std::env::var("YGGDRASIL_MAKER_APP_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let mut saw_running = false;
    let mut polls = 0_u64;
    let mut last_error: Option<String> = None;
    loop {
        match request_app_control(home, AppControlCommand::DescribeState, 8_000) {
            Ok(response) => {
                polls += 1;
                let handled_by_pid = response.handled_by_pid;
                let state = response.data.unwrap_or_else(|| serde_json::json!({}));
                let build = state
                    .get("build")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let running = build
                    .get("running")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                let status = build
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let success = state.get("success").is_some_and(|value| !value.is_null());
                let failed = status.to_ascii_lowercase().contains("failed");
                if running {
                    saw_running = true;
                }
                if success || failed || (!running && saw_running) {
                    let trace_path = event_trace_path(home);
                    return Ok(serde_json::json!({
                        "wait": {
                            "timed_out": false,
                            "saw_running": saw_running,
                            "polls": polls,
                            "target_pid": target_pid,
                            "handled_by_pid": handled_by_pid,
                            "last_error": last_error,
                        },
                        "build": {
                            "running": running,
                            "status": status,
                            "failed": failed,
                            "succeeded": success,
                        },
                        "state": state,
                        "trace": {
                            "path": trace_path,
                            "lines": read_trace_tail(&trace_path, trace_lines),
                        }
                    }));
                }
            }
            Err(error) => {
                last_error = Some(error.to_string());
            }
        }
        if Instant::now() >= deadline {
            let trace_path = event_trace_path(home);
            return Ok(serde_json::json!({
                "wait": {
                    "timed_out": true,
                    "saw_running": saw_running,
                    "polls": polls,
                    "target_pid": target_pid,
                    "handled_by_pid": serde_json::Value::Null,
                    "last_error": last_error,
                },
                "build": {
                    "running": serde_json::Value::Null,
                    "status": "",
                    "failed": false,
                    "succeeded": false,
                },
                "state": serde_json::Value::Null,
                "trace": {
                    "path": trace_path,
                    "lines": read_trace_tail(&trace_path, trace_lines),
                }
            }));
        }
        std::thread::sleep(poll_duration);
    }
}

fn load_build_inputs(app: &MakerApp, args: BuildInputArgs) -> Result<BuildInputs> {
    Ok(BuildInputs {
        setup_document: app.load_setup_path(&args.setup)?,
        artifacts_dir: args.artifacts_dir,
        authorized_keys_file: args.authorized_keys_file,
        host_keys_dir: args.host_keys_dir,
        repo_root: args.repo_root,
        skip_smoke: args.skip_smoke,
    })
}

fn shell_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| {
            if part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=,".contains(ch))
            {
                part.clone()
            } else {
                format!("'{}'", part.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_mode(mode: BuildMode) -> &'static str {
    match mode {
        BuildMode::LocalDocker => "local-docker",
        BuildMode::ExportOnly => "export-only",
    }
}

fn emit_local_event(event: &BuildEvent, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string(event)?);
        return Ok(());
    }

    match event {
        BuildEvent::StageStarted { stage } => println!("stage started: {:?}", stage),
        BuildEvent::StageFinished { stage } => println!("stage finished: {:?}", stage),
        BuildEvent::ArtifactReady { profile, path } => {
            println!("artifact ready: {} -> {}", profile, path)
        }
        BuildEvent::Failure {
            code,
            message_key,
            detail,
        } => {
            eprintln!("failure: {:?} ({}) {}", code, message_key, detail);
        }
        BuildEvent::LogLine { stream, line } => {
            println!("[{}] {}", stream, line);
        }
    }
    Ok(())
}
