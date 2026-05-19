use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use witcherscript_language::document::parse_document;

#[path = "common/synth.rs"]
mod synth;

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for (label, num_classes, methods) in [("small", 2, 3), ("medium", 10, 6), ("large", 50, 10)] {
        let source = synth::synth_file(num_classes, methods);
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &source, |b, src| {
            b.iter(|| parse_document(src.as_str()).expect("synth source must parse"));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
