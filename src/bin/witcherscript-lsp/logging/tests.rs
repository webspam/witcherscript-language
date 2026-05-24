use super::*;

#[test]
fn own_targets_pass_dependency_targets_are_rejected() {
    assert!(is_own_target("witcherscript_lsp"));
    assert!(is_own_target("witcherscript_lsp::indexing"));
    assert!(is_own_target("witcherscript_language::resolve"));
    assert!(!is_own_target("async_lsp::router"));
    assert!(!is_own_target("hyper::proto"));
    assert!(!is_own_target("witcherscript_lsp_extra"));
}

#[test]
fn utc_timestamp_has_millisecond_granularity() {
    let ts = utc_timestamp();
    assert_eq!(ts.len(), 12, "expected HH:MM:SS.mmm, got {ts}");
    let (time, millis) = ts.split_once('.').expect("missing millisecond component");
    assert_eq!(millis.len(), 3);
    assert!(millis.chars().all(|c| c.is_ascii_digit()));
    let parts: Vec<&str> = time.split(':').collect();
    assert_eq!(parts.len(), 3);
    assert!(parts
        .iter()
        .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit())));
}

#[test]
fn level_u8_round_trip_covers_every_level() {
    for level in [
        tracing::Level::ERROR,
        tracing::Level::WARN,
        tracing::Level::INFO,
        tracing::Level::DEBUG,
        tracing::Level::TRACE,
    ] {
        assert_eq!(
            level_from_u8(level_to_u8(level)),
            level,
            "round-trip lost level {level}"
        );
    }
}
