use criterion::{criterion_group, criterion_main, Criterion};
use witcherscript_language::document::parse_document;
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{
    completion_members, statement_completions, SymbolDb, WorkspaceIndex,
};

#[path = "common/synth.rs"]
mod synth;

use synth::{build_workspace, synth_workspace, TARGET_URI};

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
        canary.active
            && (!canary.locals.is_empty() || !canary.members.is_empty() || canary.needs_globals),
        "synth layout drifted: cursor no longer lands inside a method body with visible symbols"
    );

    c.bench_function("statement_completions/in_method_body", |b| {
        b.iter(|| statement_completions(TARGET_URI, &target_doc, &db, pos));
    });
}

fn build_large_base_workspace() -> (
    WorkspaceIndex,
    WorkspaceIndex,
    witcherscript_language::document::ParsedDocument,
) {
    let mut base = WorkspaceIndex::default();
    base.begin_bulk_catalog_update();
    for (uri, source) in synth_workspace(500) {
        let doc = parse_document(source).expect("synth source must parse");
        base.update_document(uri.to_string(), &doc);
    }
    base.end_bulk_catalog_update();

    let mut workspace = WorkspaceIndex::default();
    let target_source = synth::synth_file(6, 6);
    let target_doc = parse_document(target_source).expect("synth target must parse");
    workspace.update_document(TARGET_URI.to_string(), &target_doc);
    (workspace, base, target_doc)
}

fn bench_statement_completions_large_base(c: &mut Criterion) {
    let (workspace, base, target_doc) = build_large_base_workspace();
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 7,
        character: 4,
    };
    let canary = statement_completions(TARGET_URI, &target_doc, &db, pos);
    assert!(canary.active);

    c.bench_function("statement_completions/large_base", |b| {
        b.iter(|| statement_completions(TARGET_URI, &target_doc, &db, pos));
    });
}

fn bench_statement_completions_keyword_large_base(c: &mut Criterion) {
    let (workspace, base, _) = build_large_base_workspace();
    let target_doc =
        parse_document("class Class0 {\n  function method0(arg: int) : int {\n    if$0\n  }\n}\n")
            .expect("keyword fixture must parse");
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 2,
        character: 6,
    };
    let canary = statement_completions(TARGET_URI, &target_doc, &db, pos);
    assert!(canary.active && !canary.needs_globals);

    c.bench_function("statement_completions/keyword_large_base", |b| {
        b.iter(|| statement_completions(TARGET_URI, &target_doc, &db, pos));
    });
}

criterion_group!(
    benches,
    bench_completion_members,
    bench_statement_completions,
    bench_statement_completions_large_base,
    bench_statement_completions_keyword_large_base
);
criterion_main!(benches);
