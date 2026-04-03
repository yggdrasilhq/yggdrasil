use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use maker_build::{AppBuildRequest, BuildMode, SourceMode, build_plan_for_request};
use maker_copy::preset_cards;
use maker_model::{BuildProfile, PresetId, SetupDocument};
use std::fs;
use std::path::{Path, PathBuf};

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
}

#[derive(Args, Debug)]
struct PlanBuildArgs {
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
            let mut document = read_setup(&args.setup)?;
            apply_runtime_sensitive_overrides(&mut document, &args);
            require_runtime_sensitive_inputs(&document)?;
            let request = AppBuildRequest {
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
                setup_document: document,
                artifacts_dir: args.artifacts_dir,
                source_mode: if args.repo_root.is_some() {
                    SourceMode::RepoLocal
                } else {
                    SourceMode::ReleaseContainer
                },
                repo_root: args.repo_root,
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
    }
    Ok(())
}

fn apply_runtime_sensitive_overrides(document: &mut SetupDocument, args: &PlanBuildArgs) {
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
