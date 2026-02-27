#!/bin/bash
# =============================================================================
# Yggdrasil Build Configuration
# =============================================================================
# Purpose: Generate a custom Debian USB build with ZFS, LXC, and optional hypervisors
#
# Usage: ./mkconfig.sh [options]
#
# Environment Variables:
#    PASSWD           ignored. root/pi passwords are fixed to "yggdrasil".
#
# Options:
#   --with-xen        Include Xen hypervisor (default: off)
#   --with-kvm        Include KVM support (default: on)
#   --without-nvidia  Exclude NVIDIA support (default: on)
#   --with-kde        KDE laptop profile (broad firmware, NVIDIA disabled, hostname yggdrasil)
#
# Boot Process Notes:
# - GRUB is used as bootloader for both BIOS and UEFI
# - When Xen is enabled, it's properly integrated into GRUB config
# - ZFS pools are not force-imported on first boot
#
# References:
# 1. https://live-team.pages.debian.net/live-manual/html/live-manual/index.en.html
# 2. man lb config
# 3. man lb build
# 4. man lb clean
# 5. man lb live-build

# =============================================================================
# Command Line Argument Processing
# =============================================================================

WITH_XEN=false
WITH_KVM=false
WITH_NVIDIA=true
WITH_KDE=false
KDE_PROFILE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --with-xen)
            WITH_XEN=true
            shift
            ;;
        --with-kvm)
            WITH_KVM=true
            shift
            ;;
        --without-nvidia)
            WITH_NVIDIA=false
            shift
            ;;
        --with-kde)
            WITH_KDE=true
            KDE_PROFILE=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ "$KDE_PROFILE" == "true" ]]; then
    WITH_KDE=true
    WITH_NVIDIA=false
fi

BUILD_TIMESTAMP="$(date +"%Y%m%d%H%M")"
BUILD_DAY="$(date +"%Y%m%d")"
IMAGE_SUFFIX=""
if [[ "$WITH_KDE" == "true" ]]; then
    IMAGE_SUFFIX="-kde"
fi
IMAGE_NAME="yggdrasil-${BUILD_TIMESTAMP}${IMAGE_SUFFIX}"
MOK_WORKDIR=".build/secureboot"
MOK_KEY="${MOK_WORKDIR}/ygg-local-mok.key"
MOK_CERT_PEM="${MOK_WORKDIR}/ygg-local-mok.crt"
MOK_CERT_DER="${MOK_WORKDIR}/ygg-local-mok.cer"

# =============================================================================
# Environment Initialization
# =============================================================================

echo "Initializing Yggdrasil build environment..."
echo "Configuration Matrix:"
echo "  → Xen Support: $WITH_XEN"
echo "  → KVM Support: $WITH_KVM"
echo "  → NVIDIA Integration: $WITH_NVIDIA"
echo "  → KDE Integration: $WITH_KDE"
echo "  → KDE Profile: $KDE_PROFILE"

# =============================================================================
# Initial Cleanup
# =============================================================================

lb clean --purge
rm -fr config/

# =============================================================================
# Error Handling
# =============================================================================

set -euo pipefail

# Function framework for error management
handle_error() {
    local exit_code=$?
    echo "Error occurred in build process (exit code: $exit_code)"
    # Cleanup logic here
    exit $exit_code
}
trap handle_error ERR

mkdir -p "$MOK_WORKDIR"
if [[ ! -f "$MOK_KEY" || ! -f "$MOK_CERT_PEM" || ! -f "$MOK_CERT_DER" ]]; then
    echo "Generating local Secure Boot MOK keypair in ${MOK_WORKDIR}..."
    openssl req -new -x509 -newkey rsa:4096 \
        -keyout "$MOK_KEY" \
        -out "$MOK_CERT_PEM" \
        -nodes \
        -sha256 \
        -days 3650 \
        -subj "/CN=Yggdrasil Local MOK/"
    openssl x509 -outform DER -in "$MOK_CERT_PEM" -out "$MOK_CERT_DER"
fi

prune_iso_builds() {
    local cutoff_epoch
    cutoff_epoch=$(date -d '3 days ago' +%s)

    prune_iso_group() {
        local group_name="$1"
        shift
        local files=("$@")

        if [[ ${#files[@]} -eq 0 ]]; then
            return 0
        fi

        local recent_files=()
        local old_files=()
        local f mtime
        for f in "${files[@]}"; do
            mtime=$(stat -c %Y "$f")
            if [[ "$mtime" -ge "$cutoff_epoch" ]]; then
                recent_files+=("$f")
            else
                old_files+=("$f")
            fi
        done

        declare -A keep=()
        if [[ ${#recent_files[@]} -gt 0 ]]; then
            keep["${recent_files[0]}"]=1
        fi
        if [[ ${#old_files[@]} -gt 0 ]]; then
            keep["${old_files[0]}"]=1
        fi
        if [[ ${#old_files[@]} -gt 1 ]]; then
            keep["${old_files[1]}"]=1
        fi

        for f in "${files[@]}"; do
            if [[ -z "${keep[$f]+x}" ]]; then
                echo "Pruning old ${group_name} ISO artifact: $f"
                rm -f -- "$f"
            fi
        done
    }

    mapfile -t server_iso_files < <(ls -1t yggdrasil-*-amd64.hybrid.iso 2>/dev/null | rg -v -- '-kde-amd64\.hybrid\.iso$' || true)
    mapfile -t kde_iso_files < <(ls -1t yggdrasil-*-kde-amd64.hybrid.iso 2>/dev/null || true)

    prune_iso_group "server" "${server_iso_files[@]}"
    prune_iso_group "kde" "${kde_iso_files[@]}"
}

# =============================================================================
# Base Configuration
# =============================================================================
# Setting up base configuration for live-build
# Notes:
# - Uses minbase variant for minimal footprint
# - Sets up for both UEFI and BIOS boot
# - Configures systemd init system
# - Disables firmware packages by default for faster build

APT_PROXY_CONF="/etc/apt/apt.conf.d/02proxy"
APT_CACHE_TUNING_CONF="/etc/apt/apt.conf.d/03cache-tuning"

extract_proxy_value() {
    local key="$1" file="$2"
    awk -F'"' -v key="$key" '$0 ~ key {print $2; exit}' "$file"
}

APT_HTTP_PROXY="${YGG_APT_HTTP_PROXY:-}"
APT_HTTPS_PROXY="${YGG_APT_HTTPS_PROXY:-}"
APT_PROXY_BYPASS_HOST="${YGG_APT_PROXY_BYPASS_HOST:-}"

if [[ -z "$APT_HTTP_PROXY" && -f "$APT_PROXY_CONF" ]]; then
    APT_HTTP_PROXY=$(extract_proxy_value "Acquire::http::Proxy" "$APT_PROXY_CONF" || true)
fi
if [[ -z "$APT_HTTPS_PROXY" && -f "$APT_PROXY_CONF" ]]; then
    APT_HTTPS_PROXY=$(extract_proxy_value "Acquire::https::Proxy" "$APT_PROXY_CONF" || true)
fi
if [[ -z "$APT_HTTPS_PROXY" ]]; then
    APT_HTTPS_PROXY="$APT_HTTP_PROXY"
fi

LB_APT_PROXY_FLAGS=()
if [[ -n "$APT_HTTP_PROXY" ]]; then
    LB_APT_PROXY_FLAGS+=(--apt-http-proxy "$APT_HTTP_PROXY")
fi

LIVE_HOSTNAME="yggdrasil.local"
if [[ "$KDE_PROFILE" == "true" ]]; then
    LIVE_HOSTNAME="yggdrasil"
fi
if [[ -n "${YGG_HOSTNAME:-}" ]]; then
    LIVE_HOSTNAME="$YGG_HOSTNAME"
fi

lb config \
  --apt apt \
  --apt-recommends false \
  --apt-indices false \
  --apt-source-archives false \
  --architecture amd64 \
  --archive-areas "main contrib non-free non-free-firmware" \
  --backports false \
  --binary-filesystem fat32 \
  --binary-image iso-hybrid \
  --bootappend-live "boot=live edd=off noautologin components locales=en_US.UTF-8 keyboard-layouts=us hostname=${LIVE_HOSTNAME} timezone=Asia/Kolkata i915.enable_guc=3" \
  --bootloaders "grub-efi grub-pc" \
  --build-with-chroot true \
  --cache true \
  --cache-indices true \
  --cache-packages true \
  --chroot-filesystem squashfs \
  --chroot-squashfs-compression-type zstd \
  --chroot-squashfs-compression-level 15 \
  --clean \
  --color \
  --compression gzip \
  --conffile yggdrasil.config \
  --debian-installer none \
  --debootstrap-options "--variant=minbase" \
  --distribution unstable \
  --firmware-binary false \
  --firmware-chroot false \
  --ignore-system-defaults \
  --image-name "$IMAGE_NAME" \
  --initramfs live-boot \
  --initsystem systemd \
  --interactive false \
  --iso-application "$IMAGE_NAME" \
  --linux-flavours "amd64" \
  --linux-packages "linux-image linux-headers" \
  --memtest none \
  --parent-distribution unstable \
  --parent-distribution-binary unstable \
  --parent-distribution-chroot unstable \
  --security true \
  --uefi-secure-boot disable \
  --updates false \
  "${LB_APT_PROXY_FLAGS[@]}"

#  --linux-packages "linux-image linux-headers" \
#  --linux-packages "none" \

# Keep bundled SSH host keys; live-build's default hook removes them.
rm -f config/hooks/normal/8050-remove-openssh-server-host-keys.hook.chroot

# =============================================================================
# Package Lists Configuration
# =============================================================================
# Notes:
# - Firmware packages are minimal by default
# - Additional firmware can be enabled with flags
# - Base system uses minbase variant
# - Package selection focuses on server use case

# --- Firmware Integration Matrix ---
# Notes:
# - Server baseline remains conservative by default.
# - Optional KDE profile adds broader laptop firmware coverage.

cat <<'EOF' > config/package-lists/firmware.list.chroot

# AMD Platform Essentials
amd64-microcode
firmware-linux-free

# Intel GPU / Arc
firmware-intel-graphics
firmware-intel-misc
firmware-misc-nonfree

# BMC Integration
firmware-ast

# Wired network stack
firmware-bnx2
firmware-bnx2x
# firmware-tg3
EOF

if [[ "$KDE_PROFILE" == "true" ]]; then
   cat <<'EOF' >> config/package-lists/firmware.list.chroot
# KDE profile: broad laptop graphics/wifi firmware
firmware-linux
firmware-linux-nonfree
firmware-amd-graphics
firmware-iwlwifi
firmware-atheros
firmware-mediatek
firmware-realtek
firmware-brcm80211
firmware-libertas
firmware-ti-connectivity
EOF
fi

# Since, we will compile nvidia open source kernel modules, the gsp package
# is not needed. If a future decision to not include them is taken, then
# noveau will require them for NVK.

# if [[ "$WITH_NVIDIA" == "true" ]]; then
#     cat <<'EOF' >> config/package-lists/firmware.list.chroot
# firmware-nvidia-gsp
# EOF
# fi

# --- Base System Packages ---
cat <<'EOF' > config/package-lists/live.list.chroot
live-boot
live-config
live-config-systemd
systemd-sysv
EOF

# --- Core System Packages ---
cat <<'EOF' > config/package-lists/ygg.list.chroot
# Security and certificates
ca-certificates
ssl-cert

# Core networking
network-manager
wpasupplicant
wireless-regdb
iw
rfkill
iputils-ping

# Essential utilities
coreutils
bash
gpg
gpg-agent
gpm
ripgrep
openssh-server
squashfs-tools
gdisk
zstd
dmsetup
libdevmapper1.02.1
exfat-fuse
exfatprogs
fuse3
lxc
distrobuilder
apcupsd
apparmor
apparmor-utils
# unattended-upgrades # has a lot of dependencies including python

# Intel media stack for Arc/VAAPI
intel-media-va-driver
libigdgmm12
libva2
vainfo

# System monitoring
htop
ipmitool
lm-sensors
nvtop

# zfs
zfsutils-linux
zfs-dkms

# minimal userland
# cockpit
curl
less
git
vim
sudo
bash-completion
mokutil

EOF

# Add KVM packages if enabled
if [[ "$WITH_KVM" == "true" ]]; then
   cat <<'EOF' >> config/package-lists/ygg.list.chroot
# KVM packages for Android Studio development
qemu-system-x86
bridge-utils
cpu-checker
libvirt-daemon-system
libvirt-clients
EOF
fi

# Add Xen packages if enabled
if [[ "$WITH_XEN" == "true" ]]; then
   cat <<'EOF' >> config/package-lists/ygg.list.chroot
# Xen packages
xen-hypervisor
xen-tools
xen-utils
qemu-system-xen
qemu-utils
ovmf
seabios
EOF
fi

# Add KDE packages if enabled
if [[ "$WITH_KDE" == "true" ]]; then
   cat <<'EOF' >> config/package-lists/ygg.list.chroot
# KDE packages
tasksel
virt-manager
chromium
plasma-nm
EOF
fi

# =============================================================================
# Bootloader Configuration
# =============================================================================
# Notes:
# - Uses GRUB for both BIOS and UEFI
# - Xen integration is handled through GRUB when enabled
# - Custom timeout and theme settings
# - Includes sound support for accessibility
# References:
# - https://www.gnu.org/software/grub/manual/grub/html_node/
# - https://xenbits.xen.org/docs/unstable/misc/xen-command-line.html

# --- Xen Boot Parameters ---
# Only set if Xen is enabled
if [[ "$WITH_XEN" == "true" ]]; then
   export xen_append_live="dom0_mem=max:8192M "
fi

# --- GRUB Configuration ---
xen_append_live=${xen_append_live:-""}
mkdir -p config/bootloaders/grub-pc
cat <<'EOF' | sed "s/@APPEND_XEN@/$xen_append_live/g" > config/bootloaders/grub-pc/grub.cfg
# Theme and Font Configuration
if loadfont /boot/grub/unicode.pf2 ; then
   set gfxmode=auto
   set gfxpayload=keep
   insmod video_bochs
   insmod video_cirrus
   insmod all_video
fi

insmod gfxterm
insmod png
terminal_output gfxterm

# Audio feedback for accessibility
insmod play
play 960 440 1 0 4 440 1

# Color scheme configuration
# Reference: GNU GRUB manual
set color_normal=black/white
set color_highlight=brown/white
set menu_color_normal=black/white
set menu_color_highlight=brown/white

# Core GRUB modules
insmod gzio
insmod part_gpt
insmod ext2

# Menu Configuration
set default="0"
set timeout=5

EOF

# Add Xen menu entry if enabled
if [[ "$WITH_XEN" == "true" ]]; then
   cat <<'EOF' >> config/bootloaders/grub-pc/grub.cfg
menuentry "Live system with Xen" {
   multiboot2 /live/xen.gz @APPEND_XEN@
   module2 @KERNEL_LIVE@ @APPEND_LIVE@
   module2 @INITRD_LIVE@
}
EOF
fi

# Add standard boot entries
cat <<'EOF' >> config/bootloaders/grub-pc/grub.cfg
# Standard Live Boot
@LINUX_LIVE@

# Memtest (if available)
if @ENABLE_MEMTEST@; then
   source /boot/grub/memtest.cfg
fi

# UEFI Firmware Setup Option
if [ "${grub_platform}" = "efi" ]; then
   menuentry "UEFI Firmware Settings" {
       fwsetup
   }
fi
EOF

# --- Xen Kernel Copy Hook ---
# Only create if Xen is enabled
if [[ "$WITH_XEN" == "true" ]]; then
   tee config/hooks/normal/9030-copy-xen-to-live.hook.binary <<'EOF'
#!/bin/bash
# Copy Xen kernel to /live for boot
source_file=$(find ../chroot/boot/ -name 'xen*.gz' -type f -print -quit)
cp -a "$source_file" live/xen.gz
EOF
   chmod +777 config/hooks/normal/9030-copy-xen-to-live.hook.binary
fi

# =============================================================================
# System Configuration Hooks
# =============================================================================
# Notes:
# - Sets up root environment and SSH keys
# - Configures static networking and LXC bridge
# - Creates persistent /var on ZFS
# - SSH host keys are preserved across boots

PASSWD="yggdrasil"
export PASSWD
echo "Using fixed default password for root/pi users."
tee config/hooks/normal/9100-set-root.hook.chroot <<EOF
#!/bin/bash

# Root password setup
# Note: Change this password in production
echo "root:$PASSWD" | chpasswd

# Bash aliases and configuration
tee /root/.bashrc <<'EOL'
source /usr/share/bash-completion/bash_completion
alias ls='ls --color=auto'
alias ll='ls -alhF'
alias ls='ls --color=auto'
alias ll='ls -alhF --color=auto'
alias l='lxc-ls -f'
alias s='lxc-start -n'
alias x='lxc-stop -n'
alias a='lxc-attach -n'
alias c='lxc-copy -B zfs -s -n'
alias deb='c deb -N'
alias dock='c docker -N'
alias arch='c arch -N'
alias smb='c smb -N'
alias cvx='c convexdb -N'
EOL

systemctl enable ssh
EOF
chmod +777 config/hooks/normal/9100-set-root.hook.chroot

if [[ "$WITH_KDE" != "true" ]]; then
tee config/hooks/normal/9108-setup-nas-users.hook.chroot <<EOF
#!/bin/bash

# --- Step 1: Create the group if it doesn't exist ---
if ! getent group datashare >/dev/null; then
  groupadd --gid 3000 --system datashare
  echo "Group 'datashare' created."
else
  echo "Group 'datashare' already exists."
fi

# --- Step 2: Create the user with adduser ---
# --disabled-password -> Guarantees no password prompt.
# --disabled-login    -> Sets shell to /usr/sbin/nologin, a good security practice.
if ! getent passwd datauser >/dev/null; then
  adduser \
    --uid 3001 \
    --system \
    --no-create-home \
    --ingroup datashare \
    --disabled-password \
    --disabled-login \
    datauser
  echo "User 'datauser' created."
else
  echo "User 'datauser' already exists."
fi

EOF
chmod +777 config/hooks/normal/9108-setup-nas-users.hook.chroot
fi

# --- Enhanced SSH Server Configuration ---

# 1. Configure sshd_config for key-based root login
tee config/hooks/normal/9107-sshd-permitrootlogin.hook.chroot <<'EOF'
#!/bin/bash
SSHD_CONFIG="/etc/ssh/sshd_config"
sed -i 's|^#PermitRootLogin prohibit-password|PermitRootLogin prohibit-password|' "${SSHD_CONFIG}"
if ! grep -q "^PermitRootLogin prohibit-password" "${SSHD_CONFIG}"; then
    echo "Warning: Could not configure PermitRootLogin in ${SSHD_CONFIG}"
fi
EOF
chmod +777 config/hooks/normal/9107-sshd-permitrootlogin.hook.chroot

# 2. SSH host keys management

DEFAULT_SSH_KEY_DIR="./assets/ssh"
LEGACY_SSH_KEY_DIR="./ssh"

if [[ -d "$DEFAULT_SSH_KEY_DIR" ]]; then
    ssh_host_keys_dir="$DEFAULT_SSH_KEY_DIR"
elif [[ -d "$LEGACY_SSH_KEY_DIR" ]]; then
    ssh_host_keys_dir="$LEGACY_SSH_KEY_DIR"
else
    ssh_host_keys_dir="$DEFAULT_SSH_KEY_DIR"
    mkdir -p "$ssh_host_keys_dir"
fi

if [ ! -f "$ssh_host_keys_dir/ssh_host_rsa_key" ] || \
   [ ! -f "$ssh_host_keys_dir/ssh_host_ed25519_key" ] || \
   [ ! -f "$ssh_host_keys_dir/ssh_host_ecdsa_key" ]; then
    echo "Generating persistent SSH host keys in $ssh_host_keys_dir"
    ssh-keygen -t rsa -f "$ssh_host_keys_dir/ssh_host_rsa_key" -N ''
    ssh-keygen -t ed25519 -f "$ssh_host_keys_dir/ssh_host_ed25519_key" -N ''
    ssh-keygen -t ecdsa -f "$ssh_host_keys_dir/ssh_host_ecdsa_key" -N ''
fi

if [ ! -f "$ssh_host_keys_dir/hostid" ]; then
    hostid > "$ssh_host_keys_dir/hostid"
fi

# Copy SSH host keys to the build
target_ssh_config_dir="config/includes.chroot/etc/ssh"
mkdir -p "$target_ssh_config_dir"
cp "$ssh_host_keys_dir/ssh_host_"* "$target_ssh_config_dir/"
# Copy the hostid to the correct system location for ZFS
cp "$ssh_host_keys_dir/hostid" "config/includes.chroot/etc/hostid"

if [[ "$WITH_NVIDIA" == "true" ]]; then
    # Ensure Debian trixie repo is available for kernel/headers while the rest stays on unstable.
    mkdir -p config/archives
    cat <<'EOF' > config/archives/debian-trixie.list.chroot
deb http://deb.debian.org/debian trixie main contrib non-free non-free-firmware
EOF

    # Also ensure the generated system keeps this repo definition.
    mkdir -p "config/includes.chroot/etc/apt/sources.list.d" "config/includes.chroot_before_packages/etc/apt/sources.list.d"
    cat <<'EOF' > "config/includes.chroot/etc/apt/sources.list.d/debian-trixie.list"
deb http://deb.debian.org/debian trixie main contrib non-free non-free-firmware
EOF
    cp "config/includes.chroot/etc/apt/sources.list.d/debian-trixie.list" \
       "config/includes.chroot_before_packages/etc/apt/sources.list.d/debian-trixie.list"

    # Strong pin so kernels/headers/kbuild stick to trixie.
    cat <<'EOF' > config/archives/kernel-trixie.pref
Package: linux-image-amd64 linux-image-* linux-headers-amd64 linux-headers-* linux-kbuild-*
Pin: release n=trixie
Pin-Priority: 1001

Package: linux-image-amd64 linux-image-* linux-headers-amd64 linux-headers-* linux-kbuild-*
Pin: release n=unstable
Pin-Priority: 100
EOF
    mkdir -p "config/includes.chroot/etc/apt/preferences.d" "config/includes.chroot_before_packages/etc/apt/preferences.d"
    cp config/archives/kernel-trixie.pref "config/includes.chroot/etc/apt/preferences.d/kernel-trixie.pref"
    cp config/archives/kernel-trixie.pref "config/includes.chroot_before_packages/etc/apt/preferences.d/kernel-trixie.pref"
else
    echo "NVIDIA disabled: using default unstable (sid) kernel packages."
fi

# Pre-apt hook to patch zfs-dkms so it allows experimental kernels
mkdir -p "config/includes.chroot/opt/ygg" "config/includes.chroot_before_packages/opt/ygg"
cat <<'EOF' > "config/includes.chroot/opt/ygg/patch-zfs-dkms.sh"
#!/bin/bash
set -euo pipefail

cache_dir="/var/cache/apt/archives"
needs_patch=0

if ! [ -t 0 ]; then
    while read -r pkg; do
        if [[ "$pkg" == zfs-dkms* ]]; then
            needs_patch=1
            break
        fi
    done
fi

if [[ $needs_patch -eq 0 ]]; then
    exit 0
fi

shopt -s nullglob
for deb in "$cache_dir"/zfs-dkms_*.deb; do
    tmpdir=$(mktemp -d)
    dpkg-deb -R "$deb" "$tmpdir"
    srcdir=$(find "$tmpdir/usr/src" -maxdepth 1 -mindepth 1 -type d -name 'zfs-*' | head -n1 || true)
    if [[ -z "$srcdir" ]]; then
        rm -rf "$tmpdir"
        continue
    fi

    dkms_conf="$srcdir/dkms.conf"
    if grep -q -- '--enable-linux-experimental' "$dkms_conf"; then
        rm -rf "$tmpdir"
        continue
    fi

    echo "[ygg] Patching $(basename "$deb") for experimental kernel support"
    perl -0pi -e 's/(--with-linux-obj="\$\{kernel_source_dir\}"\n)/$1  --enable-linux-experimental\n/' "$dkms_conf"
    dpkg-deb -b "$tmpdir" "${deb}.patched" >/dev/null
    mv "${deb}.patched" "$deb"
    rm -rf "$tmpdir"
done
shopt -u nullglob
EOF
chmod +x "config/includes.chroot/opt/ygg/patch-zfs-dkms.sh"
cp "config/includes.chroot/opt/ygg/patch-zfs-dkms.sh" "config/includes.chroot_before_packages/opt/ygg/patch-zfs-dkms.sh"

# Inject Secure Boot signing material for build-time module signing.
# Late hooks will remove private keys before the image is finalized.
mkdir -p "config/includes.chroot/root/.secureboot" "config/includes.chroot_before_packages/root/.secureboot"
cp "$MOK_KEY" "config/includes.chroot/root/.secureboot/MOK.key"
cp "$MOK_CERT_PEM" "config/includes.chroot/root/.secureboot/MOK.crt"
cp "$MOK_KEY" "config/includes.chroot_before_packages/root/.secureboot/MOK.key"
cp "$MOK_CERT_PEM" "config/includes.chroot_before_packages/root/.secureboot/MOK.crt"

mkdir -p "config/includes.chroot/root" "config/includes.chroot_before_packages/root"
cp "$MOK_CERT_DER" "config/includes.chroot/root/ygg-local-mok.cer"
cp "$MOK_CERT_DER" "config/includes.chroot_before_packages/root/ygg-local-mok.cer"

# Stop Debian live-config from regenerating host keys every boot so SSH fingerprints stay stable.
mkdir -p "config/includes.chroot/etc/live/config.conf.d" \
         "config/includes.chroot_before_packages/etc/live/config.conf.d"
cat <<'EOF' > "config/includes.chroot/etc/live/config.conf.d/ssh-hostkeys.conf"
LIVE_GENERATE_HOSTKEYS=false
EOF
cp "config/includes.chroot/etc/live/config.conf.d/ssh-hostkeys.conf" \
   "config/includes.chroot_before_packages/etc/live/config.conf.d/ssh-hostkeys.conf"

mkdir -p "config/includes.chroot/etc/apt/apt.conf.d" "config/includes.chroot_before_packages/etc/apt/apt.conf.d"
if [[ -n "$APT_HTTP_PROXY" ]]; then
    {
        echo "Acquire::http::Proxy \"$APT_HTTP_PROXY\";"
        echo "Acquire::https::Proxy \"$APT_HTTPS_PROXY\";"
        if [[ -n "$APT_PROXY_BYPASS_HOST" ]]; then
            echo "Acquire::http::Proxy::$APT_PROXY_BYPASS_HOST \"DIRECT\";"
            echo "Acquire::https::Proxy::$APT_PROXY_BYPASS_HOST \"DIRECT\";"
        fi
    } > "config/includes.chroot/etc/apt/apt.conf.d/02proxy"
    cp "config/includes.chroot/etc/apt/apt.conf.d/02proxy" \
       "config/includes.chroot_before_packages/etc/apt/apt.conf.d/02proxy"
else
    cat <<'EOF' > "config/includes.chroot/etc/apt/apt.conf.d/02proxy"
// Optional APT proxy override.
// Example:
// Acquire::http::Proxy "http://apt-proxy.local:3142";
// Acquire::https::Proxy "http://apt-proxy.local:3142";
EOF
    cp "config/includes.chroot/etc/apt/apt.conf.d/02proxy" \
       "config/includes.chroot_before_packages/etc/apt/apt.conf.d/02proxy"
fi

if [[ -f "$APT_CACHE_TUNING_CONF" ]]; then
    cp "$APT_CACHE_TUNING_CONF" "config/includes.chroot/etc/apt/apt.conf.d/03cache-tuning"
    cp "$APT_CACHE_TUNING_CONF" "config/includes.chroot_before_packages/etc/apt/apt.conf.d/03cache-tuning"
else
    echo "WARN: $APT_CACHE_TUNING_CONF not found; apt cache tuning defaults will be skipped."
fi

cat <<'EOF' > "config/includes.chroot/etc/apt/apt.conf.d/99ygg-zfs-dkms.conf"
DPkg::Pre-Install-Pkgs::=/opt/ygg/patch-zfs-dkms.sh;
EOF
cp "config/includes.chroot/etc/apt/apt.conf.d/99ygg-zfs-dkms.conf" \
   "config/includes.chroot_before_packages/etc/apt/apt.conf.d/99ygg-zfs-dkms.conf"

# 3. Copy authorized_keys for root user if present
AUTHORIZED_KEYS_SOURCE="${YGG_SSH_AUTHORIZED_KEYS_FILE:-$ssh_host_keys_dir/authorized_keys}"
AUTHORIZED_KEYS_TARGET_DIR="config/includes.chroot/root/.ssh"
YGG_EMBED_SSH_KEYS="${YGG_EMBED_SSH_KEYS:-true}"

if [[ "$YGG_EMBED_SSH_KEYS" == "true" ]] && [ -f "$AUTHORIZED_KEYS_SOURCE" ]; then
    echo "Copying authorized_keys for root from $AUTHORIZED_KEYS_SOURCE..."
    mkdir -p "$AUTHORIZED_KEYS_TARGET_DIR"
    chmod 700 "$AUTHORIZED_KEYS_TARGET_DIR"
    cp "$AUTHORIZED_KEYS_SOURCE" "$AUTHORIZED_KEYS_TARGET_DIR/authorized_keys"
    chmod 600 "$AUTHORIZED_KEYS_TARGET_DIR/authorized_keys"
elif [[ "$YGG_EMBED_SSH_KEYS" != "true" ]]; then
    echo "Skipping authorized_keys embedding by configuration (YGG_EMBED_SSH_KEYS=$YGG_EMBED_SSH_KEYS)."
else
    echo "Warning: No authorized_keys found at $AUTHORIZED_KEYS_SOURCE. Root SSH key auth will be unavailable."
fi

# --- End of Enhanced SSH Server Configuration ---

# --- Bundle local helper binaries ---
mkdir -p "config/includes.chroot/usr/local/bin"
EDIT_BIN_PATH=$(command -v edit || true)
GIT_BIN_PATH=$(command -v git || true)
CARGO_BIN_PATH=$(command -v cargo || true)

if [[ -z "$EDIT_BIN_PATH" ]]; then
    echo "ERROR: 'edit' binary not found on host; install it before running mkconfig." >&2
    exit 1
fi

if [[ -z "$GIT_BIN_PATH" ]]; then
    echo "ERROR: 'git' binary not found on host; install it before running mkconfig." >&2
    exit 1
fi

if [[ -z "$CARGO_BIN_PATH" ]]; then
    echo "ERROR: 'cargo' binary not found on host; install Rust tooling before running mkconfig." >&2
    exit 1
fi

install -m 0755 "$EDIT_BIN_PATH" "config/includes.chroot/usr/local/bin/edit"

RUNTIME_CACHE_DIR="$PWD/runtime-cache/$BUILD_DAY"
RUNTIME_CARGO_TARGET_DIR="$PWD/runtime-target/$BUILD_DAY"
mkdir -p "$RUNTIME_CACHE_DIR" "$RUNTIME_CARGO_TARGET_DIR"

if [[ -s "$RUNTIME_CACHE_DIR/codex" && -s "$RUNTIME_CACHE_DIR/codex-litellm" && -s "$RUNTIME_CACHE_DIR/codex-session-tui" ]]; then
    echo "Using cached runtime binaries for day $BUILD_DAY."
else
    RUNTIME_BUILD_ROOT=$(mktemp -d)
    CODEx_SRC_DIR="$RUNTIME_BUILD_ROOT/codex"
    CODEX_LITELLM_SRC_DIR="$RUNTIME_BUILD_ROOT/codex-litellm"
    CODEX_SESSION_TUI_SRC_DIR="$RUNTIME_BUILD_ROOT/codex-session-tui"

    echo "Building release runtime: openai/codex (codex-rs/codex)..."
    "$GIT_BIN_PATH" clone --depth 1 https://github.com/openai/codex "$CODEx_SRC_DIR"
    (
        cd "$CODEx_SRC_DIR/codex-rs"
        CARGO_TARGET_DIR="$RUNTIME_CARGO_TARGET_DIR/codex-rs" \
        CARGO_PROFILE_RELEASE_LTO=off \
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
        "$CARGO_BIN_PATH" build --release --locked -p codex-cli --bin codex
    )
    install -m 0755 "$RUNTIME_CARGO_TARGET_DIR/codex-rs/release/codex" "$RUNTIME_CACHE_DIR/codex"

    echo "Building release runtime: avikalpa/codex-litellm..."
    "$GIT_BIN_PATH" clone --depth 1 https://github.com/avikalpa/codex-litellm "$CODEX_LITELLM_SRC_DIR"
    (
        cd "$CODEX_LITELLM_SRC_DIR"
        CARGO_PROFILE_RELEASE_LTO=off \
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
        TARGET=x86_64-unknown-linux-gnu SUFFIX=linux-x64 ./build.sh
    )
    install -m 0755 "$CODEX_LITELLM_SRC_DIR/dist/linux-x64/codex-litellm" "$RUNTIME_CACHE_DIR/codex-litellm"

    echo "Building release runtime: avikalpa/codex-session-tui..."
    "$GIT_BIN_PATH" clone --depth 1 https://github.com/avikalpa/codex-session-tui "$CODEX_SESSION_TUI_SRC_DIR"
    (
        cd "$CODEX_SESSION_TUI_SRC_DIR"
        CARGO_TARGET_DIR="$RUNTIME_CARGO_TARGET_DIR/codex-session-tui" \
        CARGO_PROFILE_RELEASE_LTO=off \
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
        "$CARGO_BIN_PATH" build --release --locked --bin codex-session-tui
    )
    install -m 0755 "$RUNTIME_CARGO_TARGET_DIR/codex-session-tui/release/codex-session-tui" "$RUNTIME_CACHE_DIR/codex-session-tui"

    rm -rf "$RUNTIME_BUILD_ROOT"
fi

install -m 0755 "$RUNTIME_CACHE_DIR/codex" "config/includes.chroot/usr/local/bin/codex"
install -m 0755 "$RUNTIME_CACHE_DIR/codex-litellm" "config/includes.chroot/usr/local/bin/codex-litellm"
install -m 0755 "$RUNTIME_CACHE_DIR/codex-session-tui" "config/includes.chroot/usr/local/bin/codex-session-tui"

# --- Network Configuration ---
LXC_PARENT_IF="${YGG_LXC_PARENT_IF:-eno1}"
MACVLAN_CIDR="${YGG_MACVLAN_CIDR:-10.10.0.250/24}"
MACVLAN_ROUTE="${YGG_MACVLAN_ROUTE:-10.10.0.0/24}"
HOST_NET_MODE="${YGG_NET_MODE:-dhcp}"
HOST_STATIC_IFACE="${YGG_STATIC_IFACE:-$LXC_PARENT_IF}"
HOST_STATIC_IP="${YGG_STATIC_IP:-}"
HOST_STATIC_GATEWAY="${YGG_STATIC_GATEWAY:-}"
HOST_STATIC_DNS="${YGG_STATIC_DNS:-}"

tee config/hooks/normal/9103-set-networking.hook.chroot <<EOF
#!/bin/bash

# LXC macvlan configuration
tee <<'EOL' /etc/lxc/default.conf
lxc.net.0.type = macvlan
lxc.net.0.macvlan.mode = bridge
lxc.net.0.link = ${LXC_PARENT_IF}
lxc.net.0.flags = up
lxc.net.0.name = eth0

lxc.apparmor.profile = generated
lxc.apparmor.allow_nesting = 1
EOL

systemctl disable lxc-net.service

# Host macvlan interface so the host can reach LXC macvlan guests
cat <<'EOL' >/usr/local/sbin/ygg-setup-mac0
#!/bin/bash
set -euo pipefail

PARENT_IF="${LXC_PARENT_IF}"
MACVLAN_IF="mac0"
MACVLAN_CIDR="${MACVLAN_CIDR}"
MACVLAN_ROUTE="${MACVLAN_ROUTE}"

if ! ip link show "\$PARENT_IF" >/dev/null 2>&1; then
    echo "[ygg-mac0] Parent interface \$PARENT_IF not found; skipping macvlan setup."
    exit 0
fi

ip link delete "\$MACVLAN_IF" 2>/dev/null || true
ip link add "\$MACVLAN_IF" link "\$PARENT_IF" type macvlan mode bridge
ip addr flush dev "\$MACVLAN_IF" || true
ip addr add "\$MACVLAN_CIDR" dev "\$MACVLAN_IF"
ip link set "\$MACVLAN_IF" up
ip route replace "\$MACVLAN_ROUTE" dev "\$MACVLAN_IF"
EOL
chmod +x /usr/local/sbin/ygg-setup-mac0

cat <<'EOL' >/etc/systemd/system/ygg-macvlan-mac0.service
[Unit]
Description=Yggdrasil macvlan interface for host access to LXC guests
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/ygg-setup-mac0
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOL

systemctl daemon-reload
systemctl enable ygg-macvlan-mac0.service
EOF

if [[ "$HOST_NET_MODE" == "static" && -n "$HOST_STATIC_IP" ]]; then
    tee -a config/hooks/normal/9103-set-networking.hook.chroot <<EOF
cat <<'EOL' >/usr/local/sbin/ygg-apply-static-ip
#!/bin/bash
set -euo pipefail

IFACE="${HOST_STATIC_IFACE}"
ADDR="${HOST_STATIC_IP}"
GW="${HOST_STATIC_GATEWAY}"
DNS="${HOST_STATIC_DNS}"

if ! ip link show "\$IFACE" >/dev/null 2>&1; then
    echo "[ygg-net] interface \$IFACE not found; skipping static config"
    exit 0
fi

ip addr flush dev "\$IFACE" || true
ip addr add "\$ADDR" dev "\$IFACE"
ip link set "\$IFACE" up

if [[ -n "\$GW" ]]; then
    ip route replace default via "\$GW" dev "\$IFACE"
fi

if [[ -n "\$DNS" ]]; then
    install -d -m 0755 /etc/systemd/resolved.conf.d
    {
        echo "[Resolve]"
        echo "DNS=\$DNS"
    } >/etc/systemd/resolved.conf.d/ygg-static-dns.conf
    systemctl restart systemd-resolved.service || true
fi
EOL
chmod +x /usr/local/sbin/ygg-apply-static-ip

cat <<'EOL' >/etc/systemd/system/ygg-static-ip.service
[Unit]
Description=Apply static host networking for Yggdrasil
After=network.target
Wants=network.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/ygg-apply-static-ip
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOL

systemctl daemon-reload
systemctl enable ygg-static-ip.service
EOF
fi
chmod +777 config/hooks/normal/9103-set-networking.hook.chroot

# --- ZFS Configuration ---
tee config/hooks/normal/9102-set-zpool-import.hook.chroot <<'EOF'
#!/bin/bash

mkdir -p /opt/ygg

tee <<'EOL' /opt/ygg/import-zpool-at-boot
#!/bin/bash

# change to maybe 'zones' or 'zroot'
POOL_NAME="zroot"

# Check if the named pool exists
if ! zpool list "$POOL_NAME" >/dev/null 2>&1; then
   # Try importing without force
   zpool import "$POOL_NAME" || {
       echo "Pool $POOL_NAME not found or cannot be imported"
       exit 0
   }
fi

# Set mountpoint only if pool is imported
if zpool list "$POOL_NAME" >/dev/null 2>&1; then
   current_mount=$(zfs get -H -o value mountpoint "$POOL_NAME" 2>/dev/null || echo "")
   if [ "$current_mount" != "/$POOL_NAME" ]; then
       zfs set mountpoint="/$POOL_NAME" "$POOL_NAME"
   fi

   # Handle /var dataset if it exists
   if zfs list -H -o name | grep -q "^$POOL_NAME/var$"; then
       current_var_mount=$(zfs get -H -o value mountpoint "$POOL_NAME/var" 2>/dev/null || echo "")
       if [ "$current_var_mount" != "/var" ]; then
           zfs set mountpoint=/var "$POOL_NAME/var"
       fi
   fi
fi
EOL
chmod +777 /opt/ygg/import-zpool-at-boot

# --- ZFS Service Configuration ---
tee <<'EOL' /etc/systemd/system/ygg-import-zpool-at-boot.service
[Unit]
Description=Import the main zfs zpool after boot

[Service]
Type=oneshot
ExecStart=/opt/ygg/import-zpool-at-boot
RemainAfterExit=yes
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
EOL

systemctl daemon-reload
systemctl enable ygg-import-zpool-at-boot.service
EOF
chmod +777 config/hooks/normal/9102-set-zpool-import.hook.chroot

# --- LXC path Configuration ---
tee config/hooks/normal/9104-set-lxc-path.hook.chroot <<'EOF'
#!/bin/bash

tee <<'EOL' /etc/lxc/lxc.conf
lxc.lxcpath=/zroot/lxc
EOL

EOF
chmod +777 config/hooks/normal/9104-set-lxc-path.hook.chroot

tee config/hooks/normal/9105-set-lxc-autostart.hook.chroot <<'EOF'
#!/bin/bash

tee <<'EOL' /etc/systemd/system/ygg-lxc-autostart.service
[Unit]
Description=Yggdrasil LXC Autostart
Documentation=man:lxc-autostart(1)

# CAVEAT 1: This is the most important part for you.
# It ensures this service runs only AFTER your ZFS pool is imported AND its filesystems are mounted.
# If the ZFS pool import fails, this service will not be started.
Requires=ygg-import-zpool-at-boot.service zfs-mount.service

# Pull in networking and LXC early but don't block on the slow network-online target.
Wants=lxc.service network-online.target
After=ygg-import-zpool-at-boot.service zfs-mount.service lxc.service network.target

[Service]
Type=oneshot
RemainAfterExit=yes

# The main command to start all containers with lxc.start.auto = 1
ExecStart=/usr/bin/lxc-autostart

# CAVEAT 2: Define a clean shutdown command.
# This will be executed on host shutdown/reboot.
# -s tells it to honor stop order and delays.
# -a tells it to stop all autostarted containers.
# -t tells it to wait before hard-stopping.
ExecStop=/usr/bin/lxc-autostart -s -a -t 30

# CAVEAT 3: Disable timeout.
# Starting many containers can take a long time.
TimeoutSec=0

[Install]
WantedBy=multi-user.target
EOL

systemctl daemon-reload
systemctl enable ygg-lxc-autostart.service
EOF
chmod +777 config/hooks/normal/9105-set-lxc-autostart.hook.chroot

# --- Ensure Infisical and dependent containers are ready ---
tee config/hooks/normal/9107-ensure-infisical.hook.chroot <<'EOF'
#!/bin/bash

tee <<'EOL' /usr/local/sbin/ygg-ensure-infisical
#!/bin/bash
set -euo pipefail

log() {
    echo "[ygg-infisical] $*"
}

container_exists() {
    local name="$1"
    lxc-ls -1 2>/dev/null | grep -qx "$name"
}

container_state() {
    lxc-info -n "$1" -sH 2>/dev/null || echo "UNKNOWN"
}

ensure_running() {
    local name="$1"
    if ! container_exists "$name"; then
        log "container $name not found; skipping"
        return 1
    fi
    local state
    state="$(container_state "$name")"
    if [ "$state" != "RUNNING" ]; then
        log "starting container $name (state: $state)"
        lxc-start -n "$name" -d
        sleep 2
    fi
}

wait_for_infisical() {
    local tries=30
    local delay=2
    local i
    for i in $(seq 1 "$tries"); do
        if lxc-attach -n infisical -- infisical-ctl status >/dev/null 2>&1; then
            log "infisical reports ready"
            return 0
        fi
        sleep "$delay"
    done
    log "infisical not ready after $((tries * delay))s; continuing"
    return 1
}

main() {
    if ! container_exists "infisical"; then
        log "infisical container missing; nothing to do"
        exit 0
    fi

    ensure_running "infisical" || true

    if ! lxc-attach -n infisical -- infisical-ctl start >/dev/null 2>&1; then
        log "infisical-ctl start failed (may already be running)"
    fi

    wait_for_infisical || true

    if container_exists "owncloud"; then
        ensure_running "owncloud" || true
        if lxc-attach -n owncloud -- test -e /root/ygg_ocis_full/update-stack.sh >/dev/null 2>&1; then
            log "running owncloud update-stack"
            lxc-attach -n owncloud -- /bin/sh -lc "/root/ygg_ocis_full/update-stack.sh" || \
                log "owncloud update-stack failed"
        else
            log "owncloud update-stack not found; skipping"
        fi
    fi
}

main "$@"
EOL

chmod +x /usr/local/sbin/ygg-ensure-infisical

tee <<'EOL' /etc/systemd/system/ygg-infisical-ensure.service
[Unit]
Description=Ensure Infisical and dependent containers are ready
After=ygg-lxc-autostart.service lxc.service network.target
Requires=ygg-lxc-autostart.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/ygg-ensure-infisical
TimeoutSec=0

[Install]
WantedBy=multi-user.target
EOL

systemctl daemon-reload
systemctl enable ygg-infisical-ensure.service
EOF
chmod +777 config/hooks/normal/9107-ensure-infisical.hook.chroot

# --- Setup for Unprivileged LXC ---
tee config/hooks/normal/9106-unprivileged-lxc-setup.hook.chroot <<'EOF'
#!/bin/bash

# Create the lxc-runtime user for running unprivileged containers
# This user will own the UID/GID maps for unprivileged containers
# --system: create a system user
# --no-create-home: no home directory needed for this service-like user
# --shell /usr/sbin/nologin: prevent direct login
if ! id -u lxc-runtime >/dev/null 2>&1; then
    useradd --system --shell /usr/sbin/nologin --no-create-home lxc-runtime
    echo "lxc-runtime user created."
else
    echo "lxc-runtime user already exists."
fi

# Configure subuid and subgid for the lxc-runtime user
# This grants lxc-runtime a range of 65536 UIDs and GIDs starting from 200000
# These ranges will be mapped to UIDs/GIDs inside unprivileged containers
# (e.g., container root (UID 0) becomes host UID 200000)
if ! grep -q "^lxc-runtime:" /etc/subuid; then
    echo "lxc-runtime:200000:65536" >> /etc/subuid
    echo "SubUID range added for lxc-runtime."
else
    echo "SubUID range for lxc-runtime already exists."
fi

if ! grep -q "^lxc-runtime:" /etc/subgid; then
    echo "lxc-runtime:200000:65536" >> /etc/subgid
    echo "SubGID range added for lxc-runtime."
else
    echo "SubGID range for lxc-runtime already exists."
fi

# Configure lxc-usernet to allow lxc-runtime to create network interfaces
# This allows the lxc-runtime user to create a certain number of veth pairs
# for its unprivileged containers.
# The 'lxcbr0' is a common default bridge name; even if you disable the
# lxc-net service and use macvlan, this permission is still useful for
# the veth interface type. You can adjust the count (e.g., 10) as needed.
# This file might not exist by default, so create it if necessary.
LXC_USERNET_CONF="/etc/lxc/lxc-usernet"
if ! grep -q "^lxc-runtime veth" "$LXC_USERNET_CONF" && \
   ! grep -q "^lxc-runtime veth" "${LXC_USERNET_CONF}.d/"* 2>/dev/null; then
    mkdir -p "$(dirname "$LXC_USERNET_CONF")"
    echo "lxc-runtime veth lxcbr0 10" >> "$LXC_USERNET_CONF"
    echo "lxc-usernet permissions added for lxc-runtime."
else
    echo "lxc-usernet permissions for lxc-runtime likely already exist."
fi

# Ensure LXC default configuration directory exists if not already
mkdir -p /etc/lxc

# Optional: Set up a default configuration for unprivileged containers
# This could specify the lxc-runtime user's idmap.
# However, this is often better handled in profiles or per-container configs.
# If you want a system-wide default for *all* unprivileged containers not
# otherwise specified:
#
# mkdir -p /etc/lxc/
# tee /etc/lxc/default.conf <<EODEF
# lxc.include = /usr/share/lxc/config/common.conf
# lxc.idmap = u 0 200000 65536
# lxc.idmap = g 0 200000 65536
# # Add other unprivileged defaults if desired
# EODEF
# echo "LXC default.conf updated for unprivileged containers."

echo "Unprivileged LXC setup for lxc-runtime completed."

EOF

# --- NVIDIA container toolkit ---
# References:
# - https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html

if [[ "$WITH_NVIDIA" == "true" ]]; then
   tee config/hooks/normal/9152-nvidia-container-toolkit.hook.chroot <<'EOF'
#!/bin/bash

# Add NVIDIA repository and GPG key
curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey | \
   gpg --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg

curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list | \
   sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' | \
   tee /etc/apt/sources.list.d/nvidia-container-toolkit.list

# Uncomment below to enable experimental packages
#sed -i -e '/experimental/ s/^#//g' /etc/apt/sources.list.d/nvidia-container-toolkit.list

# Install NVIDIA container toolkit
apt-get update
apt-get install -y nvidia-container-toolkit

# Basic NVIDIA configuration
tee /etc/nvidia-container-runtime/config.toml <<'EOL'
disable-require = false
#swarm-resource = "DOCKER_RESOURCE_GPU"

[nvidia-container-cli]
#root = "/run/nvidia/driver"
#path = "/usr/bin/nvidia-container-cli"
environment = []
#debug = "/var/log/nvidia-container-toolkit.log"
#ldcache = "/etc/ld.so.cache"
load-kmods = true
no-cgroups = false
#user = "root:video"
ldconfig = "@/sbin/ldconfig.real"

[nvidia-container-runtime]
#debug = "/var/log/nvidia-container-runtime.log"
EOL
EOF

   chmod +777 config/hooks/normal/9152-nvidia-container-toolkit.hook.chroot

tee config/hooks/normal/9151-nvidia-setup.hook.chroot <<'EOF'
#!/bin/bash

# Install NVIDIA drivers from Debian repositories.
# Avoid adding CUDA repositories here because their signing/key rotation
# can break unattended image builds.
apt-get update
apt-get install -y nvidia-driver

# Install keylase NVENC patch
mkdir -p /opt/ygg
cd /tmp
git clone https://github.com/keylase/nvidia-patch.git
cd nvidia-patch
# Store patch for first boot application
cp ./patch.sh /opt/ygg/
cp ./patch-fbc.sh /opt/ygg/

# LXC GPU Access Configuration
mkdir -p /etc/lxc/lxc.conf.d
tee /etc/lxc/lxc.conf.d/nvidia.conf <<'EOL'
# GPU device access
lxc.cgroup2.devices.allow = c 195:* rwm
lxc.cgroup2.devices.allow = c 196:* rwm
lxc.cgroup2.devices.allow = c 236:* rwm
lxc.cgroup2.devices.allow = c 238:* rwm
lxc.cgroup2.devices.allow = c 239:* rwm

# Mount NVIDIA driver components
lxc.mount.entry = /dev/nvidia0 dev/nvidia0 none bind,optional,create=file
lxc.mount.entry = /dev/nvidiactl dev/nvidiactl none bind,optional,create=file
lxc.mount.entry = /dev/nvidia-modeset dev/nvidia-modeset none bind,optional,create=file
lxc.mount.entry = /dev/nvidia-uvm dev/nvidia-uvm none bind,optional,create=file
lxc.mount.entry = /dev/nvidia-uvm-tools dev/nvidia-uvm-tools none bind,optional,create=file
EOL

# Create first-boot NVIDIA service
tee /etc/systemd/system/nvidia-firstboot.service <<'EOL'
[Unit]
Description=First boot NVIDIA setup
After=network.target

[Service]
Type=oneshot
ExecStart=/opt/ygg/nvidia-firstboot.sh
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOL

# Create first-boot script
tee /opt/ygg/nvidia-firstboot.sh <<'EOL'
#!/bin/bash

# Apply NVENC patch
cd /opt/ygg
./patch.sh || echo "Warning: NVENC patch application failed but continuing..."
./patch-fbc.sh || echo "Warning: FBC patch application failed but continuing..."

# Update NVIDIA container configuration
nvidia-ctk runtime configure --runtime=runc || echo "Warning: NVIDIA runtime configuration failed but continuing..."

# Remove self
systemctl disable nvidia-firstboot
exit 0
EOL
chmod +x /opt/ygg/nvidia-firstboot.sh

# Enable first-boot service
systemctl enable nvidia-firstboot

EOF

    chmod +777 config/hooks/normal/9151-nvidia-setup.hook.chroot

fi

# --- Secure Boot: sign DKMS modules with local MOK ---
tee config/hooks/normal/9198-sign-dkms-modules.hook.chroot <<'EOF'
#!/bin/bash
set -euo pipefail

KEY="/root/.secureboot/MOK.key"
CERT="/root/.secureboot/MOK.crt"
ENROLL_HELPER="/usr/local/sbin/ygg-enroll-mok"

if [[ ! -f "$KEY" || ! -f "$CERT" ]]; then
    echo "[ygg-mok] Signing material missing; skipping DKMS signing."
    exit 0
fi

find_sign_file() {
    local kernel_release="$1"
    local candidate

    candidate="/lib/modules/${kernel_release}/build/scripts/sign-file"
    if [[ -x "$candidate" ]]; then
        echo "$candidate"
        return 0
    fi

    candidate=$(find /usr/lib -type f -path '*/scripts/sign-file' 2>/dev/null | head -n1 || true)
    if [[ -n "$candidate" && -x "$candidate" ]]; then
        echo "$candidate"
        return 0
    fi

    return 1
}

signed_count=0
sign_module_file() {
    local sign_file="$1"
    local module="$2"
    local tmpdir tmp_ko

    case "$module" in
        *.ko)
            "$sign_file" sha256 "$KEY" "$CERT" "$module"
            ;;
        *.ko.zst)
            tmpdir=$(mktemp -d)
            tmp_ko="${tmpdir}/module.ko"
            zstd -d -q -c "$module" > "$tmp_ko"
            "$sign_file" sha256 "$KEY" "$CERT" "$tmp_ko"
            zstd -q -19 -c "$tmp_ko" > "$module"
            rm -rf "$tmpdir"
            ;;
        *.ko.xz)
            tmpdir=$(mktemp -d)
            tmp_ko="${tmpdir}/module.ko"
            xz -d -c "$module" > "$tmp_ko"
            "$sign_file" sha256 "$KEY" "$CERT" "$tmp_ko"
            xz -z -c "$tmp_ko" > "$module"
            rm -rf "$tmpdir"
            ;;
        *)
            return 1
            ;;
    esac
}

while IFS= read -r module; do
    kernel_release=$(echo "$module" | cut -d/ -f4)
    if ! sign_file=$(find_sign_file "$kernel_release"); then
        echo "[ygg-mok] sign-file not found for ${kernel_release}; skipping ${module}"
        continue
    fi
    sign_module_file "$sign_file" "$module" || continue
    signed_count=$((signed_count + 1))
done < <(find /lib/modules -type f \( -path '*/updates/dkms/*.ko' -o -path '*/updates/dkms/*.ko.zst' -o -path '*/updates/dkms/*.ko.xz' \) 2>/dev/null | sort)

if [[ "$signed_count" -gt 0 ]]; then
    echo "[ygg-mok] Signed ${signed_count} DKMS modules."
else
    echo "[ygg-mok] No DKMS modules found to sign."
fi

cat <<'EOL' > "$ENROLL_HELPER"
#!/bin/bash
set -euo pipefail

CERT="/root/ygg-local-mok.cer"
if [[ ! -f "$CERT" ]]; then
    echo "MOK certificate not found at $CERT"
    exit 1
fi

echo "Importing MOK cert: $CERT"
echo "Set a one-time enrollment password when prompted."
mokutil --import "$CERT"
echo "Reboot and complete enrollment in MokManager."
EOL
chmod 0755 "$ENROLL_HELPER"

rm -f /root/.secureboot/MOK.key /root/.secureboot/MOK.crt
rmdir /root/.secureboot 2>/dev/null || true
EOF
chmod +777 config/hooks/normal/9198-sign-dkms-modules.hook.chroot

# --- KDE installation ---
if [[ "$WITH_KDE" == "true" ]]; then
    tee config/hooks/normal/9200-kde.hook.chroot <<'EOF'
#!/bin/bash
set -euo pipefail
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get -y -o APT::Install-Recommends=true install \
  task-kde-desktop \
  sddm \
  plasma-discover \
  konsole \
  systemsettings
EOF

    chmod +777 config/hooks/normal/9200-kde.hook.chroot

# Ensure old browser hooks are removed if present from previous runs.
rm -f config/hooks/normal/9201-brave.hook.chroot config/hooks/normal/9201-chromium.hook.chroot

tee config/hooks/normal/9201-set-users.hook.chroot <<EOF
#!/bin/bash
set -euo pipefail

useradd -m -s /bin/bash pi
echo "pi:$PASSWD" | chpasswd
usermod -aG sudo pi
install -d -m 0750 /etc/sudoers.d
echo 'pi ALL=(ALL:ALL) NOPASSWD:ALL' > /etc/sudoers.d/90-pi-nopasswd
chmod 0440 /etc/sudoers.d/90-pi-nopasswd

# Vim configuration
echo 'set nocp' > /home/pi/.vimrc

cat <<'EOL' >> /home/pi/.bashrc
alias ls='ls --color=auto'
alias ll='ls -alhF'
EOL

EOF

    chmod +777 config/hooks/normal/9201-set-users.hook.chroot

fi

# =============================================================================
# Build Execution
# =============================================================================
# Notes:
# - Final step to build the ISO
# - Check build logs in case of failure
# - ISO will be in current directory named yggdrasil.iso

echo "Starting Yggdrasil ISO build..."
echo "Configuration:"
echo "  - Xen support: $WITH_XEN"
echo "  - KVM support: $WITH_KVM"
echo "  - NVIDIA support: $WITH_NVIDIA"
echo "  - KDE support: $WITH_KDE"

DEFAULT_APT_OPTIONS="${APT_OPTIONS:-"--yes -o Acquire::Retries=5"}"
APT_OPTIONS="${DEFAULT_APT_OPTIONS} -o DPkg::Pre-Install-Pkgs::=/opt/ygg/patch-zfs-dkms.sh"
export APT_OPTIONS
echo "Using APT options: $APT_OPTIONS"

env APT_OPTIONS="$APT_OPTIONS" lb build

prune_iso_builds

echo "Build complete. Check for ${IMAGE_NAME}-amd64.hybrid.iso in current directory."
