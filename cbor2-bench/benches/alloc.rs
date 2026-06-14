//! Scenario: **`no_std` + `alloc`** — the in-memory heap-buffer API path.
//!
//! Encoding produces a fresh `Vec<u8>`; decoding reads back from a `&[u8]`.
//! This is the canonical apples-to-apples comparison: every one of the five
//! crates supports it, and it is what you would write in a `no_std + alloc`
//! target such as a wasm32 canister or an `alloc`-only embedded runtime.

use std::hint::black_box;

use cbor2_bench::*;
use criterion::{criterion_group, criterion_main, Criterion};
use serde_bytes::ByteBuf;

// ---- encoders: value -> fresh Vec<u8> ----

fn enc_cbor2<T: serde::Serialize>(v: &T) -> Vec<u8> {
    cbor2::to_vec(v).unwrap()
}
fn enc_ciborium<T: serde::Serialize>(v: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::into_writer(v, &mut buf).unwrap();
    buf
}
fn enc_serde_cbor<T: serde::Serialize>(v: &T) -> Vec<u8> {
    serde_cbor::to_vec(v).unwrap()
}
fn enc_cbor4ii<T: serde::Serialize>(v: &T) -> Vec<u8> {
    cbor4ii::serde::to_vec(Vec::new(), v).unwrap()
}

fn bench_encode(c: &mut Criterion) {
    {
        let data = int_array(INT_ARRAY_LEN);
        let mini = &data; // minicbor encodes Vec<u64> directly
        let mut g = c.benchmark_group("alloc/encode/int_array");
        g.bench_function("cbor2", |b| b.iter(|| enc_cbor2(black_box(&data))));
        g.bench_function("ciborium", |b| b.iter(|| enc_ciborium(black_box(&data))));
        g.bench_function("serde_cbor", |b| {
            b.iter(|| enc_serde_cbor(black_box(&data)))
        });
        g.bench_function("cbor4ii", |b| b.iter(|| enc_cbor4ii(black_box(&data))));
        g.bench_function("minicbor", |b| {
            b.iter(|| minicbor::to_vec(black_box(mini)).unwrap())
        });
        g.finish();
    }
    {
        let data = log_batch(LOG_BATCH_LEN);
        let mini = log_batch_mini(&data);
        let mut g = c.benchmark_group("alloc/encode/log_batch");
        g.bench_function("cbor2", |b| b.iter(|| enc_cbor2(black_box(&data))));
        g.bench_function("ciborium", |b| b.iter(|| enc_ciborium(black_box(&data))));
        g.bench_function("serde_cbor", |b| {
            b.iter(|| enc_serde_cbor(black_box(&data)))
        });
        g.bench_function("cbor4ii", |b| b.iter(|| enc_cbor4ii(black_box(&data))));
        g.bench_function("minicbor", |b| {
            b.iter(|| minicbor::to_vec(black_box(&mini)).unwrap())
        });
        g.finish();
    }
    {
        let raw = blob(BLOB_LEN);
        let data = ByteBuf::from(raw.clone());
        let mini = minicbor::bytes::ByteVec::from(raw);
        let mut g = c.benchmark_group("alloc/encode/blob");
        g.bench_function("cbor2", |b| b.iter(|| enc_cbor2(black_box(&data))));
        g.bench_function("ciborium", |b| b.iter(|| enc_ciborium(black_box(&data))));
        g.bench_function("serde_cbor", |b| {
            b.iter(|| enc_serde_cbor(black_box(&data)))
        });
        g.bench_function("cbor4ii", |b| b.iter(|| enc_cbor4ii(black_box(&data))));
        g.bench_function("minicbor", |b| {
            b.iter(|| minicbor::to_vec(black_box(&mini)).unwrap())
        });
        g.finish();
    }
}

fn bench_decode(c: &mut Criterion) {
    {
        let data = int_array(INT_ARRAY_LEN);
        let cbor2_b = enc_cbor2(&data);
        let ciborium_b = enc_ciborium(&data);
        let serde_b = enc_serde_cbor(&data);
        let cbor4ii_b = enc_cbor4ii(&data);
        let mini_b = minicbor::to_vec(&data).unwrap();
        let mut g = c.benchmark_group("alloc/decode/int_array");
        g.bench_function("cbor2", |x| {
            x.iter(|| cbor2::from_slice::<Vec<u64>>(black_box(&cbor2_b)).unwrap())
        });
        g.bench_function("ciborium", |x| {
            x.iter(|| ciborium::from_reader::<Vec<u64>, _>(black_box(&ciborium_b[..])).unwrap())
        });
        g.bench_function("serde_cbor", |x| {
            x.iter(|| serde_cbor::from_slice::<Vec<u64>>(black_box(&serde_b)).unwrap())
        });
        g.bench_function("cbor4ii", |x| {
            x.iter(|| cbor4ii::serde::from_slice::<Vec<u64>>(black_box(&cbor4ii_b)).unwrap())
        });
        g.bench_function("minicbor", |x| {
            x.iter(|| minicbor::decode::<Vec<u64>>(black_box(&mini_b)).unwrap())
        });
        g.finish();
    }
    {
        let data = log_batch(LOG_BATCH_LEN);
        let mini = log_batch_mini(&data);
        let cbor2_b = enc_cbor2(&data);
        let ciborium_b = enc_ciborium(&data);
        let serde_b = enc_serde_cbor(&data);
        let cbor4ii_b = enc_cbor4ii(&data);
        let mini_b = minicbor::to_vec(&mini).unwrap();
        let mut g = c.benchmark_group("alloc/decode/log_batch");
        g.bench_function("cbor2", |x| {
            x.iter(|| cbor2::from_slice::<Vec<LogEntry>>(black_box(&cbor2_b)).unwrap())
        });
        g.bench_function("ciborium", |x| {
            x.iter(|| ciborium::from_reader::<Vec<LogEntry>, _>(black_box(&ciborium_b[..])).unwrap())
        });
        g.bench_function("serde_cbor", |x| {
            x.iter(|| serde_cbor::from_slice::<Vec<LogEntry>>(black_box(&serde_b)).unwrap())
        });
        g.bench_function("cbor4ii", |x| {
            x.iter(|| cbor4ii::serde::from_slice::<Vec<LogEntry>>(black_box(&cbor4ii_b)).unwrap())
        });
        g.bench_function("minicbor", |x| {
            x.iter(|| minicbor::decode::<Vec<LogEntryMini>>(black_box(&mini_b)).unwrap())
        });
        g.finish();
    }
    {
        let raw = blob(BLOB_LEN);
        let data = ByteBuf::from(raw.clone());
        let mini = minicbor::bytes::ByteVec::from(raw);
        let cbor2_b = enc_cbor2(&data);
        let ciborium_b = enc_ciborium(&data);
        let serde_b = enc_serde_cbor(&data);
        let cbor4ii_b = enc_cbor4ii(&data);
        let mini_b = minicbor::to_vec(&mini).unwrap();
        let mut g = c.benchmark_group("alloc/decode/blob");
        g.bench_function("cbor2", |x| {
            x.iter(|| cbor2::from_slice::<ByteBuf>(black_box(&cbor2_b)).unwrap())
        });
        g.bench_function("ciborium", |x| {
            x.iter(|| ciborium::from_reader::<ByteBuf, _>(black_box(&ciborium_b[..])).unwrap())
        });
        g.bench_function("serde_cbor", |x| {
            x.iter(|| serde_cbor::from_slice::<ByteBuf>(black_box(&serde_b)).unwrap())
        });
        g.bench_function("cbor4ii", |x| {
            x.iter(|| cbor4ii::serde::from_slice::<ByteBuf>(black_box(&cbor4ii_b)).unwrap())
        });
        g.bench_function("minicbor", |x| {
            x.iter(|| minicbor::decode::<minicbor::bytes::ByteVec>(black_box(&mini_b)).unwrap())
        });
        g.finish();
    }
}

criterion_group!(benches, bench_encode, bench_decode);
criterion_main!(benches);
