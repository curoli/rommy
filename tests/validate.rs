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
