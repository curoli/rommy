use std::fs;
use std::process::Command;

#[test]
fn rommy_basic_run_produces_valid_blocks() {
    let out_path = "target/tmp/smoke_test.rommy";
    let _ = fs::remove_file(out_path); // falls von vorher noch da

    // 1. Führe Rommy aus, einfache Kommandolinie
    let status = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "--out", out_path, "--", "echo", "Hello Rommy"])
        .status()
        .expect("Failed to run Rommy via cargo");

    assert!(status.success(), "Rommy execution failed");

    // 2. Datei lesen und Inhalt prüfen
    let content = fs::read_to_string(out_path)
        .unwrap_or_else(|e| panic!("Failed to read output file {out_path}: {e}"));

    // 3. Prüfe, ob die vier Blockmarker vorhanden sind
    for block in ["<<<META>>>", "<<<COMMAND>>>", "<<<STDOUT>>>", "<<<STDERR>>>"] {
        assert!(
            content.contains(block),
            "Output does not contain expected block marker: {}",
            block
        );
    }

    // 4. Optional: Sicherstellen, dass der stdout-Block das erwartete Echo enthält
    assert!(
        content.contains("Hello Rommy"),
        "STDOUT block did not contain expected output"
    );
}
