use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use witcherscript_language::document::parse_document;
use witcherscript_language::line_index::LineIndex;
use witcherscript_language::symbols::extract_symbols;

#[path = "common/synth.rs"]
mod synth;

fn bench_symbols(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbols");
    for (label, (num_classes, methods)) in [
        ("small", synth::FILE_SIZE_SMALL),
        ("medium", synth::FILE_SIZE_MEDIUM),
        ("large", synth::FILE_SIZE_LARGE),
    ] {
        let source = synth::synth_file(num_classes, methods);
        let document = parse_document(source.clone()).expect("synth source must parse");
        let line_index = LineIndex::new(&document.source);
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(document, line_index),
            |b, (doc, idx)| {
                b.iter(|| extract_symbols(doc.tree.root_node(), &doc.source, idx));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_symbols);
criterion_main!(benches);
