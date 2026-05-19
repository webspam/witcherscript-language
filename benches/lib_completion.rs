use criterion::{criterion_group, criterion_main, Criterion};
use witcherscript_language::builtins::load_builtins_index;
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{
    completion_members, statement_completions, SymbolDb, WorkspaceIndex,
};

#[path = "common/synth.rs"]
mod synth;

const TARGET_URI: &str = "file:///synth/target.ws";

fn build_workspace() -> (WorkspaceIndex, WorkspaceIndex, ParsedDocument) {
    let mut workspace = WorkspaceIndex::default();
    for (uri, source) in synth::synth_workspace(40) {
        let doc = parse_document(source).expect("synth source must parse");
        workspace.update_document(uri.to_string(), &doc);
    }
    let target_doc = parse_document(synth::synth_file(6, 6)).expect("synth source must parse");
    workspace.update_document(TARGET_URI, &target_doc);
    let base = load_builtins_index();
    (workspace, base, target_doc)
}

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
