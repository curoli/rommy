use chrono::Local;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Ermittle das Default-Root-Verzeichnis für Rommy-Outputs.
pub fn default_root_dir() -> PathBuf {
    // 1) explizit per Env
    if let Ok(root) = env::var("ROMMY_ROOT") {
        return PathBuf::from(root);
    }

    // 2) XDG_STATE_HOME (Linux/Unix-Style)
    if let Ok(xdg) = env::var("XDG_STATE_HOME") {
        return Path::new(&xdg).join("rommy");
    }

    // 3) OS-spezifische Defaults
    if cfg!(target_os = "macos") {
        return home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join("Library")
            .join("Application Support")
            .join("Rommy");
    }

    if cfg!(target_os = "windows") {
        if let Ok(appdata) = env::var("LOCALAPPDATA") {
            return Path::new(&appdata).join("Rommy");
        }
        if let Ok(userprofile) = env::var("USERPROFILE") {
            return Path::new(&userprofile)
                .join("AppData")
                .join("Local")
                .join("Rommy");
        }
    }

    // 4) Fallback Linux/Unix
    home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".local")
        .join("state")
        .join("rommy")
}

/// Erzeuge den finalen Pfad (und legt benötigte Ordner an), wenn --out nicht gesetzt ist.
/// Beispiel: <root>/2025/10/20/173048.cargo_clippy.rommy
pub fn resolve_auto_out_path(cmd_display: &str) -> std::io::Result<PathBuf> {
    let root = default_root_dir();
    let now = Local::now();
    let yyyy = now.format("%Y").to_string();
    let mm = now.format("%m").to_string();
    let dd = now.format("%d").to_string();
    let hms = now.format("%H%M%S").to_string();

    let token = command_token(cmd_display);
    let file = format!("{hms}.{token}.rommy");

    let dir = root.join(yyyy).join(mm).join(dd);
    fs::create_dir_all(&dir)?;
    Ok(dir.join(file))
}

/// Aus einem Befehls-String einen kurzen, dateitauglichen Token machen.
fn command_token(s: &str) -> String {
    // Beispiele:
    // "$ cargo clippy -q" -> "cargo_clippy"
    // "#!/usr/bin/env bash\n<script>" -> "bash_script"
    let mut base = s.trim().to_string();

    // Wenn es eine Shell-Zeile ist, entferne führendes "$ "
    if base.starts_with("$ ") {
        base = base.trim_start_matches("$ ").to_string();
    }

    // Nur die ersten ~2 "Wörter" für die Kurzform verwenden
    let first_words: String = base
        .split_whitespace()
        .take(2)
        .collect::<Vec<&str>>()
        .join("_");

    // Ersetze Nicht-Alnum durch '_', verdichte '_', trimme auf Länge
    let mut clean = String::with_capacity(first_words.len());
    let mut prev_us = false;
    for ch in first_words.chars() {
        let is_alnum = ch.is_ascii_alphanumeric();
        if is_alnum {
            clean.push(ch);
            prev_us = false;
        } else if !prev_us {
            clean.push('_');
            prev_us = true;
        }
    }
    let clean = clean.trim_matches('_').to_lowercase();

    let mut final_token = if clean.is_empty() {
        "cmd".to_string()
    } else {
        clean
    };
    if final_token.len() > 32 {
        final_token.truncate(32);
    }

    // Spezielle Heuristik: Skript-Erkennung
    if final_token == "usr_bin_env" || final_token == "bin_bash" {
        "bash_script".to_string()
    } else {
        final_token
    }
}

fn home_dir() -> Option<PathBuf> {
    // Minimaler Home-Sucher ohne externe Crate
    if cfg!(target_os = "windows") {
        env::var("USERPROFILE")
            .ok()
            .map(PathBuf::from)
            .or_else(|| env::var("HOME").ok().map(PathBuf::from))
    } else {
        env::var("HOME").ok().map(PathBuf::from)
    }
}
