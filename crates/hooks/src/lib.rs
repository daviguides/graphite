//! Claude Code hooks: keep the agent's grep/cat/find habits, let Graphite answer them.
//!
//! - `pre` (PreToolUse, Bash): if every segment of the command is read-only and at least one is a
//!   search/read/list Graphite can enrich, rewrite it to `graphite-hook run -- '<original>'`
//!   through `updatedInput` (the same mechanism RTK uses). No permission decision is emitted, so
//!   the normal permission flow still applies to the rewritten command.
//! - `run`: executes the command segment by segment. Search segments are answered by the daemon's
//!   embedded search + graph judgment; reads get a graph header before the real output; listings a
//!   summary after it. Any daemon error or timeout runs the original segment instead (fail open).
//! - `post` (PostToolUse): Read/Grep native tools get graph context appended; edits nudge the index.

pub mod exec;
pub mod install;
pub mod log;
pub mod parse;
pub mod post;
pub mod pre;
pub mod shell;

use std::os::unix::net::UnixStream;
use std::time::Duration;

use graphite_daemon::{client, Op, RepoPaths, Response};

/// Budget for the hook's own decision (socket connect).
pub const DECIDE_TIMEOUT: Duration = Duration::from_millis(300);
/// Budget for a daemon answer before the original command runs instead.
pub const ANSWER_TIMEOUT: Duration = Duration::from_millis(1500);

/// Connect to a running daemon only; hooks never start one.
pub fn connect(paths: &RepoPaths, timeout: Duration) -> Option<UnixStream> {
    let s = client::connect(paths)?;
    s.set_read_timeout(Some(timeout)).ok()?;
    s.set_write_timeout(Some(timeout)).ok()?;
    Some(s)
}

pub fn ask(paths: &RepoPaths, op: Op, timeout: Duration) -> Result<Response, String> {
    let mut s = connect(paths, timeout).ok_or("daemon not running")?;
    let r = client::request_on(&mut s, op).map_err(|e| e.to_string())?;
    if r.ok {
        Ok(r)
    } else {
        Err(r.error.unwrap_or_else(|| "daemon error".into()))
    }
}
