use criterion::{criterion_group, criterion_main, Criterion};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{completion_members, statement_completions, SymbolDb};

#[path = "common/synth.rs"]
mod synth;

use synth::{build_workspace, TARGET_URI};

fn bench_completion_members(c: &mut Criterion) {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    // Line 8 is `    this.field0_a = local0;` in method0; char 9 is just past the dot.
    let pos = SourcePosition {
        line: 8,
        character: 9,
    };
    c.bench_function("completion_members/this_dot", |b| {
        b.iter(|| completion_members(TARGET_URI, &target_doc, &db, pos));
    });
}

fn bench_statement_completions(c: &mut Criterion) {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    // Line 7 is `    var local0 : int = arg;`; char 4 is the start of the statement.
    let pos = SourcePosition {
        line: 7,
        character: 4,
    };
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
