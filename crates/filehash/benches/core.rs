//! Hashing throughput microbenchmarks (in-memory, no I/O).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use filehash::algo::{Algorithm, digest};
use std::hint::black_box;

fn bench(c: &mut Criterion) {
    let data = vec![0xA5u8; 1 << 20];
    let mut g = c.benchmark_group("digest_1MiB");
    g.throughput(Throughput::Bytes(data.len() as u64));
    for algo in Algorithm::ALL {
        g.bench_function(algo.to_string(), |b| {
            b.iter(|| digest(algo, black_box(&data)))
        });
    }
    g.finish();
    c.bench_function("manifest/parse_line", |b| {
        b.iter(|| filehash::manifest::parse_line(black_box("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  some/dir/file name.bin")))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
