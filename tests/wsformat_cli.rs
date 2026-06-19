use assert_cmd::Command;
use assert_fs::prelude::*;
use expect_test::expect;
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
fn colon_spacing_flag_produces_compact_colons() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(MESSY)?;

    wsformat()
        .args(["--colon-spacing", "compact"])
        .arg(file.path())
        .assert()
        .success();

    file.assert(predicate::str::contains("var x: int;"));
    Ok(())
}

#[test]
fn align_member_colons_flag_pads_consecutive_members() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str("class C { var x : int; var someLongName : string; }\n")?;

    wsformat()
        .arg("--align-member-colons")
        .arg(file.path())
        .assert()
        .success();

    file.assert(predicate::str::contains("var x            : int;"));
    Ok(())
}

#[test]
fn colon_spacing_flag_overrides_config_file() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    temp.child(".wsformat.toml")
        .write_str("colon_spacing = \"compact\"\n")?;
    let file = temp.child("script.ws");
    file.write_str(MESSY)?;

    wsformat()
        .current_dir(temp.path())
        .args(["--colon-spacing", "spaced"])
        .arg("script.ws")
        .assert()
        .success();

    file.assert(predicate::str::contains("var x : int;"));
    Ok(())
}

#[test]
fn annotation_placement_own_line_splits_annotation() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str("@addField(CClass) public var someField : bool;\n")?;

    wsformat()
        .args(["--annotation-placement", "ownLine"])
        .arg(file.path())
        .assert()
        .success();

    file.assert(predicate::str::contains(
        "@addField(CClass)\npublic var someField : bool;",
    ));
    Ok(())
}

#[test]
fn default_placement_own_line_splits_default() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(
        "class C { private const var RESET_TIME : float; default RESET_TIME = 0.750; }\n",
    )?;

    wsformat()
        .args(["--default-placement", "ownLine"])
        .arg(file.path())
        .assert()
        .success();

    file.assert(predicate::str::contains(
        "RESET_TIME : float;\n    default RESET_TIME = 0.750;",
    ));
    Ok(())
}

#[test]
fn invalid_colon_spacing_value_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let file = assert_fs::NamedTempFile::new("script.ws")?;
    file.write_str(MESSY)?;

    wsformat()
        .args(["--colon-spacing", "snug"])
        .arg(file.path())
        .assert()
        .failure();

    file.assert(MESSY);
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

// A subdir config wholly replaces the ancestor's (nearest wins), so reverting a key needs it restated.
#[test]
fn subdir_config_reverts_one_key_and_customizes_another() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = assert_fs::TempDir::new()?;
    temp.child(".wsformat.toml").write_str("tab_size = 8\n")?;
    temp.child("sub/.wsformat.toml")
        .write_str("tab_size = 4\ncolon_spacing = \"compact\"\n")?;
    temp.child("script.ws").write_str(MESSY)?;
    temp.child("sub/script.ws").write_str(MESSY)?;

    wsformat()
        .current_dir(temp.path())
        .arg(".")
        .assert()
        .success();

    let ancestor = std::fs::read_to_string(temp.child("script.ws").path())?;
    let subdir = std::fs::read_to_string(temp.child("sub/script.ws").path())?;
    expect![[r"
        --- ancestor (tab_size = 8) ---
        function f() {
                var x : int;
        }
        --- subdir (tab_size reverted to default 4, colon_spacing compact) ---
        function f() {
            var x: int;
        }
    "]]
    .assert_eq(&format!(
        "--- ancestor (tab_size = 8) ---\n{ancestor}--- subdir (tab_size reverted to default 4, colon_spacing compact) ---\n{subdir}"
    ));
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
