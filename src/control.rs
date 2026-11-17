use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

/// Default control socket. The runtime makes it configurable only through the
/// filesystem layout (`/run/rupa`) for now.
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

pub fn request(op: &str) -> Result<Value> {
    request_to(Path::new(DEFAULT_SOCKET), op)
}

pub fn request_to(path: &Path, op: &str) -> Result<Value> {
    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("rupa server is not running ({})", path.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .context("failed to set the control socket timeout")?;

    let payload = json!({ "op": op });
    writeln!(stream, "{payload}").context("failed to send the control request")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("failed to read the control response")?;
    if line.trim().is_empty() {
        bail!("empty response from the rupa server");
    }
    serde_json::from_str(line.trim()).context("invalid JSON from the rupa server")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn request_reads_a_json_response() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rupa.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(request["op"], "ping");
            let mut stream = reader.into_inner();
            writeln!(stream, "{{\"ok\":true,\"pong\":true}}").unwrap();
        });

        let response = request_to(&path, "ping").unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["pong"], true);
        server.join().unwrap();
    }
}
