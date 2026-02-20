use std::fs;
use std::io::Write;
use std::process::Command;

#[test]
fn rommy_script_run_produces_valid_blocks() {
    let tmp_dir = "target/tmp";
    let _ = fs::create_dir_all(tmp_dir);

    let script_path = format!("{}/test_script.sh", tmp_dir);
    let out_path = format!("{}/script_test.rommy", tmp_dir);

    // 1. Erstelle ein kleines Bash-Skript mit mehreren Ausgaben
    let mut f = fs::File::create(&script_path).expect("Failed to create test script");
    writeln!(
        f,
        r#"#!/usr/bin/env bash
set -Eeuo pipefail

echo "This is stdout"
echo "This is stderr" 1>&2
echo "Another stdout line"
"#,
    )
    .unwrap();
    drop(f);

    // 2. Skript ausführbar machen
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();
    }

    // 3. Rommy im Skriptmodus aufrufen
    let status = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "run",
            "--script",
            &script_path,
            "--out",
            &out_path,
        ])
        .status()
        .expect("Failed to run Rommy in script mode");

    assert!(status.success(), "Rommy script run failed");

    // 4. Datei lesen und prüfen
    let content = fs::read_to_string(&out_path)
        .unwrap_or_else(|e| panic!("Failed to read output file {out_path}: {e}"));

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

    // 5. Inhalt prüfen
    assert!(
        content.contains("This is stdout"),
        "STDOUT block missing expected text"
    );
    assert!(
        content.contains("This is stderr"),
        "STDERR block missing expected text"
    );
    assert!(
        content.contains("Another stdout line"),
        "STDOUT block missing second expected line"
    );
}
