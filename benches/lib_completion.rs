use criterion::{criterion_group, criterion_main, Criterion};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{completion_members, statement_completions, SymbolDb};

#[path = "common/synth.rs"]
mod synth;

use synth::{build_workspace, TARGET_URI};

fn bench_completion_members(c: &mut Criterion) {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 8,
        character: 9,
    };
    let canary = completion_members(TARGET_URI, &target_doc, &db, pos);
    assert!(
        !canary.is_empty(),
        "synth layout drifted: cursor no longer lands inside `this.<member>`"
    );

    c.bench_function("completion_members/this_dot", |b| {
        b.iter(|| completion_members(TARGET_URI, &target_doc, &db, pos));
    });
}

fn bench_statement_completions(c: &mut Criterion) {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 7,
        character: 4,
    };
    let canary = statement_completions(TARGET_URI, &target_doc, &db, pos);
    assert!(
        !canary.locals.is_empty() || !canary.members.is_empty() || !canary.globals.is_empty(),
        "synth layout drifted: cursor no longer lands inside a method body with visible symbols"
    );

    c.bench_function("statement_completions/in_method_body", |b| {
        b.iter(|| statement_completions(TARGET_URI, &target_doc, &db, pos));
    });
}

criterion_group!(
    benches,
    bench_completion_members,
    bench_statement_completions
);
criterion_main!(benches);
