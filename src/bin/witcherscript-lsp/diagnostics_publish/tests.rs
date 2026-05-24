use super::duplicates_not_explained_by_conflict;
use witcherscript_language::diagnostics::{Severity, WorkspaceDiagnostic};
use witcherscript_language::line_index::{SourcePosition, SourceRange};

fn diag(kind: &str, line: u32) -> WorkspaceDiagnostic {
    let pos = SourcePosition { line, character: 0 };
    WorkspaceDiagnostic {
        kind: kind.to_string(),
        message: String::new(),
        severity: Severity::Error,
        range: SourceRange {
            start: pos,
            end: pos,
        },
        related: vec![],
        data: None,
    }
}

#[test]
fn drops_duplicate_where_a_conflict_covers_the_same_declaration() {
    let dups = vec![diag("duplicate_symbol", 0), diag("duplicate_symbol", 5)];
    let conflicts = vec![diag("base_script_conflict", 0)];
    let kept: Vec<u32> = duplicates_not_explained_by_conflict(&dups, &conflicts)
        .map(|d| d.range.start.line)
        .collect();
    assert_eq!(
        kept,
        vec![5],
        "the duplicate at the conflict's declaration is suppressed"
    );
}

#[test]
fn keeps_every_duplicate_when_there_are_no_conflicts() {
    let dups = vec![diag("duplicate_symbol", 0), diag("duplicate_symbol", 5)];
    let kept = duplicates_not_explained_by_conflict(&dups, &[]).count();
    assert_eq!(kept, 2);
}
