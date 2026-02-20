use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RommyRecord {
    /// META als Key→Value (letzter Eintrag gewinnt bei Duplikaten)
    pub meta: HashMap<String, String>,
    /// Der im COMMAND-Block angezeigte Inhalt (Bash-Zeile oder Script-Text)
    pub command: String,
    /// Rohes STDOUT
    pub stdout: String,
    /// Rohes STDERR
    pub stderr: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Meta,
    Command,
    Stdout,
    Stderr,
}

impl Block {
    fn from_marker(s: &str) -> Option<Self> {
        match s.trim() {
            "<<<META>>>" => Some(Block::Meta),
            "<<<COMMAND>>>" => Some(Block::Command),
            "<<<STDOUT>>>" => Some(Block::Stdout),
            "<<<STDERR>>>" => Some(Block::Stderr),
            _ => None,
        }
    }
}

/// Parse eine .rommy-Datei in eine Liste von Records.
pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Vec<RommyRecord>> {
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.as_ref().display()))?;
    parse_str(&text)
}

/// Parse den Text-Inhalt (kann mehrere Records enthalten).
pub fn parse_str(input: &str) -> Result<Vec<RommyRecord>> {
    // Wir arbeiten zeilenbasiert, sind tolerant gegenüber CRLF und Leerzeilen
    let input = input.replace("\r\n", "\n");
    let lines = input.lines().peekable();

    let mut out: Vec<RommyRecord> = Vec::new();

    // Hilfsbuffer für gerade entstehenden Record
    let mut cur_meta: Option<HashMap<String, String>> = None;
    let mut cur_cmd = String::new();
    let mut cur_stdout = String::new();
    let mut cur_stderr = String::new();

    // Zustandsmaschine innerhalb eines Records
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum State {
        Idle,
        InBlock(Block),
    }
    let mut state = State::Idle;

    // Hilfsclosure: neuen leeren Record vorbereiten
    let start_record = |cur_meta: &mut Option<HashMap<String, String>>,
                        cur_cmd: &mut String,
                        cur_stdout: &mut String,
                        cur_stderr: &mut String| {
        *cur_meta = Some(HashMap::new());
        cur_cmd.clear();
        cur_stdout.clear();
        cur_stderr.clear();
    };

    // Welche Blöcke wurden im aktuellen Record korrekt mit <<<END>>> abgeschlossen?
    let mut saw_meta = false;
    let mut saw_command = false;
    let mut saw_stdout = false;
    let mut saw_stderr = false;

    // Hilfsclosure: aktuellen Record beenden und pushen
    let finish_record = |cur_meta: &mut Option<HashMap<String, String>>,
                         cur_cmd: &mut String,
                         cur_stdout: &mut String,
                         cur_stderr: &mut String,
                         saw_meta: bool,
                         saw_command: bool,
                         saw_stdout: bool,
                         saw_stderr: bool,
                         out: &mut Vec<RommyRecord>|
     -> Result<()> {
        let meta = cur_meta
            .take()
            .context("unexpected end of record: META missing")?;
        let mut missing = Vec::new();
        if !saw_meta {
            missing.push("META");
        }
        if !saw_command {
            missing.push("COMMAND");
        }
        if !saw_stdout {
            missing.push("STDOUT");
        }
        if !saw_stderr {
            missing.push("STDERR");
        }
        if !missing.is_empty() {
            bail!(
                "incomplete record: missing block(s): {}",
                missing.join(", ")
            );
        }

        out.push(RommyRecord {
            meta,
            command: cur_cmd.clone(),
            stdout: cur_stdout.clone(),
            stderr: cur_stderr.clone(),
        });
        Ok(())
    };

    for raw_line in lines {
        let line = raw_line;

        // Marker?
        if let Some(block) = Block::from_marker(line) {
            match state {
                State::Idle => {
                    match block {
                        Block::Meta => {
                            // Falls schon ein Record offen war, zuerst abschließen,
                            // BEVOR wir einen neuen starten.
                            if cur_meta.is_some() {
                                finish_record(
                                    &mut cur_meta,
                                    &mut cur_cmd,
                                    &mut cur_stdout,
                                    &mut cur_stderr,
                                    saw_meta,
                                    saw_command,
                                    saw_stdout,
                                    saw_stderr,
                                    &mut out,
                                )?;
                            }
                            start_record(
                                &mut cur_meta,
                                &mut cur_cmd,
                                &mut cur_stdout,
                                &mut cur_stderr,
                            );
                            saw_meta = false;
                            saw_command = false;
                            saw_stdout = false;
                            saw_stderr = false;
                            state = State::InBlock(Block::Meta);
                        }
                        // Erlaube Folgeblöcke, wenn META bereits gesehen wurde
                        Block::Command | Block::Stdout | Block::Stderr => {
                            if cur_meta.is_some() {
                                state = State::InBlock(block);
                            } else {
                                // Rauschen vor dem ersten META ignorieren
                                continue;
                            }
                        }
                    }
                }
                State::InBlock(_) => {
                    // Wir sind noch in einem Block und sehen sofort den nächsten Marker → Formatfehler
                    bail!(
                        "unexpected start of block {:?} before closing previous block",
                        block
                    );
                }
            }
            continue;
        }

        // Block-Ende?
        if line.trim() == "<<<END>>>" {
            match state {
                State::Idle => {
                    // END ohne Block → ignoriere (Rauschen)
                    continue;
                }
                State::InBlock(_) => {
                    match state {
                        State::InBlock(Block::Meta) => saw_meta = true,
                        State::InBlock(Block::Command) => saw_command = true,
                        State::InBlock(Block::Stdout) => saw_stdout = true,
                        State::InBlock(Block::Stderr) => saw_stderr = true,
                        State::Idle => {}
                    }
                    // Ein Block endet; entweder geht's weiter mit nächstem Block,
                    // oder ein neuer Record beginnt (wieder mit META), oder Datei endet.
                    state = State::Idle;
                }
            }
            continue;
        }

        // Normale Zeilen: je nach Blockinhalt einsammeln
        match state {
            State::Idle => {
                // Wenn wir bereits META/COMMAND/STDOUT/STDERR in diesem Record gesehen haben,
                // und jetzt normale Zeilen kommen (Rauschen), ignoriere sie.
                // Für Robustheit erlauben wir Kommentare/Zwischenzeilen außerhalb von Blöcken.
                continue;
            }
            State::InBlock(Block::Meta) => {
                // META ist key: value pro Zeile, leere Zeilen erlauben
                if line.trim().is_empty() {
                    continue;
                }
                if let Some((k, v)) = line.split_once(':') {
                    let key = k.trim().to_string();
                    let value = v.trim().to_string();
                    if let Some(m) = cur_meta.as_mut() {
                        m.insert(key, value);
                    }
                } else {
                    // Tolerant: ignoriere Zeilen ohne Doppelpunkt
                    // (alternativ: bail!("invalid meta line: {line}"));
                }
            }
            State::InBlock(Block::Command) => {
                if !cur_cmd.is_empty() {
                    cur_cmd.push('\n');
                }
                cur_cmd.push_str(line);
            }
            State::InBlock(Block::Stdout) => {
                if !cur_stdout.is_empty() {
                    cur_stdout.push('\n');
                }
                cur_stdout.push_str(line);
            }
            State::InBlock(Block::Stderr) => {
                if !cur_stderr.is_empty() {
                    cur_stderr.push('\n');
                }
                cur_stderr.push_str(line);
            }
        }
    }

    // Dateiende: Falls wir schon Blöcke gesehen haben, aber der Record nicht finalisiert wurde,
    // handelt es sich um eine unvollständige Datei oder es fehlen <<<END>>> Marker.
    // Wir prüfen, ob wir in einem offenen Block hängen:
    match state {
        State::InBlock(_) => {
            bail!("unexpected EOF: block not closed with <<<END>>>");
        }
        State::Idle => {
            if cur_meta.is_some() {
                finish_record(
                    &mut cur_meta,
                    &mut cur_cmd,
                    &mut cur_stdout,
                    &mut cur_stderr,
                    saw_meta,
                    saw_command,
                    saw_stdout,
                    saw_stderr,
                    &mut out,
                )?;
            }
        }
    }

    Ok(out)
}
