use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use maker_build::{AppBuildRequest, BuildMode, SourceMode, build_plan_for_request};
use maker_build::{BuildErrorCode, BuildEvent};
use maker_copy::preset_cards;
use maker_model::{BuildProfile, PresetId, SetupDocument};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};

#[derive(Parser, Debug)]
#[command(name = "yggdrasil-maker")]
#[command(about = "Foundation CLI for the yggdrasil-maker build flow")]
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

#[derive(Args, Debug)]
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Preset(args) => run_preset(args)?,
        Command::Setup { command } => run_setup(command)?,
        Command::Config { command } => run_config(command)?,
        Command::Build { command } => run_build(command)?,
    }
    Ok(())
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

fn run_setup(command: SetupCommand) -> Result<()> {
    match command {
        SetupCommand::New(args) => {
            let mut document = SetupDocument::new(args.name, args.preset);
            document.setup.profile_override = args.profile;
            if let Some(hostname) = args.hostname {
                document.setup.personalization.hostname = hostname;
            }

            let output_path = args
                .output
                .unwrap_or_else(|| PathBuf::from(format!("{}.maker.json", document.setup.slug())));
            write_json(&output_path, &document)?;
            println!("{}", output_path.display());
        }
        SetupCommand::Show(args) => {
            let document = read_setup(&args.input)?;
            println!("{}", serde_json::to_string_pretty(&document)?);
        }
    }
    Ok(())
}

fn run_config(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Emit(args) => {
            let document = read_setup(&args.setup)?;
            let config = document.validate()?;
            let toml = config.to_native_toml()?;
            if let Some(output) = args.output {
                fs::write(&output, toml.as_bytes())
                    .with_context(|| format!("failed to write {}", output.display()))?;
                println!("{}", output.display());
            } else {
                print!("{toml}");
            }
        }
    }
    Ok(())
}

fn run_build(command: BuildCommand) -> Result<()> {
    match command {
        BuildCommand::Plan(args) => {
            let mut document = read_setup(&args.input.setup)?;
            apply_runtime_sensitive_overrides(&mut document, &args.input);
            require_runtime_sensitive_inputs(&document)?;
            let request = AppBuildRequest {
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
                setup_document: document,
                artifacts_dir: args.input.artifacts_dir,
                source_mode: if args.input.repo_root.is_some() {
                    SourceMode::RepoLocal
                } else {
                    SourceMode::ReleaseContainer
                },
                repo_root: args.input.repo_root,
                skip_smoke: false,
            };
            let plan = build_plan_for_request(&request)?;
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
                println!("invocation: {}", plan.host_invocation_path.display());
                println!("artifacts: {}", plan.artifacts_dir.display());
                println!("docker command:");
                println!("  {}", shell_join(&plan.docker_command));
            }
        }
        BuildCommand::Run(args) => {
            let mut document = match read_setup(&args.input.setup) {
                Ok(document) => document,
                Err(error) => {
                    emit_local_event(
                        &BuildEvent::Failure {
                            code: BuildErrorCode::BuildConfigInvalid,
                            message_key: "setup_read_failed".to_owned(),
                            detail: error.to_string(),
                        },
                        args.json,
                    )?;
                    exit_with_failure();
                }
            };
            apply_runtime_sensitive_overrides(&mut document, &args.input);
            if let Err(error) = require_runtime_sensitive_inputs(&document) {
                emit_local_event(
                    &BuildEvent::Failure {
                        code: BuildErrorCode::BuildConfigInvalid,
                        message_key: "sensitive_input_missing".to_owned(),
                        detail: error.to_string(),
                    },
                    args.json,
                )?;
                exit_with_failure();
            }
            let request = AppBuildRequest {
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
                setup_document: document,
                artifacts_dir: args.input.artifacts_dir,
                source_mode: if args.input.repo_root.is_some() {
                    SourceMode::RepoLocal
                } else {
                    SourceMode::ReleaseContainer
                },
                repo_root: args.input.repo_root,
                skip_smoke: false,
            };
            let plan = match build_plan_for_request(&request) {
                Ok(plan) => plan,
                Err(error) => {
                    emit_local_event(
                        &BuildEvent::Failure {
                            code: BuildErrorCode::InputBundleWriteFailed,
                            message_key: "build_plan_failed".to_owned(),
                            detail: error.to_string(),
                        },
                        args.json,
                    )?;
                    exit_with_failure();
                }
            };
            run_build_plan(plan, args.json)?;
        }
    }
    Ok(())
}

fn apply_runtime_sensitive_overrides(document: &mut SetupDocument, args: &BuildInputArgs) {
    if let Some(path) = args.authorized_keys_file.as_ref() {
        document.setup.ssh.authorized_keys_file.value = Some(path.display().to_string());
    }
    if let Some(path) = args.host_keys_dir.as_ref() {
        document.setup.ssh.host_keys_dir.value = Some(path.display().to_string());
    }
}

fn require_runtime_sensitive_inputs(document: &SetupDocument) -> Result<()> {
    if !document.setup.ssh.embed_ssh_keys {
        return Ok(());
    }

    let has_authorized_keys = document
        .setup
        .ssh
        .authorized_keys_file
        .build_value()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    if !has_authorized_keys {
        bail!(
            "build planning requires --authorized-keys-file or a remembered ssh authorized_keys path"
        );
    }

    Ok(())
}

fn read_setup(path: &Path) -> Result<SetupDocument> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let document = serde_json::from_str(&raw)
        .with_context(|| format!("invalid setup JSON in {}", path.display()))?;
    Ok(document)
}

fn write_json(path: &Path, document: &SetupDocument) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    let sanitized = document.sanitized_for_persistence();
    let payload = serde_json::to_string_pretty(&sanitized)?;
    fs::write(path, payload.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
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

fn run_build_plan(plan: maker_build::BuildPlan, as_json: bool) -> Result<()> {
    match plan.mode {
        BuildMode::ExportOnly => {
            emit_local_event(
                &BuildEvent::Failure {
                    code: BuildErrorCode::UnsupportedPlatform,
                    message_key: "export_only_mode".to_owned(),
                    detail: format!(
                        "local builds are not supported on this platform; inspect the bundle at {}",
                        plan.input_bundle_dir.display()
                    ),
                },
                as_json,
            )?;
            exit_with_failure();
        }
        BuildMode::LocalDocker => {}
    }

    emit_local_event(
        &BuildEvent::StageStarted {
            stage: maker_build::BuildStage::Preflight,
        },
        as_json,
    )?;

    let docker = plan
        .docker_command
        .first()
        .cloned()
        .unwrap_or_else(|| "docker".to_owned());
    let docker_check = ProcessCommand::new(&docker)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match docker_check {
        Ok(status) if status.success() => {}
        _ => {
            emit_local_event(
                &BuildEvent::Failure {
                    code: BuildErrorCode::DockerMissing,
                    message_key: "docker_missing".to_owned(),
                    detail: "docker is required for local Linux builds".to_owned(),
                },
                as_json,
            )?;
            exit_with_failure();
        }
    }

    emit_local_event(
        &BuildEvent::StageFinished {
            stage: maker_build::BuildStage::Preflight,
        },
        as_json,
    )?;

    let mut command = ProcessCommand::new(&docker);
    command.args(plan.docker_command.iter().skip(1));
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    let status = command.status().context("failed to launch docker")?;
    if !status.success() {
        emit_local_event(
            &BuildEvent::Failure {
                code: BuildErrorCode::ContainerLaunchFailed,
                message_key: "container_launch_failed".to_owned(),
                detail: format!("docker exited with status {}", status),
            },
            as_json,
        )?;
        exit_with_failure();
    }

    Ok(())
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

fn exit_with_failure() -> ! {
    std::process::exit(1)
}
