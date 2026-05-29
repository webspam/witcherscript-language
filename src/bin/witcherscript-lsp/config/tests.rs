use super::{ConfigChange, DiagnosticsScope};

#[test]
fn diagnostics_scope_parses_setting_string() {
    struct Case {
        name: &'static str,
        input: &'static str,
        expected: DiagnosticsScope,
    }
    let cases = [
        Case {
            name: "explicit openFiles",
            input: "openFiles",
            expected: DiagnosticsScope::OpenFiles,
        },
        Case {
            name: "explicit workspace",
            input: "workspace",
            expected: DiagnosticsScope::Workspace,
        },
        Case {
            name: "explicit none",
            input: "none",
            expected: DiagnosticsScope::None,
        },
        Case {
            name: "unknown value falls back to workspace",
            input: "garbage",
            expected: DiagnosticsScope::Workspace,
        },
    ];
    for c in cases {
        assert_eq!(
            DiagnosticsScope::from_setting(c.input),
            c.expected,
            "case {}: scope mismatch",
            c.name
        );
    }
}

#[test]
fn config_change_default_is_no_op() {
    let c = ConfigChange::default();
    struct Case {
        name: &'static str,
        change: ConfigChange,
        expect_any_action: bool,
    }
    let cases = [
        Case {
            name: "default → nothing to do",
            change: c,
            expect_any_action: false,
        },
        Case {
            name: "reindex only",
            change: ConfigChange {
                needs_reindex: true,
                diagnostics_changed: false,
                ..ConfigChange::default()
            },
            expect_any_action: true,
        },
        Case {
            name: "diagnostics change only",
            change: ConfigChange {
                needs_reindex: false,
                diagnostics_changed: true,
                ..ConfigChange::default()
            },
            expect_any_action: true,
        },
        Case {
            name: "both at once",
            change: ConfigChange {
                needs_reindex: true,
                diagnostics_changed: true,
                ..ConfigChange::default()
            },
            expect_any_action: true,
        },
    ];
    for c in cases {
        let any = c.change.needs_reindex || c.change.diagnostics_changed;
        assert_eq!(
            any, c.expect_any_action,
            "case {}: action predicate wrong",
            c.name
        );
    }
}
