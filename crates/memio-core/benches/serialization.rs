//! Benchmark for serialization.

use criterion::{criterion_group, criterion_main, Criterion};
use memio_core::MemioState;
use rkyv::{Archive, Serialize, Deserialize};

#[derive(Archive, Serialize, Deserialize)]
struct TestData {
    id: u64,
    name: String,
    values: Vec<f64>,
}

fn benchmark_serialization(c: &mut Criterion) {
    let data = TestData {
        id: 42,
        name: "Example".to_string(),
        values: vec![0.1, 0.2, 0.3, 0.4],
    };

    let state = MemioState::new(data);

    c.bench_function("serialize state", |b| {
        b.iter(|| {
            let _ = state.to_bytes().unwrap();
        });
    });
}

criterion_group!(benches, benchmark_serialization);
criterion_main!(benches);
