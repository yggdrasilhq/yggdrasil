#!/usr/bin/env sh
# Keep this script POSIX-sh compatible because the documented install flow is:
# curl -fsSL .../install.sh | sh
set -eu
(set -o pipefail) >/dev/null 2>&1 && set -o pipefail

REPO="${YGGDRASIL_MAKER_REPO:-yggdrasilhq/yggdrasil}"
LATEST_API="https://api.github.com/repos/${REPO}/releases/latest"
TMP_DIR="$(mktemp -d)"

log() {
  printf '[yggdrasil-maker-install] %s\n' "$*" >&2
}

fail() {
  log "$*"
  exit 1
}

cleanup() {
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

need_cmd curl
need_cmd tar
need_cmd sed
need_cmd uname

escape_desktop_value() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/ /\\ /g'
}

try_run() {
  if command -v "$1" >/dev/null 2>&1; then
    "$@" >/dev/null 2>&1 || true
  fi
}

refresh_kde_desktop_caches() {
  cache_home="${XDG_CACHE_HOME:-${HOME}/.cache}"
  rm -f "${cache_home}/icon-cache.kcache" 2>/dev/null || true
  if [ -d "${cache_home}" ]; then
    find "${cache_home}" -maxdepth 1 -type f -name 'ksycoca*' -exec rm -f {} \; 2>/dev/null || true
  fi
  try_run kbuildsycoca6 --noincremental
  try_run kbuildsycoca5 --noincremental
  try_run qdbus6 org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.refreshCurrentShell
}

install_linux_desktop_integration() {
  version_dir="$1"
  launcher_path="$2"
  install_root="$3"

  svg_src="${version_dir}/assets/brand/yggdrasil-maker-icon.svg"
  png_src="${version_dir}/assets/brand/yggdrasil-maker-icon-512.png"
  [ -f "${svg_src}" ] || return 0
  [ -f "${png_src}" ] || return 0

  data_home="${XDG_DATA_HOME:-${HOME}/.local/share}"
  apps_dir="${data_home}/applications"
  pixmaps_dir="${data_home}/pixmaps"
  icons_dir="${data_home}/icons/hicolor/512x512/apps"
  scalable_icons_dir="${data_home}/icons/hicolor/scalable/apps"
  direct_icons_dir="${install_root}/icons"

  mkdir -p "${apps_dir}" "${pixmaps_dir}" "${icons_dir}" "${scalable_icons_dir}" "${direct_icons_dir}"

  cp "${png_src}" "${icons_dir}/yggdrasil-maker.png"
  cp "${png_src}" "${icons_dir}/dev.yggdrasil.YggdrasilMaker.png"
  cp "${svg_src}" "${scalable_icons_dir}/yggdrasil-maker.svg"
  cp "${svg_src}" "${scalable_icons_dir}/dev.yggdrasil.YggdrasilMaker.svg"
  cp "${png_src}" "${pixmaps_dir}/yggdrasil-maker.png"
  cp "${svg_src}" "${pixmaps_dir}/yggdrasil-maker.svg"
  cp "${png_src}" "${direct_icons_dir}/yggdrasil-maker.png"
  cp "${svg_src}" "${direct_icons_dir}/yggdrasil-maker.svg"

  escaped_exec="$(escape_desktop_value "${launcher_path}")"
  escaped_icon="$(escape_desktop_value "${direct_icons_dir}/yggdrasil-maker.svg")"
  startup_wm_class="dev.yggdrasil.YggdrasilMaker"

  cat > "${apps_dir}/dev.yggdrasil.YggdrasilMaker.desktop" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Yggdrasil Maker
Comment=GUI-first Debian live ISO build studio
Exec=${escaped_exec}
TryExec=${escaped_exec}
Icon=${escaped_icon}
Terminal=false
NoDisplay=true
Categories=Utility;System;Development;
StartupNotify=true
StartupWMClass=${startup_wm_class}
X-Desktop-File-Install-Version=0.27
EOF

  cat > "${apps_dir}/yggdrasil-maker.desktop" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Yggdrasil Maker
Comment=GUI-first Debian live ISO build studio
Exec=${escaped_exec}
TryExec=${escaped_exec}
Icon=${escaped_icon}
Terminal=false
NoDisplay=false
Categories=Utility;System;Development;
StartupNotify=true
StartupWMClass=${startup_wm_class}
X-Desktop-File-Install-Version=0.27
EOF

  try_run update-desktop-database "${apps_dir}"
  try_run gtk-update-icon-cache -f -t "${data_home}/icons/hicolor"
  try_run xdg-icon-resource forceupdate
  try_run xdg-desktop-menu forceupdate
  refresh_kde_desktop_caches

  log "desktop integration refreshed in ${apps_dir}"
}

os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64|Linux:amd64) target_label="linux-x86_64" ;;
  Linux:aarch64|Linux:arm64) target_label="linux-aarch64" ;;
  Darwin:x86_64) target_label="macos-x86_64" ;;
  Darwin:arm64|Darwin:aarch64) target_label="macos-aarch64" ;;
  *) fail "unsupported platform: ${os} ${arch}" ;;
esac

case "${os}" in
  Linux)
    install_root="${YGGDRASIL_MAKER_INSTALL_ROOT:-${HOME}/.local/share/yggdrasil-maker/direct}"
    ;;
  Darwin)
    install_root="${YGGDRASIL_MAKER_INSTALL_ROOT:-${HOME}/Library/Application Support/yggdrasil-maker/direct}"
    ;;
  *)
    fail "unsupported operating system: ${os}"
    ;;
esac

log "checking latest release for ${target_label}"
release_json="$(curl -fsSL "${LATEST_API}")"
release_tag="$(printf '%s' "${release_json}" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
release_version="$(printf '%s' "${release_tag}" | sed 's/^v//')"
[ -n "${release_tag}" ] || fail "failed to resolve latest release tag"
[ -n "${release_version}" ] || fail "failed to resolve latest release version"

archive_url="https://github.com/${REPO}/releases/download/${release_tag}/yggdrasil-maker-${target_label}.tar.gz"
checksum_url="${archive_url}.sha256"
archive_path="${TMP_DIR}/yggdrasil-maker.tar.gz"
checksum_path="${TMP_DIR}/yggdrasil-maker.tar.gz.sha256"
state_path="${install_root}/install-state.json"

current_version=""
if [ -f "${state_path}" ]; then
  current_version="$(
    sed -n 's/.*"active_version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "${state_path}" | head -n1
  )"
fi

if [ -n "${current_version}" ] && [ "${current_version}" = "${release_version}" ]; then
  log "yggdrasil-maker ${release_version} is already installed"
  exit 0
fi

if [ -n "${current_version}" ]; then
  log "updating yggdrasil-maker ${current_version} -> ${release_version}"
else
  log "installing yggdrasil-maker ${release_version}"
fi

curl -fL "${archive_url}" -o "${archive_path}"
curl -fL "${checksum_url}" -o "${checksum_path}"

expected="$(awk '{print $1}' "${checksum_path}")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${archive_path}" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "${archive_path}" | awk '{print $1}')"
fi
[ "${expected}" = "${actual}" ] || fail "checksum verification failed"

version_dir="${install_root}/versions/${release_version}"
mkdir -p "${version_dir}"
tar -xzf "${archive_path}" -C "${version_dir}"

installed_binary="${version_dir}/yggdrasil-maker"
[ -x "${installed_binary}" ] || fail "archive did not contain yggdrasil-maker"

mkdir -p "${HOME}/.local/bin"
launcher_path="${HOME}/.local/bin/yggdrasil-maker"
cat > "${launcher_path}" <<EOF
#!/usr/bin/env sh
set -eu
ROOT='${install_root}'
STATE="\$ROOT/install-state.json"
target=""
if [ -f "\$STATE" ]; then
  target="\$(sed -n 's/.*"active_executable"[[:space:]]*:[[:space:]]*"\\([^"]*\\)".*/\\1/p' "\$STATE" | head -n1)"
fi
if [ -z "\$target" ] || [ ! -x "\$target" ]; then
  target='${installed_binary}'
fi
exec "\$target" "\$@"
EOF
chmod 0755 "${launcher_path}" || true

cat > "${state_path}" <<EOF
{
  "active_version": "${release_version}",
  "active_executable": "${installed_binary}"
}
EOF

if [ "${os}" = "Linux" ]; then
  install_linux_desktop_integration "${version_dir}" "${launcher_path}" "${install_root}"
fi

log "installed ${installed_binary}"
log "launcher ${launcher_path}"
