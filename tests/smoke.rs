use rommy::parser::parse_file;
use std::fs;
use std::process::Command;
use std::thread;

#[test]
fn rommy_basic_run_produces_valid_blocks() {
    let out_path = "target/tmp/smoke_test.rommy";
    let _ = fs::remove_file(out_path); // falls von vorher noch da

    // 1. Führe Rommy aus, einfache Kommandolinie
    let status = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "run",
            "--out",
            out_path,
            "--",
            "echo",
            "Hello Rommy",
        ])
        .status()
        .expect("Failed to run Rommy via cargo");

    assert!(status.success(), "Rommy execution failed");

    // 2. Datei lesen und Inhalt prüfen
    let content = fs::read_to_string(out_path)
        .unwrap_or_else(|e| panic!("Failed to read output file {out_path}: {e}"));

    // 3. Prüfe, ob die vier Blockmarker vorhanden sind
    for block in [
        "<<<META>>>",
        "<<<COMMAND>>>",
        "<<<STDOUT>>>",
        "<<<STDERR>>>",
    ] {
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

#[test]
fn rommy_stdout_without_trailing_newline_keeps_end_marker_on_own_line() {
    let out_path = "target/tmp/no_newline_smoke.rommy";
    let _ = fs::remove_file(out_path);

    let status = Command::new("cargo")
        .args([
            "run", "--quiet", "--", "run", "--out", out_path, "--", "printf", "x",
        ])
        .status()
        .expect("Failed to run Rommy via cargo");

    assert!(status.success(), "Rommy execution failed");

    let content = fs::read_to_string(out_path)
        .unwrap_or_else(|e| panic!("Failed to read output file {out_path}: {e}"));

    assert!(
        content.contains("<<<STDOUT>>>\nx\n<<<END>>>"),
        "STDOUT block should terminate with END marker on a separate line"
    );
}

#[test]
fn rommy_append_writes_two_records() {
    let out_path = "target/tmp/append_test.rommy";
    let _ = fs::remove_file(out_path);

    let status1 = Command::new("cargo")
        .args([
            "run", "--quiet", "--", "run", "--out", out_path, "--", "echo", "first",
        ])
        .status()
        .expect("Failed first run");
    assert!(status1.success(), "first run failed");

    let status2 = Command::new("cargo")
        .args([
            "run", "--quiet", "--", "run", "--append", "--out", out_path, "--", "echo", "second",
        ])
        .status()
        .expect("Failed second run");
    assert!(status2.success(), "second run failed");

    let recs = parse_file(out_path).expect("Failed to parse appended file");
    assert_eq!(recs.len(), 2, "Expected two records after append");
    assert!(
        recs[0].stdout.contains("first"),
        "First record missing output"
    );
    assert!(
        recs[1].stdout.contains("second"),
        "Second record missing output"
    );
}

#[test]
fn rommy_parallel_append_preserves_all_records() {
    let out_path = "target/tmp/append_parallel_test.rommy";
    let _ = fs::remove_file(out_path);
    let bin = env!("CARGO_BIN_EXE_rommy");
    const N: usize = 6;

    let handles: Vec<_> = (0..N)
        .map(|i| {
            thread::spawn(move || {
                let msg = format!("par-{i}");
                Command::new(bin)
                    .args([
                        "run",
                        "--append",
                        "--out",
                        out_path,
                        "--",
                        "echo",
                        msg.as_str(),
                    ])
                    .status()
            })
        })
        .collect();

    for h in handles {
        let status = h
            .join()
            .expect("append thread panicked")
            .expect("run failed");
        assert!(status.success(), "parallel append run failed");
    }

    let recs = parse_file(out_path).expect("Failed to parse parallel append file");
    assert_eq!(recs.len(), N, "Expected one record per parallel append");
    for i in 0..N {
        let expected = format!("par-{i}");
        assert!(
            recs.iter().any(|r| r.stdout.contains(&expected)),
            "Missing output for {expected}"
        );
    }
}
