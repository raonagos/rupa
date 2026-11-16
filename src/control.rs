use std::io::Write;
use std::os::unix::net::UnixStream;

/// Default control socket. The runtime will make this configurable.
pub const DEFAULT_SOCKET: &str = "/run/rupa/rupa.sock";

pub fn is_running() -> bool {
    UnixStream::connect(DEFAULT_SOCKET).is_ok()
}

/// Best-effort reload notification after a database mutation.
///
/// If no server is running the change simply applies at the next `rupa up`.
pub fn notify_reload() {
    if let Ok(mut stream) = UnixStream::connect(DEFAULT_SOCKET) {
        let _ = stream.write_all(b"{\"op\":\"reload\"}\n");
    }
}
