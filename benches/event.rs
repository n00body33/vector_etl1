use bytes::Bytes;
use criterion::{criterion_group, BatchSize, Criterion};
use serde_json::{json, Value};
use vector::{
    config::log_schema,
    event::{Event, LogEvent},
    transforms::{FunctionTransform, OutputBuffer},
};

fn benchmark_event_iterate(c: &mut Criterion) {
    let mut group = c.benchmark_group("event/iterate");

    group.bench_function("single-level", |b| {
        b.iter_batched_ref(
            || {
                let mut log = LogEvent::new();
                log.insert("key1", Bytes::from("value1"));
                log.insert("key2", Bytes::from("value2"));
                log.insert("key3", Bytes::from("value3"));
                log
            },
            |e| e.all_fields().unwrap().count(),
            BatchSize::SmallInput,
        )
    });

    group.bench_function("nested-keys", |b| {
        b.iter_batched_ref(
            || {
                let mut log = Event::new_empty_log().into_log();
                log.insert("key1.nested1.nested2", Bytes::from("value1"));
                log.insert("key1.nested1.nested3", Bytes::from("value4"));
                log.insert("key3", Bytes::from("value3"));
                log
            },
            |e| e.all_fields().unwrap().count(),
            BatchSize::SmallInput,
        )
    });

    group.bench_function("array", |b| {
        b.iter_batched_ref(
            || {
                let mut log = Event::new_empty_log().into_log();
                log.insert("key1.nested1[0]", Bytes::from("value1"));
                log.insert("key1.nested1[1]", Bytes::from("value2"));
                log
            },
            |e| e.all_fields().unwrap().count(),
            BatchSize::SmallInput,
        )
    });
}

fn benchmark_event_create(c: &mut Criterion) {
    let mut group = c.benchmark_group("event/create");

    group.bench_function("single-level", |b| {
        b.iter(|| {
            let mut log = Event::new_empty_log().into_log();
            log.insert("key1", Bytes::from("value1"));
            log.insert("key2", Bytes::from("value2"));
            log.insert("key3", Bytes::from("value3"));
        })
    });

    group.bench_function("nested-keys", |b| {
        b.iter(|| {
            let mut log = Event::new_empty_log().into_log();
            log.insert("key1.nested1.nested2", Bytes::from("value1"));
            log.insert("key1.nested1.nested3", Bytes::from("value4"));
            log.insert("key3", Bytes::from("value3"));
        })
    });
    group.bench_function("array", |b| {
        b.iter(|| {
            let mut log = Event::new_empty_log().into_log();
            log.insert("key1.nested1[0]", Bytes::from("value1"));
            log.insert("key1.nested1[1]", Bytes::from("value2"));
        })
    });
}

criterion_group!(
    name = benches;
    // encapsulates inherent CI noise we saw in
    // https://github.com/vectordotdev/vector/issues/5394
    config = Criterion::default().noise_threshold(0.05);
    targets = benchmark_event_create, benchmark_event_iterate
);
