use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use witcherscript_language::document::parse_document;

#[path = "common/synth.rs"]
mod synth;

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for (label, (num_classes, methods)) in [
        ("small", synth::FILE_SIZE_SMALL),
        ("medium", synth::FILE_SIZE_MEDIUM),
        ("large", synth::FILE_SIZE_LARGE),
    ] {
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
