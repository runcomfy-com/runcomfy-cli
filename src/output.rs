//! Output / progress / verbosity helpers.
//!
//! Centralizes:
//!   - `--output {pretty,json}` (machine-readable mode)
//!   - `-q`/`-v` verbosity
//!   - TTY detection on stderr (decides whether progress lines use emoji
//!     or plain `[tag]` prefixes)
//!   - `NO_COLOR` / `TERM=dumb` honor
//!
//! Initialized once from `main` via [`init`]; commands call [`progress`]
//! / [`verbose`] / [`detail`] instead of touching stderr directly.

use std::io::IsTerminal;
use std::sync::OnceLock;

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputMode {
    /// Human-friendly: pretty JSON to stdout, progress emoji on stderr
    Pretty,
    /// Machine-readable: compact JSON to stdout, stderr stays empty
    Json,
}

#[derive(Debug, Clone, Copy)]
pub struct OutputCtx {
    pub mode: OutputMode,
    pub verbose: u8,
    pub quiet: bool,
    pub use_emoji: bool,
}

static CTX: OnceLock<OutputCtx> = OnceLock::new();

pub fn init(mode: OutputMode, verbose: u8, quiet: bool) {
    let no_color = std::env::var_os("NO_COLOR").is_some()
        || std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false);
    let stderr_tty = std::io::stderr().is_terminal();
    let use_emoji = matches!(mode, OutputMode::Pretty) && !no_color && stderr_tty && !quiet;
    let _ = CTX.set(OutputCtx {
        mode,
        verbose,
        quiet,
        use_emoji,
    });
}

pub fn ctx() -> OutputCtx {
    CTX.get().copied().unwrap_or(OutputCtx {
        mode: OutputMode::Pretty,
        verbose: 0,
        quiet: false,
        use_emoji: false,
    })
}

pub fn is_json() -> bool {
    matches!(ctx().mode, OutputMode::Json)
}

/// User-visible progress line. In JSON or quiet mode, suppressed.
/// In pretty mode on a TTY, uses the emoji; otherwise prints `[tag]`.
pub fn progress(emoji: &str, plain_tag: &str, msg: impl AsRef<str>) {
    let c = ctx();
    if c.quiet || matches!(c.mode, OutputMode::Json) {
        return;
    }
    if c.use_emoji {
        eprintln!("{} {}", emoji, msg.as_ref());
    } else {
        eprintln!("[{}] {}", plain_tag, msg.as_ref());
    }
}

/// Indented detail line under a progress (e.g. "request_id: req_xxx").
/// Same suppression rules as `progress`.
pub fn detail(msg: impl AsRef<str>) {
    let c = ctx();
    if c.quiet || matches!(c.mode, OutputMode::Json) {
        return;
    }
    eprintln!("   {}", msg.as_ref());
}

/// `-v` verbose-only line. Suppressed unless verbose > 0.
pub fn verbose(msg: impl AsRef<str>) {
    let c = ctx();
    if c.verbose > 0 && !c.quiet {
        eprintln!("[v] {}", msg.as_ref());
    }
}

/// Print the final structured payload to stdout. In Pretty mode, indented
/// 2-space JSON; in Json mode, single-line compact JSON. Always to stdout.
pub fn payload(value: &serde_json::Value) -> anyhow::Result<()> {
    match ctx().mode {
        OutputMode::Pretty => println!("{}", serde_json::to_string_pretty(value)?),
        OutputMode::Json => println!("{}", serde_json::to_string(value)?),
    }
    Ok(())
}

/// Print an aligned, whitespace-separated table to stdout (pretty mode
/// only — JSON mode callers print the raw payload instead). The last
/// column is not padded, so lines carry no trailing whitespace.
pub fn table(headers: &[&str], rows: &[Vec<String>]) {
    let ncol = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(ncol) {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let render = |cells: &[&str]| -> String {
        let mut line = String::new();
        for (i, cell) in cells.iter().enumerate().take(ncol) {
            if i > 0 {
                line.push_str("  ");
            }
            if i == ncol - 1 {
                line.push_str(cell);
            } else {
                line.push_str(cell);
                for _ in cell.chars().count()..widths[i] {
                    line.push(' ');
                }
            }
        }
        line.trim_end().to_string()
    };
    println!("{}", render(headers));
    for row in rows {
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        println!("{}", render(&cells));
    }
}

/// Print aligned `key:  value` lines to stdout (pretty mode).
pub fn kv(pairs: &[(&str, String)]) {
    let width = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0) + 1;
    for (k, v) in pairs {
        let key = format!("{}:", k);
        println!("{:<w$} {}", key, v, w = width);
    }
}

/// Ask a yes/no question on stderr and read the answer from stdin.
/// Returns `Ok(None)` when stdin is not a terminal (nobody can answer),
/// so callers can distinguish "declined" from "non-interactive".
pub fn confirm(prompt: &str) -> anyhow::Result<Option<bool>> {
    use std::io::{BufRead, Write};
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    eprint!("{} [y/N] ", prompt);
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let answer = line.trim().to_ascii_lowercase();
    Ok(Some(answer == "y" || answer == "yes"))
}
