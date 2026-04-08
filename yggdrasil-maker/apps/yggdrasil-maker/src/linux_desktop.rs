use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use yggterm_core::{AppIconAssets, install_linux_icon_assets};

use crate::window_icon::{YGGDRASIL_MAKER_ICON_PNG_512, YGGDRASIL_MAKER_ICON_SVG};

pub const YGGDRASIL_MAKER_DESKTOP_APP_ID: &str = "dev.yggdrasil.YggdrasilMaker";
pub const YGGDRASIL_MAKER_WM_CLASS: &str = "yggdrasil-maker";

pub fn refresh_dev_desktop_integration() -> Result<()> {
    let data_home = data_home()?;
    let applications_dir = data_home.join("applications");
    let direct_assets_dir = data_home.join("yggdrasil-maker").join("icons");
    let icons_dir = data_home.join("icons").join("hicolor");

    fs::create_dir_all(&applications_dir)?;
    fs::create_dir_all(&direct_assets_dir)?;

    let _installed_icons = install_linux_icon_assets(
        &data_home,
        &direct_assets_dir,
        &[
            YGGDRASIL_MAKER_WM_CLASS,
            YGGDRASIL_MAKER_DESKTOP_APP_ID,
            "Yggdrasil-maker",
        ],
        AppIconAssets {
            svg_bytes: YGGDRASIL_MAKER_ICON_SVG,
            png_512_bytes: YGGDRASIL_MAKER_ICON_PNG_512,
        },
    )?;
    let direct_png_path = direct_assets_dir.join("yggdrasil-maker.png");
    let direct_svg_path = direct_assets_dir.join("yggdrasil-maker.svg");
    write_if_changed(&direct_png_path, YGGDRASIL_MAKER_ICON_PNG_512)?;
    write_if_changed(&direct_svg_path, YGGDRASIL_MAKER_ICON_SVG)?;

    let current_exe = std::env::current_exe().context("resolve current executable")?;
    let escaped_exec = escape_desktop_value(&current_exe);
    let escaped_icon = escape_desktop_value(&direct_svg_path);

    let hidden_desktop = applications_dir.join(format!("{YGGDRASIL_MAKER_DESKTOP_APP_ID}.desktop"));
    let visible_desktop = applications_dir.join("yggdrasil-maker.desktop");
    let lower_alias_desktop = applications_dir.join("yggdrasil-maker-wmclass.desktop");
    let title_alias_desktop = applications_dir.join("Yggdrasil-maker.desktop");
    let hidden_contents = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Yggdrasil Maker\nComment=GUI-first Debian live ISO build studio\nExec={exec}\nTryExec={exec}\nIcon={icon}\nTerminal=false\nNoDisplay=true\nCategories=Utility;System;Development;\nStartupNotify=true\nStartupWMClass={wm_class}\nX-GNOME-WMClass={wm_class}\nX-Desktop-File-Install-Version=0.27\n",
        exec = escaped_exec,
        icon = escaped_icon,
        wm_class = YGGDRASIL_MAKER_DESKTOP_APP_ID,
    );
    let visible_contents = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Yggdrasil Maker\nComment=GUI-first Debian live ISO build studio\nExec={exec}\nTryExec={exec}\nIcon={icon}\nTerminal=false\nNoDisplay=false\nCategories=Utility;System;Development;\nStartupNotify=true\nStartupWMClass={wm_class}\nX-GNOME-WMClass={wm_class}\nX-Desktop-File-Install-Version=0.27\n",
        exec = escaped_exec,
        icon = escaped_icon,
        wm_class = "Yggdrasil-maker",
    );
    let lower_alias_contents = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Yggdrasil Maker\nComment=GUI-first Debian live ISO build studio\nExec={exec}\nTryExec={exec}\nIcon={icon}\nTerminal=false\nNoDisplay=true\nCategories=Utility;System;Development;\nStartupNotify=true\nStartupWMClass={wm_class}\nX-GNOME-WMClass={wm_class}\nX-Desktop-File-Install-Version=0.27\n",
        exec = escaped_exec,
        icon = escaped_icon,
        wm_class = YGGDRASIL_MAKER_WM_CLASS,
    );
    let title_alias_contents = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Yggdrasil Maker\nComment=GUI-first Debian live ISO build studio\nExec={exec}\nTryExec={exec}\nIcon={icon}\nTerminal=false\nNoDisplay=true\nCategories=Utility;System;Development;\nStartupNotify=true\nStartupWMClass={wm_class}\nX-GNOME-WMClass={wm_class}\nX-Desktop-File-Install-Version=0.27\n",
        exec = escaped_exec,
        icon = escaped_icon,
        wm_class = "Yggdrasil-maker",
    );
    write_if_changed(&hidden_desktop, hidden_contents.as_bytes())?;
    write_if_changed(&visible_desktop, visible_contents.as_bytes())?;
    write_if_changed(&lower_alias_desktop, lower_alias_contents.as_bytes())?;
    write_if_changed(&title_alias_desktop, title_alias_contents.as_bytes())?;

    try_run("update-desktop-database", &[applications_dir.as_os_str()]);
    try_run(
        "gtk-update-icon-cache",
        &["-f".as_ref(), "-t".as_ref(), icons_dir.as_os_str()],
    );
    try_run("xdg-icon-resource", &["forceupdate".as_ref()]);
    try_run("xdg-desktop-menu", &["forceupdate".as_ref()]);
    refresh_kde_desktop_caches();
    Ok(())
}

fn data_home() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var("HOME").context("resolve HOME for desktop integration")?;
    Ok(PathBuf::from(home).join(".local").join("share"))
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    let existing = fs::read(path).unwrap_or_default();
    if existing == contents {
        return Ok(());
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

fn escape_desktop_value(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace(' ', "\\ ")
}

fn try_run(program: &str, args: &[&std::ffi::OsStr]) {
    let _ = Command::new(program).args(args).status();
}

fn refresh_kde_desktop_caches() {
    let cache_home = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".cache")));
    if let Ok(cache_home) = cache_home {
        let _ = fs::remove_file(cache_home.join("icon-cache.kcache"));
        if let Ok(entries) = fs::read_dir(&cache_home) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(|name| name.starts_with("ksycoca"))
                    .unwrap_or(false)
                {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
    try_run("kbuildsycoca6", &["--noincremental".as_ref()]);
    try_run("kbuildsycoca5", &["--noincremental".as_ref()]);
    try_run(
        "qdbus6",
        &[
            "org.kde.plasmashell".as_ref(),
            "/PlasmaShell".as_ref(),
            "org.kde.PlasmaShell.refreshCurrentShell".as_ref(),
        ],
    );
}
