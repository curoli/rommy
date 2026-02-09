use anyhow::{Context, Result};
use clap::{Parser, Subcommand, Args, ValueEnum};
use chrono::{DateTime, Utc};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::process::{Command, Stdio, Child};
use std::io::{self, Write, IsTerminal, Read};
use std::thread;

use crate::scratch::launch_editor_and_get_script;

mod outpath;
mod scratch;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ColorChoice { Auto, Always, Never }

const CYAN: &str   = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str  = "\x1b[0m";

#[derive(Args, Debug, Clone)]
pub struct RunConfig {
    /// Output file (optional; if omitted, Rommy chooses a time-based path)
    #[arg(long, value_name="FILE")]
    pub out: Option<PathBuf>,

    /// Working directory
    #[arg(long, value_name="DIR")]
    pub cwd: Option<PathBuf>,

    /// Provide KEY=VALUE environment pairs (repeatable)
    #[arg(long="env", value_name="KEY=VALUE")]
    pub envs: Vec<String>,

    /// Append instead of overwrite
    #[arg(long)]
    pub append: bool,

    /// Optional label to include in META
    #[arg(long)]
    pub label: Option<String>,

    /// Run given bash script file instead of a single command
    #[arg(long, value_name="SCRIPT.sh", conflicts_with="cmd")]
    pub script: Option<PathBuf>,

    /// Disable live streaming to terminal (default: streaming ON)
    #[arg(long="no-stream")]
    pub no_stream: bool,

    /// Command to run (after --). Example: rommy run -- cargo test
    #[arg(last = true)]
    pub cmd: Vec<String>,
    
    /// Color output: auto|always|never (default: auto)
    #[arg(long = "color", value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
}

#[derive(Parser, Debug)]
#[command(name="rommy")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Run {
        #[command(flatten)]
        run_config: RunConfig,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Run { run_config } => {
            run(run_config)
        }
    }
}

/// Führt den Child-Prozess aus.
/// - stream=true: stdout/stderr werden live ins Terminal gespiegelt UND gesammelt.
/// - stream=false: stdout/stderr werden vollständig gesammelt (keine Terminalausgabe).
///   Rückgabe: (stdout_bytes, stderr_bytes, exit_code)
fn spawn_and_stream(mut child: Child, stream: bool, colors: bool)
    -> anyhow::Result<(Vec<u8>, Vec<u8>, i32)>
{
    fn tee<R: Read + Send + 'static, W: Write + Send + 'static>(
        mut r: R,
        mut w: W,
        colorize_each_chunk: bool,
    ) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut all = Vec::new();
            loop {
                match r.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if colorize_each_chunk {
                            let _ = w.write_all(YELLOW.as_bytes());
                            let _ = w.write_all(&buf[..n]);
                            let _ = w.write_all(RESET.as_bytes());
                        } else {
                            let _ = w.write_all(&buf[..n]);
                        }
                        let _ = w.flush();
                        all.extend_from_slice(&buf[..n]); // capture bleibt uncolored
                    }
                    Err(_) => break,
                }
            }
            all
        })
    }

    if stream {
        // take() nur im Streaming-Zweig
        let out_r = child.stdout.take();
        let err_r = child.stderr.take();

        // stdout: niemals einfärben
        let h_out = out_r.map(|r| tee(r, io::stdout(), false));
        // stderr: pro Chunk einfärben (nur wenn colors=true)
        let h_err = err_r.map(|r| tee(r, io::stderr(), colors));

        let status = child.wait()?;
        let code = status.code().unwrap_or(-1);

        let stdout_bytes = h_out.map(|h| h.join().unwrap_or_default()).unwrap_or_default();
        let stderr_bytes = h_err.map(|h| h.join().unwrap_or_default()).unwrap_or_default();

        Ok((stdout_bytes, stderr_bytes, code))
    } else {
        // kein Streaming: keine take(); vollständiges Lesen
        let output = child.wait_with_output()?;
        let code = output.status.code().unwrap_or(-1);
        Ok((output.stdout, output.stderr, code))
    }
}

fn run(cfg: RunConfig) -> Result<()> {
    let stream = !cfg.no_stream;
    let colors = color_is_enabled(cfg.color);
    // Resolve CWD
    let cwd_path = cfg.cwd.unwrap_or(std::env::current_dir()?);

    let script = if cfg.script.is_some() {
        cfg.script
    } else if cfg.cmd.is_empty() {
        let script = launch_editor_and_get_script()?;
        Some(script)
    } else {
        None
    };
    
    // Build command invocation
    let (display_command, exec) = if let Some(script_path) = &script {
        let script_abs = fs::canonicalize(script_path)
            .with_context(|| format!("Cannot resolve script path: {}", script_path.display()))?;
        let script_text = fs::read_to_string(&script_abs)
            .with_context(|| format!("Cannot read script: {}", script_abs.display()))?;

        let display = format!(
            "#!/usr/bin/env bash\n{}\n",
            script_text
        );

        // Execute bash with -Eeuo pipefail for safety & clear failures
        let mut command = Command::new("bash");
        command
            .arg("-Eeuo")
            .arg("pipefail")
            .arg(&script_abs)
            .current_dir(&cwd_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        (RommyCommand::Script { path: script_abs, content: display }, command)
    } else {
        anyhow::ensure!(!cfg.cmd.is_empty(), "Provide either --script <file> or a command after --");
        let bash_line = shell_join(&cfg.cmd)?;
        let mut command = Command::new("bash");
        command
            .arg("-lc")
            .arg(&bash_line)
            .current_dir(&cwd_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        (RommyCommand::Line(bash_line), command)
    };

    // Apply envs
    let mut command = exec;
    for kv in cfg.envs {
        if let Some((k, v)) = kv.split_once('=') {
            command.env(k, v);
        } else {
            eprintln!("WARN: ignoring malformed --env '{}', expected KEY=VALUE", kv);
        }
    }

    // Collect metadata
    let rommy_version = env!("CARGO_PKG_VERSION").to_string();
    let user = whoami::username();
    let host = whoami::hostname();

    // ... du hast vorher schon `command` gebaut (bash -lc ... oder script), gut!
    
    // WICHTIG: Pipes setzen, sonst gibt es nichts zu lesen
    // WICHTIG: Pipes setzen – für den Streaming-Pfad nötig.
    // Für den no-stream-Pfad stört es nicht; wait_with_output() funktioniert weiterhin.
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    
    let start: DateTime<Utc> = Utc::now();
    let child = command.spawn().with_context(|| "Failed to spawn process")?;
    
    // stream = !no_stream (so wie du es aktuell ableitest)
    let (stdout_bytes, stderr_bytes, exit_code) = spawn_and_stream(child, stream, colors)
        .with_context(|| "stream/capture failed")?;
    
    let end: DateTime<Utc> = Utc::now();
    let duration_ms = (end - start).num_milliseconds();
    
    let status_str = if exit_code == 0 { "ok" } else { "error" };
    let stdout_txt = String::from_utf8_lossy(&stdout_bytes);
    let stderr_txt = String::from_utf8_lossy(&stderr_bytes);
    
    // Bestimme Ausgabedatei
    let out_path: PathBuf = if let Some(explicit) = cfg.out {
        explicit
    } else {
        // Display-String für COMMAND-Block vorbereiten (wie bisher)
        let display_for_token = match &display_command {
            RommyCommand::Script { .. } => "#!/usr/bin/env bash\n<script>".to_string(),
            RommyCommand::Line(line) => format!("$ {}", line),
        };
        outpath::resolve_auto_out_path(&display_for_token)
            .context("failed to resolve automatic output path")?
    };

    
    // Prepare writer
    if let Some(parent) = out_path.parent()
        && !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create directory {}", parent.display()))?;
        }
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .append(cfg.append)
        .truncate(!cfg.append)
        .open(&out_path)
        .with_context(|| format!("Cannot open {}", out_path.display()))?;

    // --- Write blocks ---
    // META
    writeln!(f, "<<<META>>>")?;
    writeln!(f, "rommy_version: {}", rommy_version)?;
    if let Some(label) = cfg.label { writeln!(f, "label: {}", label)?; }
    writeln!(f, "cwd: {}", fs::canonicalize(&cwd_path)?.display())?;
    if let Ok(user) = user {
        writeln!(f, "user: {}", user)?;        
    }
    if let Ok(host) = host {
        writeln!(f, "host: {}", host)?;        
    }
    match &display_command {
        RommyCommand::Script { path, .. } => {
            writeln!(f, "script_path: {}", path.display())?;
        }
        RommyCommand::Line(line) => {
            writeln!(f, "command_line: {}", line)?;
        }
    }
    writeln!(f, "start_ts: {}", start.to_rfc3339())?;
    writeln!(f, "end_ts: {}", end.to_rfc3339())?;
    writeln!(f, "duration_ms: {}", duration_ms)?;
    writeln!(f, "output_path: {}", out_path.display())?;
    writeln!(f, "status: {}", status_str)?;
    writeln!(f, "exit_code: {}", exit_code)?;
    writeln!(f, "<<<END>>>")?;

    // COMMAND
    writeln!(f, "<<<COMMAND>>>")?;
    match &display_command {
        RommyCommand::Script { content, .. } => {
            f.write_all(content.as_bytes())?;
        }
        RommyCommand::Line(line) => {
            writeln!(f, "$ {}", line)?;
        }
    }
    writeln!(f, "<<<END>>>")?;

    // STDOUT
    writeln!(f, "<<<STDOUT>>>")?;
    f.write_all(stdout_txt.as_bytes())?;
    writeln!(f, "<<<END>>>")?;
    
    writeln!(f, "<<<STDERR>>>")?;
    f.write_all(stderr_txt.as_bytes())?;
    writeln!(f, "<<<END>>>")?;
    rommy_note_cyan(colors, &format!("Wrote {}", out_path.display()));
    Ok(())
}

enum RommyCommand {
    Line(String),
    Script { path: PathBuf, content: String },
}

/// Join argv-like pieces into a bash-safe single line for display/execution with bash -lc.
/// Minimal approach: quote each arg safely.
fn shell_join<S: AsRef<OsStr>>(parts: &[S]) -> Result<String> {
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 { out.push(' '); }
        out.push_str(&shell_escape(p.as_ref()));
    }
    Ok(out)
}

fn shell_escape(s: &OsStr) -> String {
    // Simple POSIX single-quote escaping: ' -> '\'' and wrap in single quotes.
    let bytes = s.as_encoded_bytes();
    let mut escaped = String::from("'");
    for &b in bytes {
        let c = b as char;
        if c == '\'' {
            escaped.push_str("'\\''");
        } else {
            escaped.push(c);
        }
    }
    escaped.push('\'');
    escaped
}

fn color_is_enabled(choice: ColorChoice) -> bool {
    // Respect NO_COLOR, CLICOLOR, CLICOLOR_FORCE, and TTY
    if std::env::var_os("NO_COLOR").is_some() { return false; }
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never  => false,
        ColorChoice::Auto => {
            if std::env::var_os("CLICOLOR_FORCE").is_some() { return true; }
            if let Ok(v) = std::env::var("CLICOLOR") && v == "0" { return false; }            
            io::stderr().is_terminal() || io::stdout().is_terminal()
        }
    }
}

// Cyan message for Rommy (stderr)
fn rommy_note_cyan(colors: bool, msg: &str) {
    if colors {
        eprintln!("{CYAN}{msg}{RESET}");
    } else {
        eprintln!("{msg}");
    }
}