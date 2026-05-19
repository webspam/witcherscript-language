use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use witcherscript_language::document::parse_document;
use witcherscript_language::resolve::WorkspaceIndex;

#[path = "common/synth.rs"]
mod synth;

fn bench_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_build");
    for size in [10usize, 100, 500] {
        let files: Vec<_> = synth::synth_workspace(size)
            .into_iter()
            .map(|(uri, source)| {
                let document = parse_document(source).expect("synth source must parse");
                (uri.to_string(), document)
            })
            .collect();
        group.bench_with_input(BenchmarkId::from_parameter(size), &files, |b, fixtures| {
            b.iter(|| {
                let mut index = WorkspaceIndex::default();
                for (uri, doc) in fixtures {
                    index.update_document(uri.clone(), doc);
                }
                index
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_index_build);
criterion_main!(benches);
