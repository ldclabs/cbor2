//! Scenario: **`std`** — streaming through `std::io` reader/writer traits.
//!
//! This is the path you take with files and sockets. Encoding writes into a
//! *reused* buffer through an `io::Write`, modeling a server that keeps one
//! scratch buffer per connection (so, unlike the `alloc` scenario, the
//! allocator is not on the hot path). Decoding goes through an `io::Read`,
//! whose copying source is the distinctive `std` cost versus a borrowed
//! slice.
//!
//! Two crates have no streaming form and fall back to their slice APIs, which
//! is itself worth seeing: **ciborium** is reader-only (its `from_reader` is
//! the same call the other scenarios use), and **minicbor** is slice-only (it
//! has no `io::Read` decoder at all).

use std::hint::black_box;

use cbor2_bench::*;
use criterion::{criterion_group, criterion_main, Criterion};
use serde_bytes::ByteBuf;

fn bench_encode(c: &mut Criterion) {
    macro_rules! encode_group {
        ($name:literal, $serde:expr, $mini:expr) => {{
            let data = $serde;
            let mini = $mini;
            let mut g = c.benchmark_group($name);
            g.bench_function("cbor2", |b| {
                let mut buf = Vec::new();
                b.iter(|| {
                    buf.clear();
                    cbor2::to_writer(black_box(&data), &mut buf).unwrap();
                    black_box(buf.len())
                })
            });
            g.bench_function("ciborium", |b| {
                let mut buf = Vec::new();
                b.iter(|| {
                    buf.clear();
                    ciborium::into_writer(black_box(&data), &mut buf).unwrap();
                    black_box(buf.len())
                })
            });
            g.bench_function("serde_cbor", |b| {
                let mut buf = Vec::new();
                b.iter(|| {
                    buf.clear();
                    serde_cbor::to_writer(&mut buf, black_box(&data)).unwrap();
                    black_box(buf.len())
                })
            });
            g.bench_function("cbor4ii", |b| {
                let mut buf = Vec::new();
                b.iter(|| {
                    buf.clear();
                    cbor4ii::serde::to_writer(&mut buf, black_box(&data)).unwrap();
                    black_box(buf.len())
                })
            });
            g.bench_function("minicbor", |b| {
                let mut buf = Vec::new();
                b.iter(|| {
                    buf.clear();
                    minicbor::encode(black_box(&mini), &mut buf).unwrap();
                    black_box(buf.len())
                })
            });
            g.finish();
        }};
    }

    let logs = log_batch(LOG_BATCH_LEN);
    let logs_mini = log_batch_mini(&logs);
    let raw = blob(BLOB_LEN);

    encode_group!(
        "std/encode/int_array",
        int_array(INT_ARRAY_LEN),
        int_array(INT_ARRAY_LEN)
    );
    encode_group!("std/encode/log_batch", logs, logs_mini);
    encode_group!(
        "std/encode/blob",
        ByteBuf::from(raw.clone()),
        minicbor::bytes::ByteVec::from(raw)
    );
}

fn bench_decode(c: &mut Criterion) {
    macro_rules! decode_group {
        ($name:literal, $ty:ty, $mty:ty, $serde:expr, $mini:expr) => {{
            let value = $serde;
            let mini = $mini;
            // Each decoder reads bytes it produced itself: crates differ in
            // preferred encoding (e.g. cbor2 narrows floats to f32, which
            // cbor4ii's decoder rejects for an f64 field), so a shared buffer
            // is not portable.
            let b_cbor2 = cbor2::to_vec(&value).unwrap();
            let b_ciborium = {
                let mut v = Vec::new();
                ciborium::into_writer(&value, &mut v).unwrap();
                v
            };
            let b_serde = serde_cbor::to_vec(&value).unwrap();
            let b_cbor4ii = cbor4ii::serde::to_vec(Vec::new(), &value).unwrap();
            let b_mini = minicbor::to_vec(&mini).unwrap();
            let mut g = c.benchmark_group($name);
            g.bench_function("cbor2", |x| {
                x.iter(|| cbor2::from_reader::<$ty, _>(black_box(&b_cbor2[..])).unwrap())
            });
            g.bench_function("ciborium", |x| {
                x.iter(|| ciborium::from_reader::<$ty, _>(black_box(&b_ciborium[..])).unwrap())
            });
            g.bench_function("serde_cbor", |x| {
                x.iter(|| serde_cbor::from_reader::<$ty, _>(black_box(&b_serde[..])).unwrap())
            });
            g.bench_function("cbor4ii", |x| {
                x.iter(|| cbor4ii::serde::from_reader::<$ty, _>(black_box(&b_cbor4ii[..])).unwrap())
            });
            g.bench_function("minicbor", |x| {
                x.iter(|| minicbor::decode::<$mty>(black_box(&b_mini)).unwrap())
            });
            g.finish();
        }};
    }

    let logs = log_batch(LOG_BATCH_LEN);
    let logs_mini = log_batch_mini(&logs);
    let raw = blob(BLOB_LEN);

    decode_group!(
        "std/decode/int_array",
        Vec<u64>,
        Vec<u64>,
        int_array(INT_ARRAY_LEN),
        int_array(INT_ARRAY_LEN)
    );
    decode_group!(
        "std/decode/log_batch",
        Vec<LogEntry>,
        Vec<LogEntryMini>,
        logs,
        logs_mini
    );
    decode_group!(
        "std/decode/blob",
        ByteBuf,
        minicbor::bytes::ByteVec,
        ByteBuf::from(raw.clone()),
        minicbor::bytes::ByteVec::from(raw)
    );
}

criterion_group!(benches, bench_encode, bench_decode);
criterion_main!(benches);
