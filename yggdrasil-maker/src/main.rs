//! `yggdrasil-maker` — the front door to building a Yggdrasil image.
//!
//! Run it inside a yggterm terminal and it takes over the viewport: it declares
//! a web surface on its own PTY over OSC 7717 and serves the UI from loopback.
//! Run it in a plain terminal and it prints the URL instead, because the app is
//! a normal local web app that happens to know how to ask for a viewport.
//!
//! The maker is a front door, never a proprietary format trap, and never
//! charged for. Everything it does, `./mkconfig.sh` also does from a shell, and
//! the plan flow exists to show you exactly which shell commands those are.

mod discover;
mod plan;
mod run;
mod server;
mod surface;

use std::net::TcpListener;
use std::path::PathBuf;

const USAGE: &str = "\
Usage: yggdrasil-maker [options]

Opens the Yggdrasil build front door in the yggterm viewport. Outside yggterm
it serves the same UI and prints its URL.

Options:
  --repo PATH   Yggdrasil checkout to drive. Default: search upward from the
                current directory.
  --port PORT   Bind a fixed loopback port. Default: let the kernel choose.
  --print-url   Print the URL and exit without declaring a surface.
  -h, --help    Show this help.
";

fn main() -> std::process::ExitCode {
    match real_main() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("[yggdrasil-maker] {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<(), String> {
    let mut repo: Option<PathBuf> = None;
    let mut port: u16 = 0;
    let mut print_url = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo" => {
                repo = Some(PathBuf::from(
                    args.next().ok_or("--repo needs a path")?,
                ));
            }
            "--port" => {
                port = args
                    .next()
                    .ok_or("--port needs a number")?
                    .parse()
                    .map_err(|_| "--port must be a number between 0 and 65535")?;
            }
            "--print-url" => print_url = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown option: {other}\n\n{USAGE}")),
        }
    }

    let root = match repo {
        Some(path) => {
            let path = path
                .canonicalize()
                .map_err(|err| format!("--repo {}: {err}", path.display()))?;
            discover::find_repo_root(&path).ok_or_else(|| {
                format!("{} is not a Yggdrasil checkout", path.display())
            })?
        }
        None => {
            let cwd = std::env::current_dir()
                .map_err(|err| format!("could not read the current directory: {err}"))?;
            discover::find_repo_root(&cwd).ok_or(
                "no Yggdrasil checkout here or above. Run this from inside the repository, \
                 or pass --repo PATH.",
            )?
        }
    };

    // Bind before announcing. Owning the listener across the handover is what
    // removes the classic bind-close-respawn race: the port we publish is the
    // port we are already listening on.
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|err| format!("could not bind 127.0.0.1:{port}: {err}"))?;
    let bound = listener
        .local_addr()
        .map_err(|err| format!("could not read the bound address: {err}"))?
        .port();
    // http:// is required — yggterm's surface gate rejects every other scheme.
    let url = format!("http://127.0.0.1:{bound}/");

    if let Some(dir) = surface::state_dir() {
        // Host-resident state, created eagerly so the first flow that wants to
        // write does not have to think about it.
        let _ = std::fs::create_dir_all(&dir);
    }
    surface::write_launcher_manifest();

    let runner = run::Runner::new(root.clone());
    let ui = server::Ui {
        root: root.clone(),
        runner,
    };
    std::thread::spawn(move || server::serve(listener, ui));

    if print_url {
        println!("{url}");
        return Ok(());
    }

    match surface::session_id() {
        Some(session) => {
            let title = format!(
                "Yggdrasil Maker — {}",
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.display().to_string())
            );
            // The URL goes to stderr so it stays visible in the scrollback
            // underneath the surface, and so a `--print-url`-style capture of
            // stdout is never polluted by it. stdout is the control channel.
            eprintln!("[yggdrasil-maker] {url}");
            eprintln!("[yggdrasil-maker] press Ctrl-C, or the surface's ✕, to close.");
            surface::Surface::new(session, &url, &title)
                .hold_until_signalled()
                .map_err(|err| format!("could not install signal handlers: {err}"))?;
            Ok(())
        }
        None => {
            // Degradation, not failure: the app is fully usable, it simply has
            // no viewport to ask for. An unknown OSC would be ignored by a
            // plain terminal anyway, but saying nothing would leave a user
            // wondering why no window appeared.
            eprintln!(
                "[yggdrasil-maker] not inside a yggterm session (no YGGTERM_SESSION_ID), \
                 so no viewport was requested."
            );
            eprintln!("[yggdrasil-maker] the UI is served at {url}");
            eprintln!("[yggdrasil-maker] press Ctrl-C to stop.");
            println!("{url}");
            park();
            Ok(())
        }
    }
}

/// Hold the process open for the plain-terminal case, where there is no
/// heartbeat to run. Ctrl-C ends it the ordinary way.
fn park() {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
