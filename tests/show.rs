use serde_json::Value;
use std::fs;
use std::process::Command;

#[test]
fn show_text_displays_record_blocks() {
    let out_path = "target/tmp/show_text.rommy";
    let _ = fs::remove_file(out_path);
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let run_status = Command::new(bin)
        .args(["run", "--out", out_path, "--", "echo", "show-text"])
        .status()
        .expect("failed to execute rommy run");
    assert!(run_status.success(), "rommy run should succeed");

    let show = Command::new(bin)
        .args(["show", out_path])
        .output()
        .expect("failed to execute rommy show");
    assert!(
        show.status.success(),
        "show should succeed, stderr: {}",
        String::from_utf8_lossy(&show.stderr)
    );
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(
        stdout.contains("=== Record 1 ==="),
        "unexpected output: {stdout}"
    );
    assert!(stdout.contains("<<<META>>>"), "unexpected output: {stdout}");
    assert!(stdout.contains("show-text"), "unexpected output: {stdout}");
}

#[test]
fn show_json_outputs_machine_readable_records() {
    let out_path = "target/tmp/show_json.rommy";
    let _ = fs::remove_file(out_path);
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let run_status = Command::new(bin)
        .args(["run", "--out", out_path, "--", "echo", "show-json"])
        .status()
        .expect("failed to execute rommy run");
    assert!(run_status.success(), "rommy run should succeed");

    let show = Command::new(bin)
        .args(["show", "--format", "json", out_path])
        .output()
        .expect("failed to execute rommy show");
    assert!(show.status.success(), "show json should succeed");

    let stdout = String::from_utf8_lossy(&show.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("show json output should be JSON");
    assert_eq!(parsed["path"], out_path);
    assert_eq!(parsed["records"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(parsed["records"][0]["record"], 1);
    assert_eq!(parsed["records"][0]["stdout"], "show-json");
}

#[test]
fn show_record_selects_single_record() {
    let out_path = "target/tmp/show_record_select.rommy";
    let _ = fs::remove_file(out_path);
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let first = Command::new(bin)
        .args(["run", "--out", out_path, "--", "echo", "first-show"])
        .status()
        .expect("failed first run");
    assert!(first.success(), "first run should succeed");
    let second = Command::new(bin)
        .args([
            "run",
            "--append",
            "--out",
            out_path,
            "--",
            "echo",
            "second-show",
        ])
        .status()
        .expect("failed second run");
    assert!(second.success(), "second run should succeed");

    let show = Command::new(bin)
        .args(["show", "--record", "2", out_path])
        .output()
        .expect("failed to execute rommy show");
    assert!(show.status.success(), "show should succeed");
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(
        stdout.contains("=== Record 2 ==="),
        "expected selected record header, got: {stdout}"
    );
    assert!(
        stdout.contains("second-show"),
        "expected second record output, got: {stdout}"
    );
    assert!(
        !stdout.contains("first-show"),
        "should not include first record output, got: {stdout}"
    );
}

#[test]
fn show_record_out_of_range_fails() {
    let out_path = "target/tmp/show_record_range.rommy";
    let _ = fs::remove_file(out_path);
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let run_status = Command::new(bin)
        .args(["run", "--out", out_path, "--", "echo", "one-record"])
        .status()
        .expect("failed to execute rommy run");
    assert!(run_status.success(), "rommy run should succeed");

    let show = Command::new(bin)
        .args(["show", "--record", "2", out_path])
        .output()
        .expect("failed to execute rommy show");
    assert!(!show.status.success(), "show should fail for invalid index");
    let stderr = String::from_utf8_lossy(&show.stderr);
    assert!(
        stderr.contains("out of range"),
        "expected out-of-range message, got: {stderr}"
    );
}
