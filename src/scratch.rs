use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Pick an editor from $EDITOR/$VISUAL or sensible defaults.
fn pick_editor() -> (String, Vec<String>) {
    use std::env;
    if let Ok(ed) = env::var("EDITOR") {
        return (ed, vec![]);
    }
    if let Ok(ed) = env::var("VISUAL") {
        return (ed, vec![]);
    }
    #[cfg(target_os = "windows")]
    { return ("notepad".to_string(), vec![]); }
    #[cfg(not(target_os = "windows"))]
    { ("nano".to_string(), vec![]) }
}

/// Add “wait” flags for GUI editors that otherwise detach.
fn editor_wait_args(editor_cmd: &str) -> Vec<String> {
    let e = editor_cmd.to_lowercase();
    if e.contains("code") || e.contains("codium") { return vec!["--wait".into()]; }
    if e.contains("subl") || e.contains("sublime_text") { return vec!["-w".into()]; }
    if e.contains("gedit") { return vec!["--wait".into()]; }
    vec![]
}

/// Create a temporary, executable bash script with a safe template.
fn write_scratch_script(path: &Path) -> std::io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(br#"#!/usr/bin/env bash
set -Eeuo pipefail
# Rommy scratch script: write your commands below, then save & close the editor.
echo "Hello from Rommy scratch!"
"#)?;
    f.flush()?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

/// Open an editor on a scratch script in the system temp dir and return its path.
/// The file will be placed under: {temp_dir}/rommy/scratch-{ts}-{pid}.sh
pub fn launch_editor_and_get_script() -> Result<PathBuf> {
    // Temp base: OS-specific
    let mut dir = std::env::temp_dir();
    dir.push("rommy");
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create scratch dir {}", dir.display()))?;

    // Unique filename: timestamp + pid
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pid = std::process::id();
    let script_path = dir.join(format!("scratch-{ts}-{pid}.sh"));

    write_scratch_script(&script_path)
        .with_context(|| format!("Failed to write scratch script {}", script_path.display()))?;

    // Pick editor & wait flags
    let (editor, mut args) = pick_editor();
    args.extend(editor_wait_args(&editor));
    args.push(script_path.display().to_string());

    // Launch editor and wait
    let status = Command::new(&editor)
        .args(&args)
        .status()
        .with_context(|| format!("Failed to launch editor '{}'", editor))?;
    if !status.success() {
        return Err(anyhow!("Editor exited with non-zero status"));
    }

    // Ensure there is at least one non-comment, non-empty line
    let content = fs::read_to_string(&script_path)
        .with_context(|| "Failed to read scratch script after editor")?;
    let has_real_content = content.lines().any(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    });
    if !has_real_content {
        return Err(anyhow!("Scratch script is empty. Aborting."));
    }

    Ok(script_path)
}
