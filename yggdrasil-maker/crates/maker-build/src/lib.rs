use anyhow::{Context, Result, bail};
use maker_model::{BuildProfile, SetupDocument, ValidatedBuildConfig};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const CONTAINER_INPUT_ROOT: &str = "/workspace/input";
const CONTAINER_OUTPUT_ROOT: &str = "/workspace/output";
const CONTAINER_CONFIG_PATH: &str = "/workspace/input/ygg.local.toml";
const CONTAINER_INVOCATION_PATH: &str = "/workspace/input/invocation.json";
const CONTAINER_SSH_AUTHORIZED_KEYS_PATH: &str = "/workspace/input/secrets/authorized_keys";
const CONTAINER_SSH_HOST_KEYS_PATH: &str = "/workspace/input/secrets/ssh-host-keys";
const BUNDLE_CONFIG_NAME: &str = "ygg.local.toml";
const BUNDLE_SETUP_NAME: &str = "setup.runtime.json";
const BUNDLE_SETUP_PERSISTED_NAME: &str = "setup.persisted.json";
const BUNDLE_INVOCATION_NAME: &str = "invocation.json";
pub const ARTIFACT_MANIFEST_NAME: &str = "artifact-manifest.json";
pub const EXPORT_README_NAME: &str = "README.txt";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildMode {
    LocalDocker,
    ExportOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceMode {
    ReleaseContainer,
    RepoLocal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBuildRequest {
    pub app_version: String,
    pub setup_document: SetupDocument,
    pub artifacts_dir: PathBuf,
    pub source_mode: SourceMode,
    pub repo_root: Option<PathBuf>,
    pub skip_smoke: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationPayload {
    pub app_version: String,
    pub setup_name: String,
    pub build_profile: BuildProfile,
    pub skip_smoke: bool,
    pub config_path: String,
    pub artifacts_dir: String,
    pub source_mode: SourceMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildPlan {
    pub mode: BuildMode,
    pub image_ref: String,
    pub repo_root: Option<PathBuf>,
    pub input_bundle_dir: PathBuf,
    pub host_config_path: PathBuf,
    pub host_invocation_path: PathBuf,
    pub host_persisted_setup_path: PathBuf,
    pub host_artifact_manifest_path: PathBuf,
    pub artifacts_dir: PathBuf,
    pub docker_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStage {
    Preflight,
    Bundle,
    DockerRun,
    Build,
    Smoke,
    ArtifactCopy,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildErrorCode {
    DockerMissing,
    ImageMissing,
    ImageVersionMismatch,
    BuildConfigInvalid,
    InputBundleWriteFailed,
    ContainerLaunchFailed,
    BuildProcessFailed,
    EventStreamInvalid,
    ArtifactMissing,
    OutputPermissionDenied,
    UnsupportedPlatform,
    SmokeTestFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BuildEvent {
    StageStarted {
        stage: BuildStage,
    },
    StageFinished {
        stage: BuildStage,
    },
    ArtifactReady {
        profile: BuildProfile,
        path: String,
    },
    Failure {
        code: BuildErrorCode,
        message_key: String,
        detail: String,
    },
    LogLine {
        stream: String,
        line: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Iso,
    NativeConfig,
    SetupDocument,
    HandoffReadme,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub kind: ArtifactKind,
    pub profile: Option<BuildProfile>,
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub app_version: String,
    pub setup_name: String,
    pub build_profile: BuildProfile,
    pub mode: BuildMode,
    pub source_mode: SourceMode,
    pub artifacts: Vec<ArtifactRecord>,
}

pub fn build_plan_for_request(request: &AppBuildRequest) -> Result<BuildPlan> {
    let prepared = PreparedBundle::new(request)?;
    let image_ref = image_ref(&request.app_version);
    let mode = mode_for_current_platform();
    let docker_command = docker_command(request, &prepared, &image_ref, mode)?;

    Ok(BuildPlan {
        mode,
        image_ref,
        repo_root: request.repo_root.clone(),
        input_bundle_dir: prepared.tempdir.keep(),
        host_config_path: prepared.config_path,
        host_invocation_path: prepared.invocation_path,
        host_persisted_setup_path: prepared.persisted_setup_path,
        host_artifact_manifest_path: request.artifacts_dir.join(ARTIFACT_MANIFEST_NAME),
        artifacts_dir: request.artifacts_dir.clone(),
        docker_command,
    })
}

pub fn image_ref(app_version: &str) -> String {
    format!("ghcr.io/yggdrasilhq/yggdrasil-maker-build:v{app_version}")
}

pub fn mode_for_current_platform() -> BuildMode {
    if std::env::consts::OS == "linux" {
        BuildMode::LocalDocker
    } else {
        BuildMode::ExportOnly
    }
}

fn docker_command(
    request: &AppBuildRequest,
    prepared: &PreparedBundle,
    image_ref: &str,
    mode: BuildMode,
) -> Result<Vec<String>> {
    if mode == BuildMode::ExportOnly {
        return Ok(Vec::new());
    }

    let mut args = vec![
        "docker".to_owned(),
        "run".to_owned(),
        "--rm".to_owned(),
        "--pull".to_owned(),
        "never".to_owned(),
        "--mount".to_owned(),
        format!(
            "type=bind,src={},dst={},readonly",
            prepared.tempdir.path().display(),
            CONTAINER_INPUT_ROOT
        ),
        "--mount".to_owned(),
        format!(
            "type=bind,src={},dst={}",
            request.artifacts_dir.display(),
            CONTAINER_OUTPUT_ROOT
        ),
    ];

    if request.source_mode == SourceMode::RepoLocal {
        let repo_root = request
            .repo_root
            .as_ref()
            .context("repo-local mode requires repo_root")?;
        args.push("--mount".to_owned());
        args.push(format!(
            "type=bind,src={},dst=/workspace/repo",
            repo_root.display()
        ));
    }

    args.push(image_ref.to_owned());
    args.push("--config".to_owned());
    args.push(CONTAINER_CONFIG_PATH.to_owned());
    args.push("--invoke".to_owned());
    args.push(CONTAINER_INVOCATION_PATH.to_owned());
    Ok(args)
}

struct PreparedBundle {
    tempdir: TempDir,
    config_path: PathBuf,
    invocation_path: PathBuf,
    persisted_setup_path: PathBuf,
}

impl PreparedBundle {
    fn new(request: &AppBuildRequest) -> Result<Self> {
        fs::create_dir_all(&request.artifacts_dir).with_context(|| {
            format!(
                "failed to create artifacts dir {}",
                request.artifacts_dir.display()
            )
        })?;

        let validated = request
            .setup_document
            .validate()
            .map_err(|error| anyhow::anyhow!(error))?;
        let tempdir = tempfile::Builder::new()
            .prefix("yggdrasil-maker-")
            .tempdir()
            .context("failed to create build input bundle")?;
        let secrets_dir = tempdir.path().join("secrets");
        fs::create_dir_all(&secrets_dir)
            .context("failed to create secrets directory in build bundle")?;

        let container_config =
            prepare_sensitive_mounts(&validated, &request.setup_document, &secrets_dir)?;
        let config_path = tempdir.path().join(BUNDLE_CONFIG_NAME);
        fs::write(&config_path, container_config.to_native_toml()?.as_bytes())
            .context("failed to write native config bundle")?;

        let invocation = InvocationPayload {
            app_version: request.app_version.clone(),
            setup_name: request.setup_document.setup.name.clone(),
            build_profile: container_config.build_profile,
            skip_smoke: request.skip_smoke,
            config_path: CONTAINER_CONFIG_PATH.to_owned(),
            artifacts_dir: CONTAINER_OUTPUT_ROOT.to_owned(),
            source_mode: request.source_mode,
        };
        let invocation_path = tempdir.path().join(BUNDLE_INVOCATION_NAME);
        fs::write(
            &invocation_path,
            serde_json::to_vec_pretty(&invocation)
                .context("failed to encode invocation payload")?,
        )
        .context("failed to write invocation payload")?;

        let runtime_setup_path = tempdir.path().join(BUNDLE_SETUP_NAME);
        fs::write(
            &runtime_setup_path,
            serde_json::to_vec_pretty(&request.setup_document)
                .context("failed to encode runtime setup payload")?,
        )
        .context("failed to write runtime setup payload")?;

        let persisted_setup_path = tempdir.path().join(BUNDLE_SETUP_PERSISTED_NAME);
        fs::write(
            &persisted_setup_path,
            serde_json::to_vec_pretty(&request.setup_document.sanitized_for_persistence())
                .context("failed to encode persisted setup payload")?,
        )
        .context("failed to write persisted setup payload")?;

        Ok(Self {
            tempdir,
            config_path,
            invocation_path,
            persisted_setup_path,
        })
    }
}

pub fn parse_build_event_line(line: &str) -> Result<BuildEvent> {
    serde_json::from_str(line).with_context(|| format!("invalid build event line: {line}"))
}

pub fn parse_build_event_stream<R, F>(reader: R, mut on_event: F) -> Result<()>
where
    R: BufRead,
    F: FnMut(BuildEvent),
{
    for line in reader.lines() {
        let line = line.context("failed reading build event stream")?;
        if line.trim().is_empty() {
            continue;
        }
        on_event(parse_build_event_line(&line)?);
    }
    Ok(())
}

pub fn read_artifact_manifest(path: &Path) -> Result<ArtifactManifest> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read artifact manifest {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("invalid artifact manifest {}", path.display()))
}

fn prepare_sensitive_mounts(
    validated: &ValidatedBuildConfig,
    setup_document: &SetupDocument,
    secrets_dir: &Path,
) -> Result<ValidatedBuildConfig> {
    let mut config = validated.clone();

    if validated.embed_ssh_keys {
        if let Some(host_path) = setup_document.setup.ssh.authorized_keys_file.build_value() {
            if !host_path.trim().is_empty() {
                let source = Path::new(host_path);
                if !source.is_file() {
                    bail!("authorized_keys file not found: {}", source.display());
                }
                let target = secrets_dir.join("authorized_keys");
                fs::copy(source, &target).with_context(|| {
                    format!(
                        "failed to copy authorized_keys into bundle from {}",
                        source.display()
                    )
                })?;
                config.ssh_authorized_keys_file = CONTAINER_SSH_AUTHORIZED_KEYS_PATH.to_owned();
            }
        }

        if let Some(host_dir) = setup_document.setup.ssh.host_keys_dir.build_value() {
            if !host_dir.trim().is_empty() {
                let source_dir = Path::new(host_dir);
                if !source_dir.is_dir() {
                    bail!(
                        "ssh host keys directory not found: {}",
                        source_dir.display()
                    );
                }
                let target_dir = secrets_dir.join("ssh-host-keys");
                copy_dir_recursive(source_dir, &target_dir)?;
                config.ssh_host_keys_dir = CONTAINER_SSH_HOST_KEYS_PATH.to_owned();
            }
        }
    }

    Ok(config)
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target).with_context(|| format!("failed to create {}", target.display()))?;
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&path, &target_path)?;
        } else if file_type.is_file() {
            fs::copy(&path, &target_path).with_context(|| {
                format!(
                    "failed to copy {} into {}",
                    path.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maker_model::{PresetId, SensitiveField};

    #[test]
    fn linux_uses_local_docker_mode() {
        if std::env::consts::OS == "linux" {
            assert_eq!(mode_for_current_platform(), BuildMode::LocalDocker);
        }
    }

    #[test]
    fn image_ref_is_exactly_versioned() {
        assert_eq!(
            image_ref("0.1.0"),
            "ghcr.io/yggdrasilhq/yggdrasil-maker-build:v0.1.0"
        );
    }

    #[test]
    fn build_plan_rewrites_sensitive_paths_into_bundle() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let ssh_path = tempdir.path().join("authorized_keys");
        fs::write(&ssh_path, "ssh-ed25519 AAAA test\n").expect("write authorized_keys");
        let artifacts_dir = tempdir.path().join("artifacts");

        let mut setup_document = SetupDocument::new("NAS".to_owned(), PresetId::Nas);
        setup_document.setup.ssh.authorized_keys_file =
            SensitiveField::ephemeral(ssh_path.display().to_string());

        let request = AppBuildRequest {
            app_version: "0.1.0".to_owned(),
            setup_document,
            artifacts_dir,
            source_mode: SourceMode::ReleaseContainer,
            repo_root: None,
            skip_smoke: false,
        };

        let plan = build_plan_for_request(&request).expect("build plan");
        let emitted = fs::read_to_string(&plan.host_config_path).expect("read config");
        assert!(emitted.contains(CONTAINER_SSH_AUTHORIZED_KEYS_PATH));
    }

    #[test]
    fn repo_local_mode_mounts_repo_writable() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let ssh_path = tempdir.path().join("authorized_keys");
        fs::write(&ssh_path, "ssh-ed25519 AAAA test\n").expect("write authorized_keys");
        let repo_root = tempdir.path().join("repo");
        fs::create_dir_all(&repo_root).expect("repo dir");
        let artifacts_dir = tempdir.path().join("artifacts");

        let mut setup_document = SetupDocument::new("NAS".to_owned(), PresetId::Nas);
        setup_document.setup.ssh.authorized_keys_file =
            SensitiveField::ephemeral(ssh_path.display().to_string());

        let request = AppBuildRequest {
            app_version: "0.1.0".to_owned(),
            setup_document,
            artifacts_dir,
            source_mode: SourceMode::RepoLocal,
            repo_root: Some(repo_root.clone()),
            skip_smoke: false,
        };

        let plan = build_plan_for_request(&request).expect("build plan");
        let repo_mount = plan
            .docker_command
            .iter()
            .find(|value| value.contains("/workspace/repo"))
            .expect("repo mount present");
        assert!(!repo_mount.contains("readonly"));
        assert!(repo_mount.contains(&repo_root.display().to_string()));
    }

    #[test]
    fn parse_build_event_line_decodes_json() {
        let event = parse_build_event_line(r#"{"type":"stage-started","stage":"preflight"}"#)
            .expect("parse build event");
        assert_eq!(
            event,
            BuildEvent::StageStarted {
                stage: BuildStage::Preflight
            }
        );
    }

    #[test]
    fn read_artifact_manifest_decodes_records() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join(ARTIFACT_MANIFEST_NAME);
        fs::write(
            &path,
            r#"{
  "app_version": "0.1.0",
  "setup_name": "Lab NAS",
  "build_profile": "server",
  "mode": "export-only",
  "source_mode": "release-container",
  "artifacts": [
    {
      "kind": "native-config",
      "profile": null,
      "path": "/tmp/ygg.local.toml",
      "sha256": "abc123",
      "size_bytes": 42
    }
  ]
}"#,
        )
        .expect("write manifest");

        let manifest = read_artifact_manifest(&path).expect("read manifest");
        assert_eq!(manifest.mode, BuildMode::ExportOnly);
        assert_eq!(manifest.artifacts[0].kind, ArtifactKind::NativeConfig);
        assert_eq!(manifest.artifacts[0].size_bytes, 42);
    }
}
