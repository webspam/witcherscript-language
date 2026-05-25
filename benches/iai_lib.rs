use std::hint::black_box;

use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use witcherscript_language::document::{parse_document, ParsedDocument};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{
    completion_members, find_references, resolve_definition, statement_completions, Definition,
    SymbolDb, WorkspaceIndex,
};

#[path = "common/synth.rs"]
mod synth;

use synth::{build_workspace, WorkspaceFixture, TARGET_URI};

type FindRefsFixture = (
    WorkspaceIndex,
    WorkspaceIndex,
    ParsedDocument,
    Vec<(String, ParsedDocument)>,
    Definition,
);
type IndexFixture = Vec<(String, ParsedDocument)>;

fn build_find_refs_fixture() -> FindRefsFixture {
    let (workspace, base, target_doc) = build_workspace();
    let definition = {
        let db = SymbolDb::new(&workspace, &base);
        let pos = SourcePosition {
            line: 1,
            character: 25,
        };
        resolve_definition(TARGET_URI, &target_doc, &db, pos)
            .expect("must resolve Top0 for references bench")
    };

    let mut docs: Vec<(String, ParsedDocument)> = Vec::new();
    for (uri, source) in synth::synth_workspace(40) {
        let doc = parse_document(source).expect("synth source must parse");
        docs.push((uri.to_string(), doc));
    }
    let target_clone = parse_document(synth::synth_file(6, 6)).expect("synth re-parse");
    docs.push((TARGET_URI.to_string(), target_clone));

    (workspace, base, target_doc, docs, definition)
}

fn synth_source_small() -> String {
    let (c, m) = synth::FILE_SIZE_SMALL;
    synth::synth_file(c, m)
}
fn synth_source_medium() -> String {
    let (c, m) = synth::FILE_SIZE_MEDIUM;
    synth::synth_file(c, m)
}
fn synth_source_large() -> String {
    let (c, m) = synth::FILE_SIZE_LARGE;
    synth::synth_file(c, m)
}

fn parse_workspace_small() -> IndexFixture {
    parse_workspace(synth::WORKSPACE_SIZE_SMALL)
}
fn parse_workspace_medium() -> IndexFixture {
    parse_workspace(synth::WORKSPACE_SIZE_MEDIUM)
}
fn parse_workspace_large() -> IndexFixture {
    parse_workspace(synth::WORKSPACE_SIZE_LARGE)
}

fn parse_workspace(num_files: usize) -> IndexFixture {
    synth::synth_workspace(num_files)
        .into_iter()
        .map(|(uri, source)| {
            let doc = parse_document(source).expect("synth source must parse");
            (uri.to_string(), doc)
        })
        .collect()
}

#[library_benchmark]
#[bench::small(setup = synth_source_small)]
#[bench::medium(setup = synth_source_medium)]
#[bench::large(setup = synth_source_large)]
fn bench_parse(source: String) {
    black_box(parse_document(source).expect("synth source must parse"));
}

#[library_benchmark]
#[bench::small(setup = parse_workspace_small)]
#[bench::medium(setup = parse_workspace_medium)]
#[bench::large(setup = parse_workspace_large)]
fn bench_index_build(files: IndexFixture) {
    let mut index = WorkspaceIndex::default();
    for (uri, doc) in &files {
        index.update_document(uri.clone(), doc);
    }
    black_box(index);
}

#[library_benchmark]
#[bench::main(setup = build_workspace)]
fn bench_resolve_definition(fixture: WorkspaceFixture) {
    let (workspace, base, target_doc) = fixture;
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 1,
        character: 25,
    };
    black_box(resolve_definition(TARGET_URI, &target_doc, &db, pos).expect("must resolve Top0"));
}

#[library_benchmark]
#[bench::main(setup = build_find_refs_fixture)]
fn bench_find_references(fixture: FindRefsFixture) {
    let (workspace, base, target_doc, docs, definition) = fixture;
    let db = SymbolDb::new(&workspace, &base);
    let refs: Vec<_> = docs.iter().map(|(u, d)| (u.as_str(), d)).collect();
    black_box(find_references(&definition, &target_doc, &refs, &db, true));
}

#[library_benchmark]
#[bench::main(setup = build_workspace)]
fn bench_completion_members(fixture: WorkspaceFixture) {
    let (workspace, base, target_doc) = fixture;
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 8,
        character: 9,
    };
    let result = completion_members(TARGET_URI, &target_doc, &db, pos);
    assert!(
        !result.is_empty(),
        "synth layout drifted: cursor no longer lands inside `this.<member>`"
    );
    black_box(result);
}

#[library_benchmark]
#[bench::main(setup = build_workspace)]
fn bench_statement_completions(fixture: WorkspaceFixture) {
    let (workspace, base, target_doc) = fixture;
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 7,
        character: 4,
    };
    let result = statement_completions(TARGET_URI, &target_doc, &db, pos);
    assert!(
        result.active
            && (!result.locals.is_empty() || !result.members.is_empty() || result.needs_globals),
        "synth layout drifted: cursor no longer lands inside a method body with visible symbols"
    );
    black_box(result);
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

fn build_large_base_fixture() -> WorkspaceFixture {
    let mut base = WorkspaceIndex::default();
    base.begin_bulk_catalog_update();
    for (uri, source) in synth::synth_workspace(500) {
        let doc = parse_document(source).expect("synth source must parse");
        base.update_document(uri.to_string(), &doc);
    }
    base.end_bulk_catalog_update();

    let target_doc = parse_document(synth::synth_file(6, 6)).expect("synth target must parse");
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document(TARGET_URI.to_string(), &target_doc);
    (workspace, base, target_doc)
}

#[library_benchmark]
#[bench::main(setup = build_large_base_fixture)]
fn bench_statement_completions_large_base(fixture: WorkspaceFixture) {
    let (workspace, base, target_doc) = fixture;
    let db = SymbolDb::new(&workspace, &base);
    let pos = SourcePosition {
        line: 7,
        character: 4,
    };
    black_box(statement_completions(TARGET_URI, &target_doc, &db, pos));
}

library_benchmark_group!(
    name = completion_group;
    benchmarks = bench_completion_members, bench_statement_completions,
        bench_statement_completions_large_base
);

main!(
    library_benchmark_groups = parse_group,
    index_group,
    resolve_group,
    completion_group
);
