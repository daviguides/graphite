//! `graphite`: thin client of the per-repo daemon. `--json` is the agent surface.

mod render;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use graphite_daemon::{client, server, GraphQueries, Op, RepoPaths, Response};

/// How long a command waits for a freshly spawned daemon to finish its initial index.
const STARTUP_WAIT: Duration = Duration::from_secs(300);

#[derive(Parser)]
#[command(name = "graphite", version, about = "Code graph for AI agents")]
struct Cli {
    /// Repository root (default: nearest ancestor with .git).
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    /// Machine-readable output: the daemon's JSON response, one line.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start the daemon if needed and index the repo.
    Init,
    /// Daemon status: files, graph revision, pending events.
    Status,
    /// Find a symbol by qualified name, qualified suffix, bare name or id.
    Lookup { symbol: String },
    /// Transitive dependents of a symbol.
    Blast {
        symbol: String,
        #[command(flatten)]
        knobs: KnobArgs,
    },
    /// Symbols changed vs a git ref and everything that depends on them.
    DiffImpact {
        /// Git ref to diff against (default HEAD; working tree included).
        #[arg(long)]
        base: Option<String>,
        #[command(flatten)]
        knobs: KnobArgs,
    },
    /// Tell the daemon these paths changed (used by edit hooks).
    Nudge { paths: Vec<String> },
    /// Manage the daemon process.
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
    },
}

/// Answer-shaping options shared by traversal queries.
#[derive(clap::Args, Clone, Copy)]
struct KnobArgs {
    /// Max dependency depth (default 3).
    #[arg(long, short)]
    depth: Option<u32>,
    /// Token budget; detail degrades (full → summary → by file → by directory) to fit.
    #[arg(long)]
    budget: Option<usize>,
    /// Skip inline source for dependents.
    #[arg(long)]
    compact: bool,
}

#[derive(Subcommand)]
enum DaemonCmd {
    Start,
    Stop,
    Status,
    /// Run the daemon in the foreground (what `start` spawns).
    #[command(hide = true)]
    Run,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let start = cli
        .repo
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let paths = RepoPaths::discover(&start);
    match run(&cli, &paths) {
        Ok(code) => code,
        Err(e) => {
            if cli.json {
                println!("{}", serde_json::json!({"ok": false, "error": e}));
            } else {
                eprintln!("graphite: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, paths: &RepoPaths) -> Result<ExitCode, String> {
    let op = match &cli.cmd {
        Cmd::Daemon {
            cmd: DaemonCmd::Run,
        } => {
            server::run(paths.clone(), Box::new(GraphQueries)).map_err(|e| e.to_string())?;
            return Ok(ExitCode::SUCCESS);
        }
        Cmd::Daemon {
            cmd: DaemonCmd::Stop,
        } => {
            if client::connect(paths).is_none() {
                return emit(cli, None, "daemon not running");
            }
            Op::Shutdown
        }
        Cmd::Daemon {
            cmd: DaemonCmd::Status,
        } => {
            if client::connect(paths).is_none() {
                return emit(cli, None, "daemon not running");
            }
            Op::Status
        }
        Cmd::Daemon {
            cmd: DaemonCmd::Start,
        }
        | Cmd::Init
        | Cmd::Status => Op::Status,
        Cmd::Lookup { symbol } => Op::Lookup {
            symbol: symbol.clone(),
        },
        Cmd::Blast { symbol, knobs } => Op::Blast {
            symbol: symbol.clone(),
            depth: knobs.depth,
            budget: knobs.budget,
            compact: knobs.compact.then_some(true),
        },
        Cmd::DiffImpact { base, knobs } => Op::DiffImpact {
            base: base.clone(),
            depth: knobs.depth,
            budget: knobs.budget,
            compact: knobs.compact.then_some(true),
        },
        Cmd::Nudge { paths: p } => Op::Nudge { paths: p.clone() },
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut stream = client::ensure_daemon(paths, &exe, STARTUP_WAIT).map_err(|e| e.to_string())?;
    let resp = client::request_on(&mut stream, op).map_err(|e| e.to_string())?;
    emit(cli, Some(&resp), "")
}

fn emit(cli: &Cli, resp: Option<&Response>, note: &str) -> Result<ExitCode, String> {
    match resp {
        None if cli.json => println!(
            "{}",
            serde_json::json!({"ok": true, "running": false, "note": note})
        ),
        None => println!("{note}"),
        Some(r) if cli.json => println!("{}", serde_json::to_string(r).map_err(|e| e.to_string())?),
        Some(r) => render::human(&cli.cmd_name(), r),
    }
    Ok(match resp {
        Some(r) if !r.ok => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    })
}

impl Cli {
    fn cmd_name(&self) -> String {
        match &self.cmd {
            Cmd::Daemon {
                cmd: DaemonCmd::Stop,
            } => "stop",
            Cmd::Init | Cmd::Status | Cmd::Daemon { .. } => "status",
            Cmd::Lookup { .. } => "lookup",
            Cmd::Blast { .. } => "blast",
            Cmd::DiffImpact { .. } => "diff-impact",
            Cmd::Nudge { .. } => "nudge",
        }
        .to_string()
    }
}
