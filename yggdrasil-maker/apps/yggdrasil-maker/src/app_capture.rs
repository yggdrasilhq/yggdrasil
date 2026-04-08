use anyhow::{Context, Result, bail};
use dioxus::desktop::DesktopContext;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
use tao::platform::macos::WindowExtMacOS;
use yggterm_platform::capture_linux_x11_window_screenshot;
#[cfg(target_os = "macos")]
use yggterm_platform::{capture_macos_window_recording, capture_macos_window_screenshot};

pub fn focus_app_window(desktop: &DesktopContext) -> Result<Value> {
    desktop.set_visible(true);
    desktop.set_minimized(false);
    desktop.set_focus();
    Ok(json!({
        "focused_requested": true,
        "focused": desktop.is_focused(),
        "window": describe_window(desktop),
    }))
}

pub fn describe_window(desktop: &DesktopContext) -> Value {
    let inner = desktop.inner_size();
    let outer = desktop.outer_size();
    let position = desktop.outer_position().ok();
    json!({
        "title": desktop.title(),
        "visible": desktop.is_visible(),
        "focused": desktop.is_focused(),
        "maximized": desktop.is_maximized(),
        "decorated": desktop.is_decorated(),
        "display": std::env::var("DISPLAY").ok(),
        "wayland_display": std::env::var("WAYLAND_DISPLAY").ok(),
        "xdg_session_id": std::env::var("XDG_SESSION_ID").ok(),
        "xdg_runtime_dir": std::env::var("XDG_RUNTIME_DIR").ok(),
        "xauthority": std::env::var("XAUTHORITY").ok(),
        "inner_size": {
            "width": inner.width,
            "height": inner.height,
        },
        "outer_size": {
            "width": outer.width,
            "height": outer.height,
        },
        "outer_position": position.map(|position| {
            json!({
                "x": position.x,
                "y": position.y,
            })
        }),
    })
}

pub fn capture_visible_app_surface(
    desktop: &DesktopContext,
    output_path: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating screenshot dir {}", parent.display()))?;
    }
    platform_capture_visible_app_surface(desktop, output_path)?;
    let metadata = fs::metadata(output_path)
        .with_context(|| format!("reading screenshot metadata {}", output_path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        bail!("native screenshot capture produced no file output");
    }
    Ok(output_path.to_path_buf())
}

pub fn record_visible_app_surface(
    desktop: &DesktopContext,
    output_path: &Path,
    duration_secs: u64,
) -> Result<PathBuf> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating recording dir {}", parent.display()))?;
    }
    platform_record_visible_app_surface(desktop, output_path, duration_secs.max(1))?;
    let metadata = fs::metadata(output_path)
        .with_context(|| format!("reading recording metadata {}", output_path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        bail!("native screen recording produced no file output");
    }
    Ok(output_path.to_path_buf())
}

#[cfg(target_os = "linux")]
fn platform_capture_visible_app_surface(
    desktop: &DesktopContext,
    output_path: &Path,
) -> Result<()> {
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        if capture_linux_x11_window_screenshot(std::process::id(), output_path).is_ok() {
            return Ok(());
        }
        return capture_linux_root_crop(desktop, output_path);
    }
    let _ = desktop;
    bail!("native maker screenshot capture is only implemented for Linux/X11 right now")
}

#[cfg(target_os = "macos")]
fn platform_capture_visible_app_surface(
    desktop: &DesktopContext,
    output_path: &Path,
) -> Result<()> {
    capture_macos_window_screenshot(desktop.window.ns_window().cast(), output_path)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform_capture_visible_app_surface(
    _desktop: &DesktopContext,
    _output_path: &Path,
) -> Result<()> {
    bail!("native maker screenshot capture is not implemented for this platform yet")
}

#[cfg(target_os = "linux")]
fn platform_record_visible_app_surface(
    desktop: &DesktopContext,
    output_path: &Path,
    duration_secs: u64,
) -> Result<()> {
    let position = desktop
        .outer_position()
        .context("resolving window position for recording")?;
    let size = desktop.outer_size();
    let width = size.width.max(1);
    let height = size.height.max(1);
    let display = std::env::var("DISPLAY").context("DISPLAY is required for X11 capture")?;
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        bail!("native maker screen recording is only implemented for Linux/X11 right now");
    }
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-video_size",
            &format!("{width}x{height}"),
            "-framerate",
            "30",
            "-f",
            "x11grab",
            "-i",
            &format!("{display}+{},{}", position.x, position.y),
            "-t",
            &duration_secs.to_string(),
            output_path.to_string_lossy().as_ref(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("running ffmpeg for maker screen recording")?;
    if !status.success() {
        bail!("ffmpeg exited with status {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn capture_linux_root_crop(desktop: &DesktopContext, output_path: &Path) -> Result<()> {
    desktop.set_visible(true);
    desktop.set_minimized(false);
    desktop.set_focus();
    std::thread::sleep(std::time::Duration::from_millis(120));

    let position = desktop
        .outer_position()
        .context("resolving window position for screenshot crop")?;
    let size = desktop.outer_size();
    let width = size.width.max(1);
    let height = size.height.max(1);

    let status = Command::new("import")
        .args([
            "-window",
            "root",
            "-crop",
            &format!("{width}x{height}+{}+{}", position.x, position.y),
            output_path.to_string_lossy().as_ref(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("running ImageMagick import for screenshot crop fallback")?;
    if !status.success() {
        bail!("import exited with status {status}");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_record_visible_app_surface(
    desktop: &DesktopContext,
    output_path: &Path,
    duration_secs: u64,
) -> Result<()> {
    capture_macos_window_recording(
        desktop.window.ns_window().cast(),
        output_path,
        duration_secs,
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform_record_visible_app_surface(
    _desktop: &DesktopContext,
    _output_path: &Path,
    _duration_secs: u64,
) -> Result<()> {
    bail!("native maker screen recording is not implemented for this platform yet")
}
