use rommy::parser::parse_str; // Falls du crate=bin hast, ändere Import unten (siehe Hinweis)

// Da dein Paket aktuell bin-only ist, kannst du entweder:
// 1) src/lib.rs mit `pub mod parser;` anlegen und im Cargo.toml `[lib]` sektion hinzufügen,
//    dann `use rommy::parser::parse_str;` (wie oben).
// 2) ODER in Tests relativ importieren: `use crate::parser::parse_str;` und mit `#[cfg(test)] mod parser;` arbeiten.
// Für Einfachheit empfehle ich Option 1 (kleine lib + bin).

#[test]
fn parse_single_record() {
    let sample = r#"<<<META>>>
rommy_version: 0.1.0
cwd: /home/ollie
status: ok
exit_code: 0
<<<END>>>
<<<COMMAND>>>
$ echo Hello
<<<END>>>
<<<STDOUT>>>
Hello
<<<END>>>
<<<STDERR>>>
<<<END>>>
"#;

    let recs = parse_str(sample).expect("parse failed");
    assert_eq!(recs.len(), 1);
    let r = &recs[0];
    assert_eq!(r.meta.get("status").map(String::as_str), Some("ok"));
    assert_eq!(r.meta.get("exit_code").map(String::as_str), Some("0"));
    assert!(r.stderr.is_empty());
    assert!(r.stdout.contains("Hello"));
    assert!(r.command.contains("echo Hello"));
}

#[test]
fn parse_two_records_appended() {
    let sample = r#"<<<META>>>
rommy_version: 0.1.0
status: ok
exit_code: 0
<<<END>>>
<<<COMMAND>>>
$ echo A
<<<END>>>
<<<STDOUT>>>
A
<<<END>>>
<<<STDERR>>>
<<<END>>>
<<<META>>>
rommy_version: 0.1.0
status: error
exit_code: 2
<<<END>>>
<<<COMMAND>>>
$ echo B 1>&2
<<<END>>>
<<<STDOUT>>>
<<<END>>>
<<<STDERR>>>
B
<<<END>>>
"#;

    let recs = parse_str(sample).expect("parse failed");
    assert_eq!(recs.len(), 2);

    assert_eq!(recs[0].meta.get("status").map(String::as_str), Some("ok"));
    assert_eq!(recs[0].stdout.trim(), "A");
    assert!(recs[0].stderr.trim().is_empty());

    assert_eq!(
        recs[1].meta.get("status").map(String::as_str),
        Some("error")
    );
    assert_eq!(recs[1].meta.get("exit_code").map(String::as_str), Some("2"));
    assert_eq!(recs[1].stderr.trim(), "B");
    assert!(recs[1].stdout.trim().is_empty());
}

#[test]
fn parse_incomplete_record_fails() {
    let sample = r#"<<<META>>>
rommy_version: 0.1.0
status: ok
exit_code: 0
<<<END>>>
<<<COMMAND>>>
$ echo A
<<<END>>>
<<<STDOUT>>>
A
<<<END>>>
"#;

    let err = parse_str(sample).expect_err("parse should fail for incomplete record");
    let msg = err.to_string();
    assert!(
        msg.contains("missing block(s): STDERR"),
        "unexpected parser error: {msg}"
    );
}
