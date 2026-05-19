use std::hint::black_box;

use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use witcherscript_language::builtins::load_builtins_index;
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{
    completion_members, find_references, resolve_definition, statement_completions, SymbolDb,
    WorkspaceIndex,
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

#[library_benchmark]
#[bench::small(2, 3)]
#[bench::medium(10, 6)]
#[bench::large(50, 10)]
fn bench_parse(classes: usize, methods: usize) {
    let source = synth::synth_file(classes, methods);
    black_box(parse_document(source).expect("synth source must parse"));
}

#[library_benchmark]
#[bench::small(10)]
#[bench::medium(100)]
#[bench::large(500)]
fn bench_index_build(num_files: usize) {
    let files: Vec<_> = synth::synth_workspace(num_files)
        .into_iter()
        .map(|(uri, source)| {
            let doc = parse_document(source).expect("synth source must parse");
            (uri.to_string(), doc)
        })
        .collect();
    let mut index = WorkspaceIndex::default();
    for (uri, doc) in &files {
        index.update_document(uri.clone(), doc);
    }
    black_box(index);
}

#[library_benchmark]
fn bench_resolve_definition() {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 1,
        character: 25,
    };
    black_box(resolve_definition(TARGET_URI, &target_doc, &db, pos).expect("must resolve Top0"));
}

#[library_benchmark]
fn bench_find_references() {
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
    let refs: Vec<_> = docs.iter().map(|(u, d)| (u.as_str(), d)).collect();

    black_box(find_references(&definition, &target_doc, &refs, &db, true));
}

#[library_benchmark]
fn bench_completion_members() {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 8,
        character: 9,
    };
    black_box(completion_members(TARGET_URI, &target_doc, &db, pos));
}

#[library_benchmark]
fn bench_statement_completions() {
    let (workspace, base, target_doc) = build_workspace();
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 7,
        character: 4,
    };
    black_box(statement_completions(TARGET_URI, &target_doc, &db, pos));
}

library_benchmark_group!(
    name = parse_group;
    benchmarks = bench_parse
);

library_benchmark_group!(
    name = index_group;
    benchmarks = bench_index_build
);

library_benchmark_group!(
    name = resolve_group;
    benchmarks = bench_resolve_definition, bench_find_references
);

library_benchmark_group!(
    name = completion_group;
    benchmarks = bench_completion_members, bench_statement_completions
);

main!(
    library_benchmark_groups = parse_group,
    index_group,
    resolve_group,
    completion_group
);
