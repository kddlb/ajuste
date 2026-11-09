//! One ajuste per user session. The first instance listens on a Unix socket in
//! `$XDG_RUNTIME_DIR`; a later launch (e.g. the app-menu entry while the autostarted tray
//! instance runs) asks it to show its window and exits, instead of starting a second worker,
//! tray icon and notifier on the same device.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use crate::tray::AppMsg;

pub struct Instance {
    listener: Option<UnixListener>,
}

fn socket_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(dir).join("ajuste.sock"))
}

/// Become the running instance, or hand off to the existing one and return `None`.
/// `hidden` launches (autostart) don't raise an existing instance's window.
pub fn claim(hidden: bool) -> Option<Instance> {
    let Some(path) = socket_path() else {
        log::warn!("XDG_RUNTIME_DIR unset; not enforcing a single instance");
        return Some(Instance { listener: None });
    };
    if let Ok(mut s) = UnixStream::connect(&path) {
        let msg: &[u8] = if hidden { b"ping\n" } else { b"open\n" };
        if s.write_all(msg).is_ok() {
            log::info!("ajuste is already running; handing off");
            return None;
        }
    }
    // Nobody answered: any socket file left over is stale.
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(l) => Some(Instance { listener: Some(l) }),
        Err(e) => {
            log::warn!("could not bind {}: {e}", path.display());
            Some(Instance { listener: None })
        }
    }
}

impl Instance {
    /// Forward "open" requests from later launches as `AppMsg::OpenWindow`.
    pub fn listen(self, tx: async_channel::Sender<AppMsg>) {
        let Some(listener) = self.listener else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("ajuste-instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let mut line = String::new();
                    if BufReader::new(stream).read_line(&mut line).is_ok()
                        && line.trim() == "open"
                        && tx.send_blocking(AppMsg::OpenWindow).is_err()
                    {
                        break;
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!("could not start instance listener: {e}");
        }
    }
}
