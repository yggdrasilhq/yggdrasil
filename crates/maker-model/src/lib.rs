use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const SETUP_SCHEMA_VERSION: u32 = 1;
static SETUP_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresetId {
    Nas,
    DevHost,
    PersonalWorkstation,
    RecoveryAnchor,
}

impl PresetId {
    pub fn recommended_profile(self) -> BuildProfile {
        match self {
            Self::Nas | Self::RecoveryAnchor => BuildProfile::Server,
            Self::DevHost | Self::PersonalWorkstation => BuildProfile::Kde,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Nas => "nas",
            Self::DevHost => "dev-host",
            Self::PersonalWorkstation => "personal-workstation",
            Self::RecoveryAnchor => "recovery-anchor",
        }
    }
}

impl fmt::Display for PresetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

impl FromStr for PresetId {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "nas" => Ok(Self::Nas),
            "dev-host" => Ok(Self::DevHost),
            "personal-workstation" => Ok(Self::PersonalWorkstation),
            "recovery-anchor" => Ok(Self::RecoveryAnchor),
            _ => Err(ParseEnumError::new("preset", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildProfile {
    Server,
    Kde,
    Both,
}

impl BuildProfile {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Kde => "kde",
            Self::Both => "both",
        }
    }
}

impl fmt::Display for BuildProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

impl FromStr for BuildProfile {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "server" => Ok(Self::Server),
            "kde" => Ok(Self::Kde),
            "both" => Ok(Self::Both),
            _ => Err(ParseEnumError::new("profile", value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensitiveField<T> {
    pub remember: bool,
    pub value: Option<T>,
}

impl<T> SensitiveField<T> {
    pub fn ephemeral(value: T) -> Self {
        Self {
            remember: false,
            value: Some(value),
        }
    }

    pub fn persisted(value: T) -> Self {
        Self {
            remember: true,
            value: Some(value),
        }
    }

    pub fn build_value(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

impl<T: Clone> SensitiveField<T> {
    pub fn sanitized_for_persistence(&self) -> Self {
        if self.remember {
            self.clone()
        } else {
            Self {
                remember: false,
                value: None,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupDocument {
    #[serde(default = "default_setup_id")]
    pub setup_id: String,
    #[serde(default = "default_journey_stage")]
    pub journey_stage: JourneyStage,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub setup: Setup,
}

impl SetupDocument {
    pub fn new(name: String, preset: PresetId) -> Self {
        Self {
            setup_id: default_setup_id(),
            journey_stage: JourneyStage::Outcome,
            schema_version: SETUP_SCHEMA_VERSION,
            setup: Setup::new(name, preset),
        }
    }

    pub fn storage_filename(&self) -> String {
        format!("{}--{}.maker.json", self.setup.slug(), self.setup_id)
    }

    pub fn migrate_to_current(mut self) -> Result<Self, ValidationError> {
        if self.schema_version > SETUP_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedSchemaVersion {
                found: self.schema_version,
                expected: SETUP_SCHEMA_VERSION,
            });
        }

        self.schema_version = SETUP_SCHEMA_VERSION;
        if self.setup_id.trim().is_empty() {
            self.setup_id = default_setup_id();
        }
        Ok(self)
    }

    pub fn sanitized_for_persistence(&self) -> Self {
        let mut cloned = self.clone();
        cloned.setup.ssh.authorized_keys_file = cloned
            .setup
            .ssh
            .authorized_keys_file
            .sanitized_for_persistence();
        cloned.setup.ssh.host_keys_dir = cloned.setup.ssh.host_keys_dir.sanitized_for_persistence();
        cloned
    }

    pub fn validate(&self) -> Result<ValidatedBuildConfig, ValidationError> {
        if self.schema_version != SETUP_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedSchemaVersion {
                found: self.schema_version,
                expected: SETUP_SCHEMA_VERSION,
            });
        }

        let profile = self
            .setup
            .profile_override
            .unwrap_or_else(|| self.setup.preset.recommended_profile());

        if self.setup.personalization.net_mode == NetMode::Static
            && self.setup.personalization.static_ip.trim().is_empty()
        {
            return Err(ValidationError::MissingStaticIp);
        }

        if self.setup.personalization.hostname.trim().is_empty() {
            return Err(ValidationError::MissingHostname);
        }

        Ok(ValidatedBuildConfig {
            build_profile: profile,
            enable_qemu_smoke: self.setup.smoke.enable_qemu_smoke,
            with_nvidia: self.setup.hardware.with_nvidia,
            with_lts: self.setup.hardware.with_lts,
            setup_mode: "recommended".to_owned(),
            apt_proxy_mode: self.setup.apt.apt_proxy_mode.clone(),
            infisical_boot_mode: self.setup.apt.infisical_boot_mode.clone(),
            infisical_container_name: self.setup.apt.infisical_container_name.clone(),
            embed_ssh_keys: self.setup.ssh.embed_ssh_keys,
            ssh_authorized_keys_file: self
                .setup
                .ssh
                .authorized_keys_file
                .build_value()
                .cloned()
                .unwrap_or_else(|| "/root/.ssh/authorized_keys".to_owned()),
            ssh_host_keys_dir: self
                .setup
                .ssh
                .host_keys_dir
                .build_value()
                .cloned()
                .unwrap_or_default(),
            hostname: self.setup.personalization.hostname.clone(),
            net_mode: self.setup.personalization.net_mode,
            lxc_parent_if: self.setup.personalization.lxc_parent_if.clone(),
            macvlan_cidr: self.setup.personalization.macvlan_cidr.clone(),
            macvlan_route: self.setup.personalization.macvlan_route.clone(),
            static_iface: self.setup.personalization.static_iface.clone(),
            static_ip: self.setup.personalization.static_ip.clone(),
            static_gateway: self.setup.personalization.static_gateway.clone(),
            static_dns: self.setup.personalization.static_dns.clone(),
            apt_http_proxy: self.setup.apt.apt_http_proxy.clone(),
            apt_https_proxy: self.setup.apt.apt_https_proxy.clone(),
            apt_proxy_bypass_host: self.setup.apt.apt_proxy_bypass_host.clone(),
            enable_intel_arc_sriov: self.setup.hardware.enable_intel_arc_sriov,
            intel_arc_sriov_release: self.setup.hardware.intel_arc_sriov_release.clone(),
            intel_arc_sriov_vf_count: self.setup.hardware.intel_arc_sriov_vf_count,
            intel_arc_sriov_pf_pci: self.setup.hardware.intel_arc_sriov_pf_pci.clone(),
            intel_arc_sriov_device_id: self.setup.hardware.intel_arc_sriov_device_id.clone(),
            intel_arc_sriov_bind_vfs: self.setup.hardware.intel_arc_sriov_bind_vfs.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum JourneyStage {
    #[default]
    Outcome,
    Profile,
    Personalize,
    Review,
    Build,
    Boot,
}

impl JourneyStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Outcome => "Outcome",
            Self::Profile => "Profile",
            Self::Personalize => "Personalize",
            Self::Review => "Review",
            Self::Build => "Build",
            Self::Boot => "Boot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    pub name: String,
    pub preset: PresetId,
    pub profile_override: Option<BuildProfile>,
    pub personalization: Personalization,
    pub hardware: HardwareChoices,
    pub apt: AptChoices,
    pub ssh: SshChoices,
    pub smoke: SmokeChoices,
}

impl Setup {
    pub fn new(name: String, preset: PresetId) -> Self {
        Self {
            name,
            preset,
            profile_override: None,
            personalization: Personalization::default(),
            hardware: HardwareChoices::default(),
            apt: AptChoices::default(),
            ssh: SshChoices::default(),
            smoke: SmokeChoices::default(),
        }
    }

    pub fn slug(&self) -> String {
        self.name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches('-')
            .to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Personalization {
    pub hostname: String,
    pub net_mode: NetMode,
    pub lxc_parent_if: String,
    pub macvlan_cidr: String,
    pub macvlan_route: String,
    pub static_iface: String,
    pub static_ip: String,
    pub static_gateway: String,
    pub static_dns: String,
}

impl Default for Personalization {
    fn default() -> Self {
        Self {
            hostname: "yggdrasil".to_owned(),
            net_mode: NetMode::Dhcp,
            lxc_parent_if: "eno1".to_owned(),
            macvlan_cidr: "10.10.0.250/24".to_owned(),
            macvlan_route: "10.10.0.0/24".to_owned(),
            static_iface: "eno1".to_owned(),
            static_ip: String::new(),
            static_gateway: String::new(),
            static_dns: "1.1.1.1 8.8.8.8".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareChoices {
    pub with_nvidia: bool,
    pub with_lts: bool,
    pub enable_intel_arc_sriov: bool,
    pub intel_arc_sriov_release: String,
    pub intel_arc_sriov_vf_count: u32,
    pub intel_arc_sriov_pf_pci: String,
    pub intel_arc_sriov_device_id: String,
    pub intel_arc_sriov_bind_vfs: String,
}

impl Default for HardwareChoices {
    fn default() -> Self {
        Self {
            with_nvidia: false,
            with_lts: false,
            enable_intel_arc_sriov: false,
            intel_arc_sriov_release: "2026.03.05".to_owned(),
            intel_arc_sriov_vf_count: 7,
            intel_arc_sriov_pf_pci: String::new(),
            intel_arc_sriov_device_id: "0x56a0".to_owned(),
            intel_arc_sriov_bind_vfs: "vfio-pci".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AptChoices {
    pub apt_proxy_mode: String,
    pub infisical_boot_mode: String,
    pub infisical_container_name: String,
    pub apt_http_proxy: String,
    pub apt_https_proxy: String,
    pub apt_proxy_bypass_host: String,
}

impl Default for AptChoices {
    fn default() -> Self {
        Self {
            apt_proxy_mode: "off".to_owned(),
            infisical_boot_mode: "disabled".to_owned(),
            infisical_container_name: "infisical".to_owned(),
            apt_http_proxy: String::new(),
            apt_https_proxy: String::new(),
            apt_proxy_bypass_host: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshChoices {
    pub embed_ssh_keys: bool,
    pub authorized_keys_file: SensitiveField<String>,
    pub host_keys_dir: SensitiveField<String>,
}

impl Default for SshChoices {
    fn default() -> Self {
        let default_authorized_keys = std::env::var("HOME")
            .map(|home| format!("{home}/.ssh/authorized_keys"))
            .unwrap_or_else(|_| String::new());
        Self {
            embed_ssh_keys: true,
            authorized_keys_file: SensitiveField::ephemeral(default_authorized_keys),
            host_keys_dir: SensitiveField {
                remember: false,
                value: Some(String::new()),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmokeChoices {
    pub enable_qemu_smoke: bool,
}

impl Default for SmokeChoices {
    fn default() -> Self {
        Self {
            enable_qemu_smoke: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetMode {
    Dhcp,
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedBuildConfig {
    pub build_profile: BuildProfile,
    pub enable_qemu_smoke: bool,
    pub with_nvidia: bool,
    pub with_lts: bool,
    pub setup_mode: String,
    pub apt_proxy_mode: String,
    pub infisical_boot_mode: String,
    pub infisical_container_name: String,
    pub embed_ssh_keys: bool,
    pub ssh_authorized_keys_file: String,
    pub ssh_host_keys_dir: String,
    pub hostname: String,
    pub net_mode: NetMode,
    pub lxc_parent_if: String,
    pub macvlan_cidr: String,
    pub macvlan_route: String,
    pub static_iface: String,
    pub static_ip: String,
    pub static_gateway: String,
    pub static_dns: String,
    pub apt_http_proxy: String,
    pub apt_https_proxy: String,
    pub apt_proxy_bypass_host: String,
    pub enable_intel_arc_sriov: bool,
    pub intel_arc_sriov_release: String,
    pub intel_arc_sriov_vf_count: u32,
    pub intel_arc_sriov_pf_pci: String,
    pub intel_arc_sriov_device_id: String,
    pub intel_arc_sriov_bind_vfs: String,
}

impl ValidatedBuildConfig {
    pub fn to_native_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(&NativeConfig::from(self))
    }
}

#[derive(Debug, Serialize)]
struct NativeConfig<'a> {
    build_profile: &'a str,
    enable_qemu_smoke: bool,
    with_nvidia: bool,
    with_lts: bool,
    setup_mode: &'a str,
    apt_proxy_mode: &'a str,
    infisical_boot_mode: &'a str,
    infisical_container_name: &'a str,
    embed_ssh_keys: bool,
    ssh_authorized_keys_file: &'a str,
    ssh_host_keys_dir: &'a str,
    hostname: &'a str,
    net_mode: &'a str,
    lxc_parent_if: &'a str,
    macvlan_cidr: &'a str,
    macvlan_route: &'a str,
    static_iface: &'a str,
    static_ip: &'a str,
    static_gateway: &'a str,
    static_dns: &'a str,
    apt_http_proxy: &'a str,
    apt_https_proxy: &'a str,
    apt_proxy_bypass_host: &'a str,
    enable_intel_arc_sriov: bool,
    intel_arc_sriov_release: &'a str,
    intel_arc_sriov_vf_count: u32,
    intel_arc_sriov_pf_pci: &'a str,
    intel_arc_sriov_device_id: &'a str,
    intel_arc_sriov_bind_vfs: &'a str,
}

impl<'a> From<&'a ValidatedBuildConfig> for NativeConfig<'a> {
    fn from(value: &'a ValidatedBuildConfig) -> Self {
        Self {
            build_profile: value.build_profile.slug(),
            enable_qemu_smoke: value.enable_qemu_smoke,
            with_nvidia: value.with_nvidia,
            with_lts: value.with_lts,
            setup_mode: &value.setup_mode,
            apt_proxy_mode: &value.apt_proxy_mode,
            infisical_boot_mode: &value.infisical_boot_mode,
            infisical_container_name: &value.infisical_container_name,
            embed_ssh_keys: value.embed_ssh_keys,
            ssh_authorized_keys_file: &value.ssh_authorized_keys_file,
            ssh_host_keys_dir: &value.ssh_host_keys_dir,
            hostname: &value.hostname,
            net_mode: match value.net_mode {
                NetMode::Dhcp => "dhcp",
                NetMode::Static => "static",
            },
            lxc_parent_if: &value.lxc_parent_if,
            macvlan_cidr: &value.macvlan_cidr,
            macvlan_route: &value.macvlan_route,
            static_iface: &value.static_iface,
            static_ip: &value.static_ip,
            static_gateway: &value.static_gateway,
            static_dns: &value.static_dns,
            apt_http_proxy: &value.apt_http_proxy,
            apt_https_proxy: &value.apt_https_proxy,
            apt_proxy_bypass_host: &value.apt_proxy_bypass_host,
            enable_intel_arc_sriov: value.enable_intel_arc_sriov,
            intel_arc_sriov_release: &value.intel_arc_sriov_release,
            intel_arc_sriov_vf_count: value.intel_arc_sriov_vf_count,
            intel_arc_sriov_pf_pci: &value.intel_arc_sriov_pf_pci,
            intel_arc_sriov_device_id: &value.intel_arc_sriov_device_id,
            intel_arc_sriov_bind_vfs: &value.intel_arc_sriov_bind_vfs,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("setup schema version {found} is unsupported; expected {expected}")]
    UnsupportedSchemaVersion { found: u32, expected: u32 },
    #[error("static networking requires static_ip")]
    MissingStaticIp,
    #[error("hostname is required")]
    MissingHostname,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {kind}: {value}")]
pub struct ParseEnumError {
    kind: &'static str,
    value: String,
}

impl ParseEnumError {
    fn new(kind: &'static str, value: &str) -> Self {
        Self {
            kind,
            value: value.to_owned(),
        }
    }
}

fn default_schema_version() -> u32 {
    SETUP_SCHEMA_VERSION
}

fn default_journey_stage() -> JourneyStage {
    JourneyStage::Outcome
}

fn default_setup_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let suffix = SETUP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("setup-{millis}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_mapping_matches_expected_profiles() {
        assert_eq!(PresetId::Nas.recommended_profile(), BuildProfile::Server);
        assert_eq!(PresetId::DevHost.recommended_profile(), BuildProfile::Kde);
    }

    #[test]
    fn sanitizing_setup_removes_ephemeral_sensitive_values() {
        let mut document = SetupDocument::new("My NAS".to_owned(), PresetId::Nas);
        document.setup.ssh.authorized_keys_file =
            SensitiveField::ephemeral("/home/pi/.ssh/authorized_keys".to_owned());
        document.setup.ssh.host_keys_dir =
            SensitiveField::persisted("/home/pi/.config/ygg/host-keys".to_owned());

        let sanitized = document.sanitized_for_persistence();

        assert_eq!(sanitized.setup.ssh.authorized_keys_file.value, None);
        assert_eq!(
            sanitized.setup.ssh.host_keys_dir.value.as_deref(),
            Some("/home/pi/.config/ygg/host-keys")
        );
    }

    #[test]
    fn static_network_requires_ip() {
        let mut document = SetupDocument::new("Lab".to_owned(), PresetId::RecoveryAnchor);
        document.setup.personalization.net_mode = NetMode::Static;
        document.setup.personalization.static_ip.clear();

        let error = document.validate().unwrap_err();
        assert_eq!(error, ValidationError::MissingStaticIp);
    }

    #[test]
    fn validated_config_emits_native_toml() {
        let mut document = SetupDocument::new("Dev Box".to_owned(), PresetId::DevHost);
        document.setup.personalization.hostname = "devbox".to_owned();
        let config = document.validate().expect("valid setup");
        let toml = config.to_native_toml().expect("emit config");
        let parsed = toml.parse::<toml::Table>().expect("parse output");

        assert_eq!(
            parsed.get("build_profile").and_then(|value| value.as_str()),
            Some("kde")
        );
        assert_eq!(
            parsed.get("hostname").and_then(|value| value.as_str()),
            Some("devbox")
        );
    }

    #[test]
    fn migration_updates_legacy_documents() {
        let mut document = SetupDocument::new("Legacy".to_owned(), PresetId::Nas);
        document.schema_version = 0;
        document.setup_id.clear();

        let migrated = document.migrate_to_current().expect("migrate legacy document");
        assert_eq!(migrated.schema_version, SETUP_SCHEMA_VERSION);
        assert!(!migrated.setup_id.is_empty());
    }
}
