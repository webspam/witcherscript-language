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

#[test]
fn wsformat_toml_in_cwd_is_applied() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    temp.child(".wsformat.toml")
        .write_str("use_tabs = true\n")?;
    let file = temp.child("script.ws");
    file.write_str(MESSY)?;

    wsformat()
        .current_dir(temp.path())
        .arg("script.ws")
        .assert()
        .success();

    file.assert(predicate::str::contains("\tvar x : int;"));
    Ok(())
}

#[test]
fn explicit_flag_overrides_config_file() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    temp.child(".wsformat.toml").write_str("tab_size = 2\n")?;
    let file = temp.child("script.ws");
    file.write_str(MESSY)?;

    wsformat()
        .current_dir(temp.path())
        .args(["--tab-size", "8"])
        .arg("script.ws")
        .assert()
        .success();

    file.assert(predicate::str::contains("\n        var x : int;"));
    Ok(())
}

#[test]
fn dot_prefixed_config_takes_precedence() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    temp.child(".wsformat.toml")
        .write_str("use_tabs = true\n")?;
    temp.child("wsformat.toml").write_str("tab_size = 2\n")?;
    let file = temp.child("script.ws");
    file.write_str(MESSY)?;

    wsformat()
        .current_dir(temp.path())
        .arg("script.ws")
        .assert()
        .success();

    file.assert(predicate::str::contains("\tvar x : int;"));
    Ok(())
}

#[test]
fn config_is_found_in_an_ancestor_directory() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    temp.child(".wsformat.toml")
        .write_str("use_tabs = true\n")?;
    let nested = temp.child("nested/dir");
    nested.create_dir_all()?;
    let file = temp.child("nested/dir/script.ws");
    file.write_str(MESSY)?;

    wsformat()
        .current_dir(nested.path())
        .arg("script.ws")
        .assert()
        .success();

    file.assert(predicate::str::contains("\tvar x : int;"));
    Ok(())
}

#[test]
fn malformed_config_fails_without_writing() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    temp.child(".wsformat.toml")
        .write_str("tab_size = \"not a number\"\n")?;
    let file = temp.child("script.ws");
    file.write_str(MESSY)?;

    wsformat()
        .current_dir(temp.path())
        .arg("script.ws")
        .assert()
        .failure();

    file.assert(MESSY);
    Ok(())
}

#[test]
fn config_is_resolved_from_the_files_directory_not_cwd() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    let proj = temp.child("proj");
    proj.create_dir_all()?;
    temp.child("proj/.wsformat.toml")
        .write_str("use_tabs = true\n")?;
    let file = temp.child("proj/script.ws");
    file.write_str(MESSY)?;

    wsformat()
        .current_dir(temp.path())
        .arg("proj/script.ws")
        .assert()
        .success();

    file.assert(predicate::str::contains("\tvar x : int;"));
    Ok(())
}
