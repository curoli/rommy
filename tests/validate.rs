use serde_json::Value;
use std::fs;
use std::process::Command;

#[test]
fn validate_accepts_valid_rommy_file() {
    let out_path = "target/tmp/validate_ok.rommy";
    let _ = fs::remove_file(out_path);
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let run_status = Command::new(bin)
        .args(["run", "--out", out_path, "--", "echo", "hello-validate"])
        .status()
        .expect("failed to execute rommy run");
    assert!(run_status.success(), "rommy run should succeed");

    let validate = Command::new(bin)
        .args(["validate", out_path])
        .output()
        .expect("failed to execute rommy validate");

    assert!(
        validate.status.success(),
        "validate should succeed, stderr: {}",
        String::from_utf8_lossy(&validate.stderr)
    );

    let stdout = String::from_utf8_lossy(&validate.stdout);
    assert!(stdout.contains("OK"), "expected OK line, got: {stdout}");
    assert!(
        stdout.contains("Validated 1 file(s)."),
        "expected summary line, got: {stdout}"
    );
}

#[test]
fn validate_fails_for_invalid_rommy_file() {
    let bad_path = "target/tmp/validate_bad.rommy";
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");
    fs::write(
        bad_path,
        "<<<META>>>\nstatus: ok\n<<<END>>>\n<<<COMMAND>>>\n$ echo x\n<<<END>>>\n",
    )
    .expect("failed to write invalid rommy file");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let validate = Command::new(bin)
        .args(["validate", bad_path])
        .output()
        .expect("failed to execute rommy validate");

    assert!(!validate.status.success(), "validate should fail");
    let stderr = String::from_utf8_lossy(&validate.stderr);
    assert!(
        stderr.contains("Validation failed"),
        "expected failure summary, got: {stderr}"
    );
}

#[test]
fn validate_quiet_suppresses_ok_lines() {
    let out_path = "target/tmp/validate_quiet_ok.rommy";
    let _ = fs::remove_file(out_path);
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let run_status = Command::new(bin)
        .args(["run", "--out", out_path, "--", "echo", "hello-quiet"])
        .status()
        .expect("failed to execute rommy run");
    assert!(run_status.success(), "rommy run should succeed");

    let validate = Command::new(bin)
        .args(["validate", "--quiet", out_path])
        .output()
        .expect("failed to execute rommy validate");

    assert!(validate.status.success(), "validate should succeed");
    let stdout = String::from_utf8_lossy(&validate.stdout);
    assert!(
        !stdout.contains("OK "),
        "quiet mode should suppress per-file OK lines, got: {stdout}"
    );
    assert!(
        stdout.contains("Validated 1 file(s)."),
        "quiet mode should still print summary, got: {stdout}"
    );
}

#[test]
fn validate_json_outputs_machine_readable_result() {
    let ok_path = "target/tmp/validate_json_ok.rommy";
    let bad_path = "target/tmp/validate_json_bad.rommy";
    let _ = fs::remove_file(ok_path);
    let _ = fs::remove_file(bad_path);
    fs::create_dir_all("target/tmp").expect("failed to create target/tmp");

    let bin = env!("CARGO_BIN_EXE_rommy");
    let run_status = Command::new(bin)
        .args(["run", "--out", ok_path, "--", "echo", "hello-json"])
        .status()
        .expect("failed to execute rommy run");
    assert!(run_status.success(), "rommy run should succeed");

    fs::write(
        bad_path,
        "<<<META>>>\nstatus: ok\n<<<END>>>\n<<<COMMAND>>>\n$ echo x\n<<<END>>>\n",
    )
    .expect("failed to write invalid rommy file");

    let validate = Command::new(bin)
        .args(["validate", "--format", "json", ok_path, bad_path])
        .output()
        .expect("failed to execute rommy validate");

    assert!(
        !validate.status.success(),
        "json output should still fail exit code when invalid files exist"
    );

    let stdout = String::from_utf8_lossy(&validate.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("json output should be valid JSON");

    assert_eq!(parsed["total_files"], 2);
    assert_eq!(parsed["ok_files"], 1);
    assert_eq!(parsed["error_files"], 1);
    assert!(parsed["files"].is_array());
    assert_eq!(parsed["files"].as_array().map(|a| a.len()), Some(2));
}
