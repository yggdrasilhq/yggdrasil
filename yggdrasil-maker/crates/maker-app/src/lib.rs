use anyhow::{Context, Result, anyhow, bail};
use maker_build::{
    AppBuildRequest, ArtifactKind, ArtifactManifest, ArtifactRecord, BuildErrorCode, BuildEvent,
    BuildMode, BuildPlan, BuildStage, EXPORT_README_NAME, SourceMode, build_plan_for_request,
    parse_build_event_stream, read_artifact_manifest,
};
use maker_model::{BuildProfile, JourneyStage, PresetId, SetupDocument};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::mpsc;
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInputs {
    pub setup_document: SetupDocument,
    pub artifacts_dir: PathBuf,
    pub authorized_keys_file: Option<PathBuf>,
    pub host_keys_dir: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub skip_smoke: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRunResult {
    pub plan: BuildPlan,
    pub manifest: ArtifactManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSetupSummary {
    pub setup_id: String,
    pub name: String,
    pub slug: String,
    pub journey_stage: JourneyStage,
    pub path: PathBuf,
    pub modified_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupStore {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakerApp {
    setup_store: SetupStore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportBundleManifest {
    pub bundle_dir: PathBuf,
    pub manifest: ArtifactManifest,
}

impl MakerApp {
    pub fn new_for_current_platform() -> Result<Self> {
        Ok(Self {
            setup_store: SetupStore::new(default_setup_store_root()?)?,
        })
    }

    pub fn from_setup_root(root: PathBuf) -> Result<Self> {
        Ok(Self {
            setup_store: SetupStore::new(root)?,
        })
    }

    pub fn setup_store(&self) -> &SetupStore {
        &self.setup_store
    }

    pub fn create_setup_document(
        &self,
        name: String,
        preset: PresetId,
        profile: Option<BuildProfile>,
        hostname: Option<String>,
    ) -> SetupDocument {
        let mut document = SetupDocument::new(name, preset);
        document.setup.profile_override = profile;
        if let Some(hostname) = hostname {
            document.setup.personalization.hostname = hostname;
        }
        document
    }

    pub fn load_setup_path(&self, path: &Path) -> Result<SetupDocument> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let document = serde_json::from_str::<SetupDocument>(&raw)
            .with_context(|| format!("invalid setup JSON in {}", path.display()))?;
        document
            .migrate_to_current()
            .map_err(|error| anyhow!(error))
            .with_context(|| format!("failed to migrate {}", path.display()))
    }

    pub fn save_setup_path(&self, path: &Path, document: &SetupDocument) -> Result<()> {
        write_setup_json(path, document)
    }

    pub fn emit_config_toml(&self, document: &SetupDocument) -> Result<String> {
        let migrated = document
            .clone()
            .migrate_to_current()
            .map_err(|error| anyhow!(error))?;
        let config = migrated.validate().map_err(|error| anyhow!(error))?;
        config.to_native_toml().map_err(|error| anyhow!(error))
    }

    pub fn plan_build(&self, mut inputs: BuildInputs) -> Result<BuildPlan> {
        let authorized_keys_file = inputs.authorized_keys_file.clone();
        let host_keys_dir = inputs.host_keys_dir.clone();
        apply_runtime_sensitive_overrides(
            &mut inputs.setup_document,
            authorized_keys_file.as_deref(),
            host_keys_dir.as_deref(),
        );
        require_runtime_sensitive_inputs(&inputs.setup_document)?;
        let source_mode = source_mode_for_inputs(&inputs);
        build_plan_for_request(&AppBuildRequest {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            setup_document: inputs.setup_document,
            artifacts_dir: inputs.artifacts_dir,
            source_mode,
            repo_root: inputs.repo_root,
            skip_smoke: inputs.skip_smoke,
        })
    }

    pub fn run_build<F>(&self, inputs: BuildInputs, mut on_event: F) -> Result<BuildRunResult>
    where
        F: FnMut(BuildEvent),
    {
        let plan = self.plan_build(inputs.clone())?;
        match plan.mode {
            BuildMode::ExportOnly => {
                on_event(BuildEvent::StageStarted {
                    stage: BuildStage::Bundle,
                });
                let manifest = create_export_bundle(&plan, &inputs.setup_document)?;
                for artifact in &manifest.artifacts {
                    on_event(BuildEvent::ArtifactReady {
                        profile: manifest.build_profile,
                        path: artifact.path.clone(),
                    });
                }
                on_event(BuildEvent::StageFinished {
                    stage: BuildStage::Bundle,
                });
                on_event(BuildEvent::StageStarted {
                    stage: BuildStage::Complete,
                });
                on_event(BuildEvent::StageFinished {
                    stage: BuildStage::Complete,
                });
                Ok(BuildRunResult { plan, manifest })
            }
            BuildMode::LocalDocker => self.run_local_docker_build(plan, on_event),
        }
    }

    fn run_local_docker_build<F>(&self, plan: BuildPlan, mut on_event: F) -> Result<BuildRunResult>
    where
        F: FnMut(BuildEvent),
    {
        on_event(BuildEvent::StageStarted {
            stage: BuildStage::Preflight,
        });

        if run_command_status("docker", [OsString::from("--version")]).is_err() {
            let event = BuildEvent::Failure {
                code: BuildErrorCode::DockerMissing,
                message_key: "docker_missing".to_owned(),
                detail: "docker is required for local Linux builds".to_owned(),
            };
            on_event(event.clone());
            bail!(BuildFailure { event });
        }

        if run_command_status(
            "docker",
            [
                OsString::from("image"),
                OsString::from("inspect"),
                OsString::from(plan.image_ref.clone()),
            ],
        )
        .is_err()
        {
            let event = BuildEvent::Failure {
                code: BuildErrorCode::ImageMissing,
                message_key: "image_missing".to_owned(),
                detail: format!("missing version-matched builder image {}", plan.image_ref),
            };
            on_event(event.clone());
            bail!(BuildFailure { event });
        }

        on_event(BuildEvent::StageFinished {
            stage: BuildStage::Preflight,
        });
        on_event(BuildEvent::StageStarted {
            stage: BuildStage::DockerRun,
        });

        let mut command = ProcessCommand::new("docker");
        command.args(plan.docker_command.iter().skip(1));
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = command
            .statusless_spawn()
            .context("failed to launch docker")?;

        let stdout = child.stdout.take().context("missing docker stdout pipe")?;
        let stderr = child.stderr.take().context("missing docker stderr pipe")?;
        let (stderr_tx, stderr_rx) = mpsc::channel::<BuildEvent>();

        let stderr_thread = thread::spawn(move || -> Result<()> {
            let reader = BufReader::new(stderr);
            for line in std::io::BufRead::lines(reader) {
                let line = line.context("failed reading docker stderr")?;
                if !line.trim().is_empty() {
                    let _ = stderr_tx.send(BuildEvent::LogLine {
                        stream: "stderr".to_owned(),
                        line,
                    });
                }
            }
            Ok(())
        });

        let parse_result = {
            let reader = BufReader::new(stdout);
            parse_build_event_stream(reader, |event| on_event(event))
        };

        while let Ok(event) = stderr_rx.try_recv() {
            on_event(event);
        }

        if let Err(error) = parse_result {
            let _ = child.kill();
            let _ = child.wait();
            on_event(BuildEvent::Failure {
                code: BuildErrorCode::EventStreamInvalid,
                message_key: "event_stream_invalid".to_owned(),
                detail: error.to_string(),
            });
            let _ = stderr_thread.join();
            bail!("invalid build event stream");
        }

        let status = child.wait().context("failed waiting for docker")?;
        while let Ok(event) = stderr_rx.try_recv() {
            on_event(event);
        }
        stderr_thread
            .join()
            .map_err(|_| anyhow!("failed joining docker stderr thread"))??;

        on_event(BuildEvent::StageFinished {
            stage: BuildStage::DockerRun,
        });

        if !status.success() {
            let event = BuildEvent::Failure {
                code: BuildErrorCode::ContainerLaunchFailed,
                message_key: "container_launch_failed".to_owned(),
                detail: format!("docker exited with status {status}"),
            };
            on_event(event.clone());
            bail!(BuildFailure { event });
        }

        let manifest = match read_artifact_manifest(&plan.host_artifact_manifest_path) {
            Ok(manifest) => manifest,
            Err(error) => {
                let event = BuildEvent::Failure {
                    code: BuildErrorCode::ArtifactMissing,
                    message_key: "artifact_manifest_missing".to_owned(),
                    detail: error.to_string(),
                };
                on_event(event.clone());
                bail!(BuildFailure { event });
            }
        };
        if let Err(error) = verify_manifest_paths(&manifest) {
            let event = BuildEvent::Failure {
                code: BuildErrorCode::ArtifactMissing,
                message_key: "artifact_missing".to_owned(),
                detail: error.to_string(),
            };
            on_event(event.clone());
            bail!(BuildFailure { event });
        }
        Ok(BuildRunResult { plan, manifest })
    }
}

impl SetupStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create setup store {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save(&self, document: &SetupDocument) -> Result<PathBuf> {
        let path = self.path_for(document);
        write_setup_json(&path, document)?;
        Ok(path)
    }

    pub fn load(&self, setup_id: &str) -> Result<SetupDocument> {
        let path = self.path_for_id(setup_id)?;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let document = serde_json::from_str::<SetupDocument>(&raw)
            .with_context(|| format!("invalid setup JSON in {}", path.display()))?;
        document
            .migrate_to_current()
            .map_err(|error| anyhow!(error))
            .with_context(|| format!("failed to migrate {}", path.display()))
    }

    pub fn delete(&self, setup_id: &str) -> Result<()> {
        let path = self.path_for_id(setup_id)?;
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))
    }

    pub fn list(&self) -> Result<Vec<StoredSetupSummary>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read {}", self.root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let document = serde_json::from_str::<SetupDocument>(&raw)
                .with_context(|| format!("invalid setup JSON in {}", path.display()))?
                .migrate_to_current()
                .map_err(|error| anyhow!(error))?;
            let metadata = entry.metadata()?;
            let modified_unix_secs = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|value| value.as_secs())
                .unwrap_or_default();
            entries.push(StoredSetupSummary {
                setup_id: document.setup_id.clone(),
                name: document.setup.name.clone(),
                slug: document.setup.slug(),
                journey_stage: document.journey_stage,
                path,
                modified_unix_secs,
            });
        }
        entries.sort_by(|left, right| right.modified_unix_secs.cmp(&left.modified_unix_secs));
        Ok(entries)
    }

    fn path_for(&self, document: &SetupDocument) -> PathBuf {
        self.root.join(document.storage_filename())
    }

    fn path_for_id(&self, setup_id: &str) -> Result<PathBuf> {
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read {}", self.root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if file_name.contains(setup_id) {
                return Ok(path);
            }
        }
        bail!("setup not found: {setup_id}")
    }
}

fn source_mode_for_inputs(inputs: &BuildInputs) -> SourceMode {
    if inputs.repo_root.is_some() {
        SourceMode::RepoLocal
    } else {
        SourceMode::ReleaseContainer
    }
}

fn apply_runtime_sensitive_overrides(
    document: &mut SetupDocument,
    authorized_keys_file: Option<&Path>,
    host_keys_dir: Option<&Path>,
) {
    if let Some(path) = authorized_keys_file {
        document.setup.ssh.authorized_keys_file.value = Some(path.display().to_string());
    }
    if let Some(path) = host_keys_dir {
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

fn write_setup_json(path: &Path, document: &SetupDocument) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let payload = serde_json::to_vec_pretty(&document.sanitized_for_persistence())?;
    let temp_path = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp_path)
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    file.write_all(&payload)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn default_setup_store_root() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("YGGDRASIL_MAKER_SETUP_ROOT") {
        return Ok(PathBuf::from(path));
    }

    match std::env::consts::OS {
        "linux" => {
            let base = std::env::var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|_| {
                    std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/share"))
                })
                .context("unable to resolve Linux data directory")?;
            Ok(base.join("yggdrasil-maker").join("setups"))
        }
        "macos" => {
            let home = std::env::var("HOME").context("unable to resolve HOME")?;
            Ok(PathBuf::from(home)
                .join("Library/Application Support")
                .join("yggdrasil-maker")
                .join("setups"))
        }
        "windows" => {
            let appdata = std::env::var("APPDATA").context("unable to resolve APPDATA")?;
            Ok(PathBuf::from(appdata)
                .join("yggdrasil-maker")
                .join("setups"))
        }
        other => bail!("unsupported platform for setup store: {other}"),
    }
}

fn create_export_bundle(plan: &BuildPlan, document: &SetupDocument) -> Result<ArtifactManifest> {
    let bundle_dir = plan
        .artifacts_dir
        .join(format!("{}-export", document.setup.slug()));
    fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("failed to create {}", bundle_dir.display()))?;

    let config_target = bundle_dir.join("ygg.local.toml");
    fs::copy(&plan.host_config_path, &config_target)
        .with_context(|| format!("failed to copy {}", config_target.display()))?;

    let setup_target = bundle_dir.join("setup.persisted.json");
    fs::copy(&plan.host_persisted_setup_path, &setup_target)
        .with_context(|| format!("failed to copy {}", setup_target.display()))?;

    let readme_target = bundle_dir.join(EXPORT_README_NAME);
    fs::write(
        &readme_target,
        format!(
            "yggdrasil-maker export bundle\n\nThis machine cannot perform a local ISO build.\nMove this folder to a Linux builder with Docker and run:\n\nyggdrasil-maker build run --setup {} --artifacts-dir {}\n",
            setup_target.display(),
            plan.artifacts_dir.display()
        ),
    )
    .with_context(|| format!("failed to write {}", readme_target.display()))?;

    let build_profile = document
        .setup
        .profile_override
        .unwrap_or_else(|| document.setup.preset.recommended_profile());
    let manifest = ArtifactManifest {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        setup_name: document.setup.name.clone(),
        build_profile,
        mode: BuildMode::ExportOnly,
        source_mode: SourceMode::ReleaseContainer,
        artifacts: vec![
            artifact_record(ArtifactKind::NativeConfig, None, &config_target)?,
            artifact_record(ArtifactKind::SetupDocument, None, &setup_target)?,
            artifact_record(ArtifactKind::HandoffReadme, None, &readme_target)?,
        ],
    };
    fs::write(
        &plan.host_artifact_manifest_path,
        serde_json::to_vec_pretty(&manifest).context("failed to encode export manifest")?,
    )
    .with_context(|| {
        format!(
            "failed to write export manifest {}",
            plan.host_artifact_manifest_path.display()
        )
    })?;
    Ok(manifest)
}

fn artifact_record(
    kind: ArtifactKind,
    profile: Option<BuildProfile>,
    path: &Path,
) -> Result<ArtifactRecord> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(ArtifactRecord {
        kind,
        profile,
        path: path.display().to_string(),
        sha256: hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        size_bytes: bytes.len() as u64,
    })
}

fn verify_manifest_paths(manifest: &ArtifactManifest) -> Result<()> {
    if manifest.artifacts.is_empty() {
        bail!("artifact manifest is empty");
    }
    for artifact in &manifest.artifacts {
        let path = Path::new(&artifact.path);
        if !path.exists() {
            bail!(
                "artifact missing after reported success: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn run_command_status<I>(program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = OsString>,
{
    let status = ProcessCommand::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to run {program}"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("{program} exited with status {status}")
    }
}

trait StatuslessSpawn {
    fn statusless_spawn(&mut self) -> std::io::Result<std::process::Child>;
}

impl StatuslessSpawn for ProcessCommand {
    fn statusless_spawn(&mut self) -> std::io::Result<std::process::Child> {
        self.spawn()
    }
}

#[derive(Debug)]
struct BuildFailure {
    event: BuildEvent,
}

impl std::fmt::Display for BuildFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.event)
    }
}

impl std::error::Error for BuildFailure {}

#[cfg(test)]
mod tests {
    use super::*;
    use maker_model::{PresetId, SensitiveField};
    use tempfile::tempdir;

    #[test]
    fn setup_store_round_trips_and_strips_ephemeral_values() {
        let tempdir = tempdir().expect("tempdir");
        let app = MakerApp::from_setup_root(tempdir.path().join("setups")).expect("app");
        let mut document =
            app.create_setup_document("Lab NAS".to_owned(), PresetId::Nas, None, None);
        document.setup.ssh.authorized_keys_file = SensitiveField::ephemeral("secret".to_owned());

        let path = app.setup_store().save(&document).expect("save setup");
        let raw = fs::read_to_string(&path).expect("read stored setup");
        assert!(!raw.contains("secret"));

        let loaded = app
            .setup_store()
            .load(&document.setup_id)
            .expect("load setup");
        assert_eq!(loaded.setup.name, "Lab NAS");
    }

    #[test]
    fn export_bundle_contains_expected_files() {
        let tempdir = tempdir().expect("tempdir");
        let app = MakerApp::from_setup_root(tempdir.path().join("setups")).expect("app");
        let mut document =
            app.create_setup_document("Lab NAS".to_owned(), PresetId::Nas, None, None);
        let ssh_path = tempdir.path().join("authorized_keys");
        fs::write(&ssh_path, "ssh-ed25519 AAAA test\n").expect("write authorized_keys");
        document.setup.ssh.authorized_keys_file =
            SensitiveField::ephemeral(ssh_path.display().to_string());
        let mut plan = app
            .plan_build(BuildInputs {
                setup_document: document.clone(),
                artifacts_dir: tempdir.path().join("artifacts"),
                authorized_keys_file: Some(ssh_path),
                host_keys_dir: None,
                repo_root: None,
                skip_smoke: false,
            })
            .expect("build plan");
        plan.mode = BuildMode::ExportOnly;
        let manifest = create_export_bundle(&plan, &document).expect("export bundle build");

        assert!(plan.host_artifact_manifest_path.is_file());
        assert!(manifest.artifacts.len() >= 3);
    }
}
