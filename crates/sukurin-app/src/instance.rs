//! One Sukurin per session. On Linux and macOS a later launch hands its
//! command line to the running one, so `sukurin --capture area` can sit behind
//! a desktop shortcut where global hotkeys are unavailable (GNOME on Wayland).

use crate::app::Command;
use crate::overlay::Mode;

/// The requests a command line names, in order. Unknown arguments are ignored.
pub fn requests(args: impl IntoIterator<Item = String>) -> Vec<Command> {
    let mut out = Vec::new();
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--library" => out.push(Command::OpenLibrary),
            "--onboarding" => out.push(Command::OpenOnboarding),
            "--settings" => out.push(Command::OpenSettings),
            "--capture" => {
                let mode = match args.next_if(|a| !a.starts_with("--")).as_deref() {
                    None | Some("area") => Some(Mode::Area),
                    Some("window") => Some(Mode::Window),
                    Some("screen") => Some(Mode::Screen),
                    Some("repeat") => Some(Mode::Repeat),
                    Some("text") => Some(Mode::Text),
                    Some(other) => {
                        log::warn!("unknown capture mode {other:?}; expected area, window, screen, repeat or text");
                        None
                    }
                };
                out.extend(mode.map(Command::Capture));
            }
            _ => {}
        }
    }
    out
}

pub enum Claim {
    /// This process is the instance; serve later launches with `serve`.
    First(Listener),
    /// Another instance is running and has been handed this command line.
    Forwarded,
}

#[cfg(unix)]
pub type Listener = Option<std::os::unix::net::UnixListener>;
#[cfg(not(unix))]
pub type Listener = ();

#[cfg(unix)]
fn socket_path() -> std::path::PathBuf {
    dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join("sukurin.sock")
}

#[cfg(unix)]
pub fn claim() -> Claim {
    use std::io::Write;
    use std::os::unix::net::{UnixListener, UnixStream};
    let path = socket_path();
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if let Err(e) = stream.write_all(args.join("\n").as_bytes()) {
            log::warn!("could not hand the request to the running Sukurin: {e}");
        }
        return Claim::Forwarded;
    }
    // Nothing answered, so any socket file left is from an instance that died.
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(listener) => Claim::First(Some(listener)),
        Err(e) => {
            log::warn!("{}: {e}; later launches will start their own instance", path.display());
            Claim::First(None)
        }
    }
}

/// Feeds each later launch's requests to the app.
#[cfg(unix)]
pub fn serve(listener: Listener, tx: smol::channel::Sender<Command>) {
    use std::io::Read;
    let Some(listener) = listener else { return };
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut text = String::new();
                if stream.read_to_string(&mut text).is_ok() {
                    for cmd in requests(text.lines().map(str::to_owned)) {
                        let _ = tx.try_send(cmd);
                    }
                }
            }
        })
        .expect("spawn the instance listener");
}

#[cfg(windows)]
pub fn claim() -> Claim {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;
    unsafe {
        let handle = CreateMutexW(None, true, w!("Local\\SukurinSingleInstance"));
        let first = handle.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS;
        // The mutex must outlive the process, so its handle is leaked on purpose.
        std::mem::forget(handle);
        if first { Claim::First(()) } else { Claim::Forwarded }
    }
}

#[cfg(windows)]
pub fn serve(_: Listener, _: smol::channel::Sender<Command>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Vec<Command> {
        requests(line.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn capture_names_its_mode_and_defaults_to_area() {
        assert!(matches!(parse("--capture window")[..], [Command::Capture(Mode::Window)]));
        assert!(matches!(parse("--capture")[..], [Command::Capture(Mode::Area)]));
        assert!(matches!(parse("--capture --library")[..], [Command::Capture(Mode::Area), Command::OpenLibrary]));
    }

    #[test]
    fn unknown_modes_and_arguments_request_nothing() {
        assert!(parse("--capture sideways").is_empty());
        assert!(parse("--verbose stray").is_empty());
    }
}
