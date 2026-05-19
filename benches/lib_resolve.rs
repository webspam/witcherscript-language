use criterion::{criterion_group, criterion_main, Criterion};
use witcherscript_language::builtins::load_builtins_index;
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{
    find_references, resolve_definition, SymbolDb, WorkspaceIndex,
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
    let target_source = synth::synth_file(6, 6);
    let target_doc = parse_document(target_source).expect("synth source must parse");
    workspace.update_document(TARGET_URI, &target_doc);

    let base = load_builtins_index();
    (workspace, base, target_doc)
}

fn bench_resolve(c: &mut Criterion) {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    // The Top0(1, 2) callsite sits on line 1 of every synth file at character 25.
    let pos = SourcePosition {
        line: 1,
        character: 25,
    };

    c.bench_function("resolve_definition/callsite", |b| {
        b.iter(|| {
            resolve_definition(TARGET_URI, &target_doc, &db, pos).expect("must resolve Top0")
        });
    });
}

fn bench_find_references(c: &mut Criterion) {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 1,
        character: 25,
    };
    let definition = resolve_definition(TARGET_URI, &target_doc, &db, pos)
        .expect("must resolve Top0 for references bench");

    let mut docs: Vec<(String, ParsedDocument)> = Vec::new();
    for (uri, source) in synth::synth_workspace(40) {
        let doc = parse_document(source).expect("synth source must parse");
        docs.push((uri.to_string(), doc));
    }
    let target_clone = parse_document(synth::synth_file(6, 6)).expect("synth re-parse");
    docs.push((TARGET_URI.to_string(), target_clone));

    c.bench_function("find_references/top_level_callable", |b| {
        b.iter(|| {
            let refs: Vec<_> = docs.iter().map(|(u, d)| (u.as_str(), d)).collect();
            find_references(&definition, &target_doc, &refs, &db, true)
        });
    });
}

criterion_group!(benches, bench_resolve, bench_find_references);
criterion_main!(benches);
