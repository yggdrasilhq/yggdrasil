//! The plan flow: exactly what a build would run, without running it.
//!
//! The value of this flow is that it is not a description. The environment is
//! produced by executing the repository's own `scripts/toml-to-env.sh` against
//! the chosen config, and the command lines are the ones `mkconfig.sh` composes
//! — including its quirks, such as the smoke stage receiving the *unexpanded*
//! profile. If the shell and this view ever disagree, this view is the bug.

use std::path::Path;

use serde::Serialize;

use crate::discover;

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    /// Ordinal shown to the user, 1-based.
    pub index: usize,
    pub title: String,
    /// The argv, already quoted for display. Empty for a stage that is not a
    /// single command (the live-build interior).
    pub command: Vec<String>,
    /// Why this step exists, in the plan's own words.
    pub note: String,
    /// What running this step really costs, so nobody is surprised by it.
    pub cost: Cost,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Cost {
    /// Seconds, no privileges. These are the steps the run flow executes.
    Cheap,
    /// Needs root — live-build unpacks a chroot and mounts things.
    NeedsRoot,
    /// Needs root and takes tens of minutes to hours.
    Long,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub config: String,
    /// The profile as the user chose it (`both` stays `both`).
    pub requested_profile: String,
    /// What `both` expands into: the profiles that really get built.
    pub effective_profiles: Vec<String>,
    /// Set when the config, not the CLI, decided the profile — `mkconfig.sh`
    /// honours `YGG_BUILD_PROFILE` when `--profile` is absent, and a user who
    /// does not know that will not understand the command lines below.
    pub profile_from_config: Option<String>,
    pub steps: Vec<Step>,
    /// `KEY=value` pairs, exactly as the build will source them.
    pub env: Vec<(String, String)>,
    /// How this config differs from the shipped example: the honest answer to
    /// "what am I actually changing".
    pub delta: Vec<Delta>,
    pub skip_smoke: bool,
    /// Anything that would make the real build fail before it starts.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Delta {
    pub key: String,
    pub baseline: Option<String>,
    pub value: Option<String>,
    pub kind: &'static str,
}

/// The example every other config is read as a delta against.
const BASELINE: &str = "ygg.example.toml";

pub fn build(root: &Path, config: &str, profile: &str, skip_smoke: bool) -> Result<Plan, String> {
    let config_path = root.join(config);
    if !config_path.is_file() {
        // The same failure the shell gives, with the same remedy, rather than a
        // stack trace: `mkconfig.sh` exits here too.
        return Err(format!(
            "config file not found: {config}. Create it from {BASELINE} and re-run."
        ));
    }

    let env = resolve_env(root, config)?;
    let lookup = |key: &str| {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };

    // Faithful to the shell: an explicit --profile wins; otherwise the config's
    // build_profile does; otherwise the script's own default of `both`.
    let mut requested = profile.to_string();
    let mut profile_from_config = None;
    if profile.is_empty() {
        match lookup("YGG_BUILD_PROFILE") {
            Some(from_config) if !from_config.is_empty() => {
                profile_from_config = Some(from_config.clone());
                requested = from_config;
            }
            _ => requested = "both".to_string(),
        }
    }

    let effective_profiles = match requested.as_str() {
        "both" => vec!["server".to_string(), "kde".to_string()],
        other => vec![other.to_string()],
    };

    let qemu_smoke = lookup("YGG_ENABLE_QEMU_SMOKE").as_deref() == Some("true");

    let mut warnings = Vec::new();
    if discover::which("lb").is_none() {
        warnings.push(
            "live-build (`lb`) is not on PATH, so the image stages cannot run on this host. \
             The plan and the config stages below are still real."
                .to_string(),
        );
    }
    if config.contains("example") {
        warnings.push(format!(
            "{config} is an example meant to be copied, not built from. \
             Copy it to {} and edit that.",
            discover::DEFAULT_CONFIG
        ));
    }
    if qemu_smoke && discover::which("qemu-system-x86_64").is_none() {
        warnings.push(
            "enable_qemu_smoke is true but qemu-system-x86_64 is not on PATH, \
             so the boot smoke stage would fail."
                .to_string(),
        );
    }

    let mut steps = Vec::new();
    let mut push = |title: &str, command: Vec<&str>, note: &str, cost: Cost| {
        steps.push(Step {
            index: steps.len() + 1,
            title: title.to_string(),
            command: command.into_iter().map(str::to_string).collect(),
            note: note.to_string(),
            cost,
        });
    };

    push(
        "Convert the config to environment",
        vec!["./scripts/toml-to-env.sh", config],
        "mkconfig.sh writes this to a temporary /tmp/ygg-config-*.env and sources it. \
         The run flow below executes this step for real.",
        Cost::Cheap,
    );

    for name in &effective_profiles {
        push(
            &format!("Build the {name} image"),
            vec![
                "./scripts/build-profile.sh",
                "--profile",
                name,
                "--config",
                "/tmp/ygg-config-*.env",
            ],
            "Generates the live-build tree from the config, then runs `lb config` and \
             `lb build`. This is the stage that unpacks a chroot and needs root.",
            Cost::Long,
        );
    }

    if !skip_smoke {
        let mut smoke = vec![
            "./tests/smoke/run.sh",
            "--profile",
            &requested,
            "--require-artifacts",
            "--with-iso-rootfs",
            "--artifacts-dir",
            "./artifacts",
            "--server-iso",
            "./artifacts/server-latest.iso",
            "--kde-iso",
            "./artifacts/kde-latest.iso",
        ];
        if qemu_smoke {
            smoke.push("--with-qemu-boot");
        }
        push(
            "Smoke-test the artifacts",
            smoke,
            // Worth stating: it looks like a bug until you read the shell.
            "Note that the smoke stage receives the profile UNEXPANDED — `both` is passed \
             through as `both`, not run twice.",
            if qemu_smoke { Cost::NeedsRoot } else { Cost::Cheap },
        );
    }

    Ok(Plan {
        config: config.to_string(),
        requested_profile: requested,
        effective_profiles,
        profile_from_config,
        steps,
        delta: delta_against_baseline(root, config),
        env,
        skip_smoke,
        warnings,
    })
}

/// Run the repository's own converter and parse what it prints.
///
/// Shelling out rather than reimplementing is the point: the build sources the
/// output of this exact script, so this is the environment, not a model of it.
pub fn resolve_env(root: &Path, config: &str) -> Result<Vec<(String, String)>, String> {
    let script = root.join("scripts/toml-to-env.sh");
    if !script.is_file() {
        return Err(format!("{} is missing", script.display()));
    }
    let output = std::process::Command::new("bash")
        .arg(&script)
        .arg(config)
        .current_dir(root)
        .output()
        .map_err(|err| format!("could not run {}: {err}", script.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} exited {}: {}",
            script.display(),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            (
                key.to_string(),
                value.trim_matches('"').replace("\\\"", "\""),
            )
        })
        .collect())
}

/// Compare a config to the shipped example, key by key.
fn delta_against_baseline(root: &Path, config: &str) -> Vec<Delta> {
    if config == BASELINE {
        return Vec::new();
    }
    let (Ok(baseline_text), Ok(config_text)) = (
        std::fs::read_to_string(root.join(BASELINE)),
        std::fs::read_to_string(root.join(config)),
    ) else {
        return Vec::new();
    };
    let baseline = discover::parse_knobs(&baseline_text);
    let current = discover::parse_knobs(&config_text);

    let mut deltas = Vec::new();
    for knob in &current {
        match baseline.iter().find(|b| b.key == knob.key) {
            Some(base) if base.value == knob.value => {}
            Some(base) => deltas.push(Delta {
                key: knob.key.clone(),
                baseline: Some(base.value.clone()),
                value: Some(knob.value.clone()),
                kind: "changed",
            }),
            None => deltas.push(Delta {
                key: knob.key.clone(),
                baseline: None,
                value: Some(knob.value.clone()),
                kind: "added",
            }),
        }
    }
    for base in &baseline {
        if !current.iter().any(|k| k.key == base.key) {
            deltas.push(Delta {
                key: base.key.clone(),
                baseline: Some(base.value.clone()),
                value: None,
                // Not cosmetic: an absent key falls back to build-profile.sh's
                // own default, which is not always the example's value.
                kind: "absent",
            });
        }
    }
    deltas.sort_by(|a, b| a.key.cmp(&b.key));
    deltas
}
