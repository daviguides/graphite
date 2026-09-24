//! Thin-client side: connect, send one request, start the daemon when absent.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::paths::RepoPaths;
use crate::protocol::{Op, Request, Response};
use crate::{DaemonError, Result};

pub fn connect(paths: &RepoPaths) -> Option<UnixStream> {
    UnixStream::connect(&paths.socket).ok()
}

/// Send one request and read one response line.
pub fn request_on(stream: &mut UnixStream, op: Op) -> Result<Response> {
    let mut line =
        serde_json::to_string(&Request { op }).map_err(|e| DaemonError::Protocol(e.to_string()))?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    serde_json::from_str(&buf).map_err(|e| DaemonError::Protocol(format!("{e}: {buf}")))
}

pub fn request(paths: &RepoPaths, op: Op) -> Result<Response> {
    let mut stream =
        connect(paths).ok_or_else(|| DaemonError::Protocol("daemon not running".into()))?;
    request_on(&mut stream, op)
}

/// Spawn `<exe> daemon run --repo <root>` detached from the caller's session.
pub fn spawn_daemon(paths: &RepoPaths, exe: &Path) -> Result<()> {
    std::fs::create_dir_all(&paths.dir)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)?;
    Command::new(exe)
        .args(["daemon", "run", "--repo"])
        .arg(&paths.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .process_group(0)
        .spawn()?;
    Ok(())
}

/// Connect, starting the daemon if needed and waiting up to `wait` for its initial index.
pub fn ensure_daemon(paths: &RepoPaths, exe: &Path, wait: Duration) -> Result<UnixStream> {
    if let Some(s) = connect(paths) {
        return Ok(s);
    }
    spawn_daemon(paths, exe)?;
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if let Some(s) = connect(paths) {
            return Ok(s);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(DaemonError::Protocol(format!(
        "daemon did not come up within {}s; see {}",
        wait.as_secs(),
        paths.log.display()
    )))
}
