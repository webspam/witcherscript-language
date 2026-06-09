use rstest::rstest;

use super::*;
use crate::types::Type;

fn write_temp(name: &str, content: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn parses_globals_section() {
    let path = write_temp(
        "se_test1.ini",
        "[globals]\ntheGame=CR4Game\nthePlayer=CR4Player\n",
    );
    let env = parse_script_environment(&path).unwrap();
    assert_eq!(env.find("theGame").unwrap().type_name, "CR4Game");
    assert_eq!(env.find("thePlayer").unwrap().type_name, "CR4Player");
}

#[test]
fn skips_other_sections_and_comments() {
    let path = write_temp(
        "se_test2.ini",
        "[other]\nfoo=Bar\n[globals]\n; skip\ntheGame=CR4Game\n[more]\nbaz=Qux\n",
    );
    let env = parse_script_environment(&path).unwrap();
    assert!(env.find("theGame").is_some());
    assert!(env.find("foo").is_none());
    assert!(env.find("baz").is_none());
}

#[test]
fn symbol_has_correct_position() {
    let path = write_temp("se_test3.ini", "[globals]\ntheGame=CR4Game\n");
    let env = parse_script_environment(&path).unwrap();
    let sym = &env.find("theGame").unwrap().symbol;
    assert_eq!(sym.selection_range.start.line, 1);
    assert_eq!(sym.selection_range.start.character, 0);
    assert_eq!(sym.type_annotation, Some(Type::from_annotation("CR4Game")));
    assert_eq!(sym.kind, SymbolKind::Variable);
}

#[test]
fn camera_injected_when_absent_from_ini() {
    let path = write_temp("se_camera1.ini", "[globals]\ntheGame=CR4Game\n");
    let env = parse_script_environment(&path).unwrap();
    let camera = env.find("theCamera").expect("theCamera injected");
    assert_eq!(camera.type_name, "CCameraDirector");
    assert_eq!(
        camera.symbol.type_annotation,
        Some(Type::from_annotation("CCameraDirector"))
    );
}

#[rstest]
#[case::stock_ccamera_entry_is_overridden("se_camera_stock.ini", "CCamera", "CCameraDirector")]
#[case::mod_retyped_is_left_untouched("se_camera_modded.ini", "MyCustomCamera", "MyCustomCamera")]
fn camera_override_respects_ini_state(
    #[case] file: &str,
    #[case] ini_type: &str,
    #[case] expected: &str,
) {
    let path = write_temp(file, &format!("[globals]\ntheCamera={ini_type}\n"));
    let env = parse_script_environment(&path).unwrap();
    let camera = env.find("theCamera").unwrap();
    assert_eq!(camera.type_name, expected);
    assert_eq!(
        camera.symbol.type_annotation,
        Some(Type::from_annotation(expected))
    );
}

#[test]
fn telemetry_is_appended_even_without_ini_entry() {
    let path = write_temp("se_telemetry1.ini", "[globals]\ntheGame=CR4Game\n");
    let env = parse_script_environment(&path).unwrap();
    let tel = env.find("theTelemetry").expect("theTelemetry injected");
    assert_eq!(tel.type_name, "CR4TelemetryScriptProxy");
    assert_eq!(tel.symbol.kind, SymbolKind::Variable);
}

#[test]
fn telemetry_ini_entry_is_not_overwritten() {
    let path = write_temp(
        "se_telemetry2.ini",
        "[globals]\ntheTelemetry=SomeOtherTelemetry\n",
    );
    let env = parse_script_environment(&path).unwrap();
    let matches: Vec<_> = env
        .globals
        .iter()
        .filter(|g| g.name == "theTelemetry")
        .collect();
    assert_eq!(matches.len(), 1, "should not duplicate theTelemetry");
    assert_eq!(matches[0].type_name, "SomeOtherTelemetry");
}
