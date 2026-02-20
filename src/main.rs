use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use fs2::FileExt;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::scratch::launch_editor_and_get_script;

mod outpath;
mod scratch;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

#[derive(Args, Debug, Clone)]
pub struct RunConfig {
    /// Output file (optional; if omitted, Rommy chooses a time-based path)
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Working directory
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Provide KEY=VALUE environment pairs (repeatable)
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub envs: Vec<String>,

    /// Append instead of overwrite
    #[arg(long)]
    pub append: bool,

    /// Optional label to include in META
    #[arg(long)]
    pub label: Option<String>,

    /// Run given bash script file instead of a single command
    #[arg(long, value_name = "SCRIPT.sh", conflicts_with = "cmd")]
    pub script: Option<PathBuf>,

    /// Disable live streaming to terminal (default: streaming ON)
    #[arg(long = "no-stream")]
    pub no_stream: bool,

    /// Command to run (after --). Example: rommy run -- cargo test
    #[arg(last = true)]
    pub cmd: Vec<String>,

    /// Color output: auto|always|never (default: auto)
    #[arg(long = "color", value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
}

#[derive(Parser, Debug)]
#[command(name = "rommy")]
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
    Validate {
        #[command(flatten)]
        validate_config: ValidateConfig,
    },
}

#[derive(Args, Debug, Clone)]
pub struct ValidateConfig {
    /// File(s) or directory path(s) to validate
    #[arg(value_name = "PATH", required = true)]
    pub paths: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Run { run_config } => run(run_config),
        Commands::Validate { validate_config } => validate(validate_config),
    }
}

/// Führt den Child-Prozess aus.
/// - stream=true: stdout/stderr werden live ins Terminal gespiegelt UND gesammelt.
/// - stream=false: stdout/stderr werden vollständig gesammelt (keine Terminalausgabe).
///   Rückgabe: (stdout_bytes, stderr_bytes, exit_code)
fn spawn_and_stream(
    mut child: Child,
    stream: bool,
    colors: bool,
) -> anyhow::Result<(Vec<u8>, Vec<u8>, i32)> {
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

        let stdout_bytes = h_out
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        let stderr_bytes = h_err
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();

        Ok((stdout_bytes, stderr_bytes, code))
    } else {
        // kein Streaming: keine take(); vollständiges Lesen
        let output = child.wait_with_output()?;
        let code = output.status.code().unwrap_or(-1);
        Ok((output.stdout, output.stderr, code))
    }
}

fn temp_out_path(out_path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let base = out_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rommy.rommy");
    let mut tmp = out_path.to_path_buf();
    tmp.set_file_name(format!(".{base}.{pid}.{nanos}.tmp"));
    tmp
}

fn lock_path(out_path: &Path) -> PathBuf {
    let mut lock = out_path.to_path_buf();
    let base = out_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rommy.rommy");
    lock.set_file_name(format!(".{base}.lock"));
    lock
}

fn write_record(
    f: &mut fs::File,
    rommy_version: &str,
    label: Option<&str>,
    cwd_abs: &Path,
    user: Option<&str>,
    host: Option<&str>,
    display_command: &RommyCommand,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
    duration_ms: i64,
    out_path: &Path,
    status_str: &str,
    exit_code: i32,
    stdout_bytes: &[u8],
    stderr_bytes: &[u8],
) -> Result<()> {
    // META
    writeln!(f, "<<<META>>>")?;
    writeln!(f, "rommy_version: {}", rommy_version)?;
    if let Some(label) = label {
        writeln!(f, "label: {}", label)?;
    }
    writeln!(f, "cwd: {}", cwd_abs.display())?;
    if let Some(user) = user {
        writeln!(f, "user: {}", user)?;
    }
    if let Some(host) = host {
        writeln!(f, "host: {}", host)?;
    }
    match display_command {
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
    match display_command {
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
    f.write_all(stdout_bytes)?;
    if !stdout_bytes.is_empty() && !stdout_bytes.ends_with(b"\n") {
        writeln!(f)?;
    }
    writeln!(f, "<<<END>>>")?;

    // STDERR
    writeln!(f, "<<<STDERR>>>")?;
    f.write_all(stderr_bytes)?;
    if !stderr_bytes.is_empty() && !stderr_bytes.ends_with(b"\n") {
        writeln!(f)?;
    }
    writeln!(f, "<<<END>>>")?;

    Ok(())
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

        let display = format!("#!/usr/bin/env bash\n{}\n", script_text);

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

        (
            RommyCommand::Script {
                path: script_abs,
                content: display,
            },
            command,
        )
    } else {
        anyhow::ensure!(
            !cfg.cmd.is_empty(),
            "Provide either --script <file> or a command after --"
        );
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
            eprintln!(
                "WARN: ignoring malformed --env '{}', expected KEY=VALUE",
                kv
            );
        }
    }

    // Collect metadata
    let rommy_version = env!("CARGO_PKG_VERSION").to_string();
    let user = whoami::username().ok();
    let host = whoami::hostname().ok();

    // Ensure output pipes are always configured for both streaming and capture mode.
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let start: DateTime<Utc> = Utc::now();
    let child = command.spawn().with_context(|| "Failed to spawn process")?;

    let (stdout_bytes, stderr_bytes, exit_code) =
        spawn_and_stream(child, stream, colors).with_context(|| "stream/capture failed")?;

    let end: DateTime<Utc> = Utc::now();
    let duration_ms = (end - start).num_milliseconds();

    let status_str = if exit_code == 0 { "ok" } else { "error" };

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
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create directory {}", parent.display()))?;
    }
    let lock_path = lock_path(&out_path);
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("Cannot open lock file {}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("Cannot acquire lock {}", lock_path.display()))?;

    let cwd_abs = fs::canonicalize(&cwd_path)
        .with_context(|| format!("Cannot resolve cwd {}", cwd_path.display()))?;
    let label = cfg.label.as_deref();

    let tmp_path = temp_out_path(&out_path);
    let write_result = (|| -> Result<()> {
        let mut temp = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .with_context(|| format!("Cannot create {}", tmp_path.display()))?;

        if cfg.append {
            // Best effort atomic append: copy old file into temp first, then append new record.
            match fs::File::open(&out_path) {
                Ok(mut current) => {
                    io::copy(&mut current, &mut temp).with_context(|| {
                        format!("Cannot copy existing content from {}", out_path.display())
                    })?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(e).with_context(|| format!("Cannot open {}", out_path.display()));
                }
            }
        }

        write_record(
            &mut temp,
            &rommy_version,
            label,
            &cwd_abs,
            user.as_deref(),
            host.as_deref(),
            &display_command,
            &start,
            &end,
            duration_ms,
            &out_path,
            status_str,
            exit_code,
            &stdout_bytes,
            &stderr_bytes,
        )?;

        temp.sync_all()
            .with_context(|| format!("Cannot sync {}", tmp_path.display()))?;

        #[cfg(windows)]
        if out_path.exists() {
            fs::remove_file(&out_path)
                .with_context(|| format!("Cannot replace {}", out_path.display()))?;
        }

        fs::rename(&tmp_path, &out_path).with_context(|| {
            format!(
                "Cannot atomically move {} to {}",
                tmp_path.display(),
                out_path.display()
            )
        })?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    lock_file.unlock().ok();

    rommy_note_cyan(colors, &format!("Wrote {}", out_path.display()));
    Ok(())
}

fn collect_rommy_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let meta = fs::metadata(path).with_context(|| format!("Cannot stat {}", path.display()))?;
    if meta.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if meta.is_dir() {
        for entry in
            fs::read_dir(path).with_context(|| format!("Cannot read {}", path.display()))?
        {
            let entry = entry?;
            let p = entry.path();
            let m = entry.metadata()?;
            if m.is_dir() {
                collect_rommy_files(&p, out)?;
            } else if p
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("rommy"))
                .unwrap_or(false)
            {
                out.push(p);
            }
        }
        return Ok(());
    }
    anyhow::bail!("Unsupported path type: {}", path.display());
}

fn validate(cfg: ValidateConfig) -> Result<()> {
    let mut files = Vec::new();
    for path in &cfg.paths {
        collect_rommy_files(path, &mut files)?;
    }

    files.sort();
    files.dedup();

    anyhow::ensure!(!files.is_empty(), "No files found to validate");

    let mut ok_count = 0usize;
    let mut err_count = 0usize;

    for file in &files {
        match rommy::parser::parse_file(file) {
            Ok(records) => {
                println!("OK {} ({} record(s))", file.display(), records.len());
                ok_count += 1;
            }
            Err(err) => {
                eprintln!("ERR {}: {}", file.display(), err);
                err_count += 1;
            }
        }
    }

    if err_count > 0 {
        anyhow::bail!(
            "Validation failed: {} file(s) invalid, {} file(s) valid",
            err_count,
            ok_count
        );
    }

    println!("Validated {} file(s).", ok_count);
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
        if i > 0 {
            out.push(' ');
        }
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
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            if std::env::var_os("CLICOLOR_FORCE").is_some() {
                return true;
            }
            if let Ok(v) = std::env::var("CLICOLOR")
                && v == "0"
            {
                return false;
            }
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
