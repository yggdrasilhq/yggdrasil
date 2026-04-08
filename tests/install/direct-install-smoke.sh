#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

fail() {
  printf '[direct-install-smoke] %s\n' "$*" >&2
  exit 1
}

need_file() {
  [[ -f "$1" ]] || fail "missing file: $1"
}

need_dir() {
  [[ -d "$1" ]] || fail "missing dir: $1"
}

version="$(
  python3 - <<'PY' "$ROOT_DIR/yggdrasil-maker/Cargo.toml"
import pathlib
import sys
import tomllib

doc = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
print(doc["workspace"]["package"]["version"])
PY
)"

home_dir="$TMP_DIR/home"
release_dir="$TMP_DIR/release"
package_dir="$TMP_DIR/package"
mockbin_dir="$TMP_DIR/mockbin"
mkdir -p "$home_dir" "$release_dir" "$package_dir/assets/brand" "$mockbin_dir"

cat > "$package_dir/yggdrasil-maker" <<'EOF'
#!/usr/bin/env sh
set -eu
if [ "${1:-}" = "--version" ]; then
  printf 'yggdrasil-maker test-binary\n'
  exit 0
fi
printf 'yggdrasil-maker stub\n'
EOF
chmod 0755 "$package_dir/yggdrasil-maker"

cp "$ROOT_DIR/yggdrasil-maker/assets/brand/yggdrasil-maker-icon.svg" "$package_dir/assets/brand/"
cp "$ROOT_DIR/yggdrasil-maker/assets/brand/yggdrasil-maker-icon-512.png" "$package_dir/assets/brand/"

archive_path="$release_dir/yggdrasil-maker-linux-x86_64.tar.gz"
(
  cd "$package_dir"
  tar -czf "$archive_path" yggdrasil-maker assets
)

checksum_path="${archive_path}.sha256"
sha256sum "$archive_path" > "$checksum_path"

cat > "$release_dir/release.json" <<EOF
{
  "tag_name": "v${version}"
}
EOF

for cmd in update-desktop-database gtk-update-icon-cache xdg-icon-resource xdg-desktop-menu kbuildsycoca6 kbuildsycoca5 qdbus6; do
  cat > "$mockbin_dir/$cmd" <<'EOF'
#!/usr/bin/env sh
exit 0
EOF
  chmod 0755 "$mockbin_dir/$cmd"
done

PATH="$mockbin_dir:$PATH" \
HOME="$home_dir" \
XDG_DATA_HOME="$home_dir/.local/share" \
XDG_CACHE_HOME="$home_dir/.cache" \
YGGDRASIL_MAKER_INSTALL_ROOT="$home_dir/.local/share/yggdrasil-maker/direct" \
YGGDRASIL_MAKER_LATEST_API="file://$release_dir/release.json" \
YGGDRASIL_MAKER_ARCHIVE_URL="file://$archive_path" \
YGGDRASIL_MAKER_CHECKSUM_URL="file://$checksum_path" \
sh "$ROOT_DIR/scripts/install.sh"

launcher_path="$home_dir/.local/bin/yggdrasil-maker"
install_root="$home_dir/.local/share/yggdrasil-maker/direct"
version_dir="$install_root/versions/$version"
state_path="$install_root/install-state.json"
apps_dir="$home_dir/.local/share/applications"
icons_dir="$home_dir/.local/share/icons/hicolor"

need_file "$launcher_path"
need_dir "$version_dir"
need_file "$version_dir/yggdrasil-maker"
need_file "$version_dir/assets/brand/yggdrasil-maker-icon.svg"
need_file "$version_dir/assets/brand/yggdrasil-maker-icon-512.png"
need_file "$state_path"
need_file "$apps_dir/dev.yggdrasil.YggdrasilMaker.desktop"
need_file "$apps_dir/yggdrasil-maker.desktop"
need_file "$install_root/icons/yggdrasil-maker.svg"
need_file "$install_root/icons/yggdrasil-maker.png"
need_file "$icons_dir/512x512/apps/dev.yggdrasil.YggdrasilMaker.png"
need_file "$icons_dir/scalable/apps/dev.yggdrasil.YggdrasilMaker.svg"

rg -q "\"active_version\": \"$version\"" "$state_path" || fail "install state missing version"
rg -q "StartupWMClass=dev.yggdrasil.YggdrasilMaker" "$apps_dir/yggdrasil-maker.desktop" || fail "desktop file missing startup wm class"
rg -q "Icon=.*yggdrasil-maker\\.svg" "$apps_dir/yggdrasil-maker.desktop" || fail "desktop file missing icon path"
rg -q "Exec=.*\\.local/bin/yggdrasil-maker" "$apps_dir/yggdrasil-maker.desktop" || fail "desktop file missing launcher exec"

PATH="$mockbin_dir:$PATH" HOME="$home_dir" "$launcher_path" --version >/dev/null

printf '[direct-install-smoke] ok\n'
