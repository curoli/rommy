use anyhow::{Context, Result};
use clap::{ArgAction, Parser, Subcommand};
use chrono::{DateTime, Utc};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Parser, Debug)]
#[command(name="rommy", version, about="Structured run snapshots for chat & reviews")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Execute a bash command or bash script and write a 4-block rommy file
    Run {
        /// Output file (will be created if missing)
        #[arg(long, value_name="FILE")]
        out: PathBuf,

        /// Working directory
        #[arg(long, value_name="DIR")]
        cwd: Option<PathBuf>,

        /// Provide KEY=VALUE environment pairs (repeatable)
        #[arg(long = "env", value_name="KEY=VALUE", action=ArgAction::Append)]
        envs: Vec<String>,

        /// Append instead of overwrite
        #[arg(long)]
        append: bool,

        /// Optional label to include in META
        #[arg(long)]
        label: Option<String>,

        /// Run given bash script file instead of a single command
        #[arg(long, value_name="SCRIPT.sh", conflicts_with = "cmd")]
        script: Option<PathBuf>,

        /// Command to run (after --). Example: rommy run --out x -- cargo test
        #[arg(last = true)]
        cmd: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Run { out, cwd, envs, append, label, script, cmd } => {
            run(out, cwd, envs, append, label, script, cmd)
        }
    }
}

fn run(
    out: PathBuf,
    cwd: Option<PathBuf>,
    envs: Vec<String>,
    append: bool,
    label: Option<String>,
    script: Option<PathBuf>,
    cmd: Vec<String>,
) -> Result<()> {
    // Resolve CWD
    let cwd_path = cwd.unwrap_or(std::env::current_dir()?);

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
        anyhow::ensure!(!cmd.is_empty(), "Provide either --script <file> or a command after --");
        let bash_line = shell_join(&cmd)?;
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
    for kv in envs {
        if let Some((k, v)) = kv.split_once('=') {
            command.env(k, v);
        } else {
            eprintln!("WARN: ignoring malformed --env '{}', expected KEY=VALUE", kv);
        }
    }

    // Collect metadata
    let rommy_version = env!("CARGO_PKG_VERSION").to_string();
    let user = whoami::username();
    let host = whoami::fallible::hostname();

    let start: DateTime<Utc> = Utc::now();
    let output = command.output().with_context(|| "Failed to execute process")?;
    let end: DateTime<Utc> = Utc::now();
    let duration_ms = (end - start).num_milliseconds();

    let exit_code = output.status.code().unwrap_or(-1);
    let status = if output.status.success() { "ok" } else { "error" };

    let stdout_txt = String::from_utf8_lossy(&output.stdout);
    let stderr_txt = String::from_utf8_lossy(&output.stderr);

    // Prepare writer
    if let Some(parent) = out.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create directory {}", parent.display()))?;
        }
    }
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&out)
        .with_context(|| format!("Cannot open {}", out.display()))?;

    // --- Write blocks ---
    // META
    writeln!(f, "<<<META>>>")?;
    writeln!(f, "rommy_version: {}", rommy_version)?;
    if let Some(label) = label { writeln!(f, "label: {}", label)?; }
    writeln!(f, "cwd: {}", fs::canonicalize(&cwd_path)?.display())?;
    writeln!(f, "user: {}", user)?;
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
    writeln!(f, "status: {}", status)?;
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

    // STDERR
    writeln!(f, "<<<STDERR>>>")?;
    f.write_all(stderr_txt.as_bytes())?;
    writeln!(f, "<<<END>>>")?;

    eprintln!("Wrote {}", out.display());
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
