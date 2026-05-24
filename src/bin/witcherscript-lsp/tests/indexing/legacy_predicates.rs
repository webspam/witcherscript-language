use std::collections::{HashMap, HashSet};

use lsp_types::{FileChangeType, FileEvent, Url};

use crate::indexing::{legacy_base_replacements, legacy_replaces_base};
use crate::watcher::event_touches_legacy_dir;

use super::legacy_helpers::LocalTempDir;

#[test]
fn legacy_replaces_base_matches_same_relpath() {
    assert!(legacy_replaces_base(
        "file:///game/content/content0/scripts/game/r4Player.ws",
        "file:///mod/legacy/game/r4Player.ws",
    ));
}

#[test]
fn legacy_replaces_base_requires_path_separator() {
    assert!(!legacy_replaces_base(
        "file:///game/content/content0/scripts/game/r4Player.ws",
        "file:///mod/legacy/Xgame/r4Player.ws",
    ));
}

#[test]
fn legacy_replaces_base_skips_base_without_scripts_segment() {
    assert!(!legacy_replaces_base(
        "file:///game/r4Player.ws",
        "file:///mod/legacy/r4Player.ws",
    ));
}

#[test]
fn legacy_replaces_base_basename_only_no_match() {
    assert!(!legacy_replaces_base(
        "file:///game/content/content0/scripts/game/r4Player.ws",
        "file:///mod/legacy/r4Player.ws",
    ));
}

#[test]
fn event_touches_legacy_dir_true_inside() {
    let temp = LocalTempDir::new("ws_event_touches_legacy_dir_true_inside");
    let file = temp.path().join("game").join("r4Player.ws");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "").unwrap();
    let event = FileEvent {
        uri: Url::from_file_path(&file).unwrap(),
        typ: FileChangeType::CHANGED,
    };
    assert!(event_touches_legacy_dir(
        &event,
        &[temp.path().to_path_buf()]
    ));
}

#[test]
fn event_touches_legacy_dir_false_outside() {
    let temp = LocalTempDir::new("ws_event_touches_legacy_dir_false_outside");
    let legacy = temp.path().join("legacy");
    std::fs::create_dir_all(&legacy).unwrap();
    let elsewhere = temp.path().join("workspace").join("foo.ws");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::write(&elsewhere, "").unwrap();
    let event = FileEvent {
        uri: Url::from_file_path(&elsewhere).unwrap(),
        typ: FileChangeType::CHANGED,
    };
    assert!(!event_touches_legacy_dir(&event, &[legacy]));
}

#[test]
fn event_touches_legacy_dir_empty_dirs_returns_false() {
    let temp = LocalTempDir::new("ws_event_touches_legacy_dir_empty_dirs_returns_false");
    let file = temp.path().join("foo.ws");
    std::fs::write(&file, "").unwrap();
    let event = FileEvent {
        uri: Url::from_file_path(&file).unwrap(),
        typ: FileChangeType::CHANGED,
    };
    assert!(!event_touches_legacy_dir(&event, &[]));
}

#[test]
fn legacy_base_replacements_maps_only_real_overrides() {
    struct Case {
        name: &'static str,
        base: &'static [&'static str],
        legacy: &'static [&'static str],
        expect_skip: &'static [&'static str],
        expect_map: &'static [(&'static str, &'static str)],
    }
    let cases = [
        Case {
            name: "legacy file at the same game-relative path replaces the base script",
            base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            legacy: &["file:///mod/legacy/game/r4Player.ws"],
            expect_skip: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            expect_map: &[("file:///mod/legacy/game/r4Player.ws", "game/r4Player.ws")],
        },
        Case {
            name: "brand-new script in a legacy folder replaces nothing",
            base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            legacy: &["file:///mod/legacy/game/MyNewMod.ws"],
            expect_skip: &[],
            expect_map: &[],
        },
        Case {
            name: "same basename but a different relative path replaces nothing",
            base: &["file:///game/content/content0/scripts/game/r4Player.ws"],
            legacy: &["file:///mod/legacy/other/r4Player.ws"],
            expect_skip: &[],
            expect_map: &[],
        },
    ];
    for c in cases {
        let base: Vec<String> = c.base.iter().map(|s| s.to_string()).collect();
        let legacy: Vec<String> = c.legacy.iter().map(|s| s.to_string()).collect();
        let (skip, map) = legacy_base_replacements(&base, &legacy);
        let expect_skip: HashSet<String> = c.expect_skip.iter().map(|s| s.to_string()).collect();
        let expect_map: HashMap<String, String> = c
            .expect_map
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(skip, expect_skip, "case '{}': skip set mismatch", c.name);
        assert_eq!(
            map, expect_map,
            "case '{}': replacement map mismatch",
            c.name
        );
    }
}
