use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use maker_app::{BuildInputs, MakerApp};
use maker_build::{BuildErrorCode, BuildEvent, BuildMode};
use maker_copy::preset_cards;
use maker_model::{BuildProfile, PresetId};
use std::path::PathBuf;

#[cfg(feature = "desktop-ui")]
mod gui;

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
            let as_json = args.json;
            match app.run_build(load_build_inputs(app, args.input)?, |event| {
                let _ = emit_local_event(&event, as_json);
            }) {
                Ok(result) => {
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
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(())
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
