use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

const MESSY: &str = "function f(){var x:int;}\n";
const FORMATTED: &str = "function f() {\n    var x : int;\n}\n";

fn wsformat() -> Command {
    Command::cargo_bin("wsformat").expect("wsformat binary builds")
}

#[test]
fn formats_a_file_in_place() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(MESSY)?;

    wsformat()
        .arg(file.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("formatted"));

    file.assert(FORMATTED);
    Ok(())
}

#[test]
fn already_formatted_file_is_left_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(FORMATTED)?;

    wsformat().arg(file.path()).assert().success();

    file.assert(FORMATTED);
    Ok(())
}

#[test]
fn check_mode_reports_drift_and_writes_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(MESSY)?;

    wsformat()
        .arg("--check")
        .arg(file.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("would reformat"));

    file.assert(MESSY);
    Ok(())
}

#[test]
fn check_mode_passes_when_already_formatted() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(FORMATTED)?;

    wsformat()
        .arg("--check")
        .arg(file.path())
        .assert()
        .success();
    Ok(())
}

#[test]
fn file_with_syntax_error_is_skipped_and_untouched() -> Result<(), Box<dyn std::error::Error>> {
    let broken = "function f( {\n";
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(broken)?;

    wsformat()
        .arg(file.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("skipped"));

    file.assert(broken);
    Ok(())
}

#[test]
fn use_tabs_flag_indents_with_tabs() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(MESSY)?;

    wsformat()
        .arg("--use-tabs")
        .arg(file.path())
        .assert()
        .success();

    file.assert(predicate::str::contains("\tvar x : int;"));
    Ok(())
}
