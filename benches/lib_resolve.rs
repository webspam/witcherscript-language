use criterion::{Criterion, criterion_group, criterion_main};
use witcherscript_language::document::{ParsedDocument, parse_document};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::{SymbolDb, find_references, resolve_definition};

#[path = "common/synth.rs"]
mod synth;

use synth::{TARGET_URI, build_workspace};

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
