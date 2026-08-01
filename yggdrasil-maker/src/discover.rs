//! What the repository actually says about itself.
//!
//! Every fact on the home view comes from reading the working tree. Nothing
//! here has a hard-coded fallback that could present a plausible-looking answer
//! when the real one could not be read: if a thing cannot be discovered, the
//! field says so and the UI shows that it could not be discovered. A build tool
//! that quietly invents a profile list is worse than one that admits it is lost.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The files that together mean "this is a Yggdrasil checkout". All three must
/// be present: `mkconfig.sh` alone also matches a half-copied directory.
const ROOT_MARKERS: [&str; 3] = ["mkconfig.sh", "scripts/build-profile.sh", "config"];

/// Walk up from `start` looking for the repository root.
///
/// Walking up (rather than requiring the CLI to be run from the top) is what
/// lets the app be launched from wherever the user's terminal happens to be
/// sitting, which for a yggterm session is usually a subdirectory.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        if ROOT_MARKERS.iter().all(|marker| dir.join(marker).exists()) {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

/// One `--profile` value the build entry point will accept.
#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub name: String,
    /// True for a value that expands to several real builds rather than naming
    /// one image (`both`). The UI needs this to explain why picking it renders
    /// two command lines.
    pub composite: bool,
}

/// A `ygg*.toml` sitting in the repo root.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigFile {
    pub name: String,
    pub path: String,
    /// The leading comment block, which every shipped config uses to say what
    /// it is for. Empty when the file opens straight into keys.
    pub summary: String,
    pub knob_count: usize,
    /// True for the file `mkconfig.sh` reaches for when `--config` is omitted.
    pub is_default: bool,
    /// True for a file that is an example to copy rather than one to build
    /// from. Building from an example is a real mistake the old GUI allowed.
    pub is_example: bool,
}

/// One key in a config file, with the comment that documents it.
#[derive(Debug, Clone, Serialize)]
pub struct Knob {
    pub key: String,
    pub value: String,
    /// The trailing `# ...` on the key's own line, which is where this repo
    /// puts the enum of legal values.
    pub hint: String,
    /// The `# --- Section ---` banner this key sits under, if any.
    pub section: String,
}

/// Everything the home view shows, and everything the other flows resolve
/// against.
#[derive(Debug, Clone, Serialize)]
pub struct Repo {
    pub root: String,
    pub profiles: Vec<Profile>,
    /// Set when the profile list could not be read out of `mkconfig.sh`. The UI
    /// renders this instead of a profile list, rather than guessing.
    pub profiles_error: Option<String>,
    pub configs: Vec<ConfigFile>,
    /// `lb` is live-build itself. Absent means a real build cannot run here,
    /// which the user should learn on the home view and not 40 minutes in.
    pub live_build_present: bool,
    pub artifacts: Vec<Artifact>,
    pub hooks_count: usize,
    pub package_lists: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Artifact {
    pub name: String,
    pub bytes: u64,
}

pub fn survey(root: &Path) -> Repo {
    let (profiles, profiles_error) = match read_profiles(root) {
        Ok(profiles) => (profiles, None),
        Err(err) => (Vec::new(), Some(err)),
    };
    Repo {
        root: root.display().to_string(),
        profiles,
        profiles_error,
        configs: read_configs(root),
        live_build_present: which("lb").is_some(),
        artifacts: read_artifacts(root),
        hooks_count: count_files(&root.join("config/hooks")),
        package_lists: list_names(&root.join("config/package-lists")),
    }
}

/// Read the legal `--profile` values out of `mkconfig.sh`'s own usage text.
///
/// The usage block is the entry point's published contract, so parsing it keeps
/// this app honest: add a profile to the shell script and the app shows it,
/// with no second list here to fall out of step. If the line ever stops
/// matching we return an error and the UI says the list is unavailable — we do
/// not fall back to a remembered `server|kde|both`, because a stale hard-coded
/// answer is exactly the failure this design is avoiding.
fn read_profiles(root: &Path) -> Result<Vec<Profile>, String> {
    let path = root.join("mkconfig.sh");
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;

    let line = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("--profile "))
        .ok_or_else(|| format!("no `--profile` line in {}'s usage block", path.display()))?;

    let spec = line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("the `--profile` line in {} names no values", path.display()))?;

    let profiles: Vec<Profile> = spec
        .split('|')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| Profile {
            name: name.to_string(),
            // `both` is the one composite the script defines; it is spelled out
            // rather than inferred so a future `all` does not silently become a
            // single image in the UI.
            composite: name == "both",
        })
        .collect();

    if profiles.is_empty() {
        return Err(format!("the `--profile` line in {} parsed to nothing", path.display()));
    }
    Ok(profiles)
}

/// The default `--config` when the flag is omitted, per `mkconfig.sh`.
pub const DEFAULT_CONFIG: &str = "ygg.local.toml";

fn read_configs(root: &Path) -> Vec<ConfigFile> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut configs: Vec<ConfigFile> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("ygg") || !name.ends_with(".toml") {
                return None;
            }
            let text = std::fs::read_to_string(entry.path()).ok()?;
            Some(ConfigFile {
                summary: leading_comment(&text),
                knob_count: parse_knobs(&text).len(),
                is_default: name == DEFAULT_CONFIG,
                is_example: name.contains("example"),
                path: name.clone(),
                name,
            })
        })
        .collect();
    // Deterministic order: the buildable config first, then examples, then
    // alphabetically. Row order must not depend on readdir.
    configs.sort_by(|a, b| {
        a.is_example
            .cmp(&b.is_example)
            .then_with(|| a.name.cmp(&b.name))
    });
    configs
}

/// The comment block at the top of a file, as prose.
fn leading_comment(text: &str) -> String {
    text.lines()
        .take_while(|line| line.trim_start().starts_with('#'))
        .map(|line| line.trim_start().trim_start_matches('#').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse a Yggdrasil config into keys, values and their documenting comments.
///
/// This deliberately mirrors what `scripts/toml-to-env.sh` accepts — flat
/// `key = value` lines, `#` comments, `[table]` headers skipped — rather than
/// being a general TOML parser. A stricter parser here would accept files the
/// build then rejects, or reject files the build builds; matching the shell is
/// what keeps the preview honest.
pub fn parse_knobs(text: &str) -> Vec<Knob> {
    let mut knobs = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        if let Some(banner) = section_banner(trimmed) {
            section = banner;
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
            continue;
        }
        let Some((key, rest)) = trimmed.split_once('=') else {
            continue;
        };
        let (value, hint) = split_trailing_comment(rest);
        knobs.push(Knob {
            key: key.trim().to_string(),
            value: unquote(value.trim()),
            hint: hint.trim().to_string(),
            section: section.clone(),
        });
    }
    knobs
}

/// `# --- Disk health alerting ---------` -> `Disk health alerting`.
fn section_banner(line: &str) -> Option<String> {
    let rest = line.strip_prefix("#")?.trim_start();
    let rest = rest.strip_prefix("---")?;
    let banner = rest.trim().trim_end_matches('-').trim();
    (!banner.is_empty()).then(|| banner.to_string())
}

/// Split `"value" # hint` while leaving a `#` inside quotes alone.
fn split_trailing_comment(rest: &str) -> (&str, &str) {
    let mut in_quotes = false;
    for (index, ch) in rest.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return (&rest[..index], &rest[index + 1..]),
            _ => {}
        }
    }
    (rest, "")
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

fn read_artifacts(root: &Path) -> Vec<Artifact> {
    let Ok(entries) = std::fs::read_dir(root.join("artifacts")) else {
        return Vec::new();
    };
    let mut artifacts: Vec<Artifact> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".iso") {
                return None;
            }
            Some(Artifact {
                bytes: entry.metadata().ok()?.len(),
                name,
            })
        })
        .collect();
    artifacts.sort_by(|a, b| a.name.cmp(&b.name));
    artifacts
}

/// Count entries beneath `dir` that are not directories, recursively.
///
/// Two deliberate choices, both of which a later "cleanup" would get wrong:
///
/// - Recursion, because live-build keeps hooks in stage subdirectories
///   (`config/hooks/normal/…`); a shallow count reports zero on a checkout
///   with 37 hooks in it.
/// - Symlinks count. Most of this repo's hooks are symlinks into live-build's
///   stock hook set, and a symlinked hook runs exactly like a copied one.
///   Filtering to `is_file()` would report 12 where git reports 37.
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                count_files(&path)
            } else {
                1
            }
        })
        .sum()
}

fn list_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Resolve a bare command name against PATH.
pub fn which(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knobs_carry_their_hint_and_section() {
        let text = "\
# leading prose
build_profile = \"both\"            # server | kde | both
enable_qemu_smoke = true

# --- Disk health alerting ------------------------------------------------
alert_email = \"\"                  # e.g. \"alerts@example.com\"
";
        let knobs = parse_knobs(text);
        assert_eq!(knobs.len(), 3);
        assert_eq!(knobs[0].key, "build_profile");
        assert_eq!(knobs[0].value, "both");
        assert_eq!(knobs[0].hint, "server | kde | both");
        assert_eq!(knobs[0].section, "");
        assert_eq!(knobs[2].key, "alert_email");
        assert_eq!(knobs[2].section, "Disk health alerting");
    }

    #[test]
    fn a_hash_inside_quotes_is_not_a_comment() {
        let knobs = parse_knobs("mail_from = \"root@a#b\" # real hint");
        assert_eq!(knobs[0].value, "root@a#b");
        assert_eq!(knobs[0].hint, "real hint");
    }

    #[test]
    fn missing_usage_block_is_an_error_not_a_guess() {
        // The whole point: when the entry point cannot be read, we must not
        // produce a plausible profile list from memory.
        let dir = std::env::temp_dir().join("ygg-maker-test-no-usage");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("mkconfig.sh"), "#!/bin/sh\necho hi\n");
        let result = read_profiles(&dir);
        assert!(result.is_err(), "expected an error, got {result:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
