//! Scenario: **`no_std` + `no_alloc`** — no heap at all.
//!
//! ## Encoding (all five crates)
//!
//! Every crate can serialize into a caller-provided fixed buffer with zero
//! allocation, through a different door:
//!
//! | crate          | no-alloc encode entry point                |
//! |----------------|--------------------------------------------|
//! | cbor2          | [`cbor2::to_slice`]                         |
//! | ciborium       | `into_writer` over `&mut [u8]`             |
//! | serde_cbor     | `Serializer` over `ser::SliceWrite`        |
//! | serde_cbor_2   | `Serializer` over `ser::SliceWrite`        |
//! | minicbor       | `encode` over `encode::write::Cursor`      |
//!
//! The output buffer is allocated once during setup and reused; nothing on
//! the measured path touches the allocator.
//!
//! ## Reading (cbor2 and minicbor only)
//!
//! Deserialization is where the designs diverge sharply. The three serde
//! deserializers — cbor2, ciborium and serde_cbor(_2) — all need a heap
//! scratch buffer and therefore **cannot deserialize without `alloc` at
//! all**. What cbor2 *does* offer without a heap is `cbor2::validate`, which
//! walks the bytes and proves well-formedness without materializing a value;
//! minicbor's comparable no-alloc primitive is `Decoder::skip`. Those two are
//! compared in the `scan` groups below. minicbor additionally is the only
//! crate that can produce a *typed* value with no heap (by borrowing
//! `&str`/`&[u8]` straight out of the input) — see the README capability
//! matrix.

use std::hint::black_box;

use cbor2_bench::*;
use criterion::{criterion_group, criterion_main, Criterion};
use serde::Serialize;
use serde_bytes::ByteBuf;

/// Reused scratch buffer, sized once to fit the largest fixture.
const CAP: usize = 64 * 1024;

fn bench_encode(c: &mut Criterion) {
    macro_rules! encode_group {
        ($name:literal, $serde:expr, $mini:expr) => {{
            let data = $serde;
            let mini = $mini;
            let mut g = c.benchmark_group($name);
            g.bench_function("cbor2", |b| {
                let mut buf = vec![0u8; CAP];
                b.iter(|| cbor2::to_slice(black_box(&data), &mut buf).unwrap().len())
            });
            g.bench_function("ciborium", |b| {
                let mut buf = vec![0u8; CAP];
                b.iter(|| {
                    let mut slice: &mut [u8] = &mut buf[..];
                    ciborium::into_writer(black_box(&data), &mut slice).unwrap();
                    CAP - slice.len()
                })
            });
            g.bench_function("serde_cbor", |b| {
                let mut buf = vec![0u8; CAP];
                b.iter(|| {
                    let mut ser =
                        serde_cbor::Serializer::new(serde_cbor::ser::SliceWrite::new(&mut buf));
                    black_box(&data).serialize(&mut ser).unwrap();
                    ser.into_inner().bytes_written()
                })
            });
            g.bench_function("serde_cbor_2", |b| {
                let mut buf = vec![0u8; CAP];
                b.iter(|| {
                    let mut ser =
                        serde_cbor_2::Serializer::new(serde_cbor_2::ser::SliceWrite::new(&mut buf));
                    black_box(&data).serialize(&mut ser).unwrap();
                    ser.into_inner().bytes_written()
                })
            });
            g.bench_function("minicbor", |b| {
                let mut buf = vec![0u8; CAP];
                b.iter(|| {
                    let mut cur = minicbor::encode::write::Cursor::new(&mut buf[..]);
                    minicbor::encode(black_box(&mini), &mut cur).unwrap();
                    cur.position()
                })
            });
            g.finish();
        }};
    }

    let logs = log_batch(LOG_BATCH_LEN);
    let logs_mini = log_batch_mini(&logs);
    let raw = blob(BLOB_LEN);

    encode_group!(
        "no_alloc/encode/int_array",
        int_array(INT_ARRAY_LEN),
        int_array(INT_ARRAY_LEN)
    );
    encode_group!("no_alloc/encode/log_batch", logs, logs_mini);
    encode_group!(
        "no_alloc/encode/blob",
        ByteBuf::from(raw.clone()),
        minicbor::bytes::ByteVec::from(raw)
    );
}

/// No-alloc structural reads: prove well-formedness / skip one item without
/// building a value. Only cbor2 and minicbor expose such a primitive.
fn bench_scan(c: &mut Criterion) {
    macro_rules! scan_group {
        ($name:literal, $serde:expr, $mini:expr) => {{
            let bytes = cbor2::to_vec(&$serde).unwrap();
            let bytes_mini = minicbor::to_vec(&$mini).unwrap();
            let mut g = c.benchmark_group($name);
            g.bench_function("cbor2 (validate)", |x| {
                x.iter(|| cbor2::validate(black_box(&bytes[..])).unwrap())
            });
            g.bench_function("minicbor (skip)", |x| {
                x.iter(|| {
                    let mut d = minicbor::Decoder::new(black_box(&bytes_mini));
                    d.skip().unwrap()
                })
            });
            g.finish();
        }};
    }

    let logs = log_batch(LOG_BATCH_LEN);
    let logs_mini = log_batch_mini(&logs);
    let raw = blob(BLOB_LEN);

    scan_group!(
        "no_alloc/scan/int_array",
        int_array(INT_ARRAY_LEN),
        int_array(INT_ARRAY_LEN)
    );
    scan_group!("no_alloc/scan/log_batch", logs, logs_mini);
    scan_group!(
        "no_alloc/scan/blob",
        ByteBuf::from(raw.clone()),
        minicbor::bytes::ByteVec::from(raw)
    );
}

/// `cbor2::serialized_size` computes the exact encoded length with no output
/// buffer and no allocation — a sizing primitive the other crates do not
/// ship. Shown across the three payloads.
fn bench_serialized_size(c: &mut Criterion) {
    let logs = log_batch(LOG_BATCH_LEN);
    let ints = int_array(INT_ARRAY_LEN);
    let blob = ByteBuf::from(blob(BLOB_LEN));
    let mut g = c.benchmark_group("no_alloc/serialized_size (cbor2)");
    g.bench_function("int_array", |x| {
        x.iter(|| cbor2::serialized_size(black_box(&ints)).unwrap())
    });
    g.bench_function("log_batch", |x| {
        x.iter(|| cbor2::serialized_size(black_box(&logs)).unwrap())
    });
    g.bench_function("blob", |x| {
        x.iter(|| cbor2::serialized_size(black_box(&blob)).unwrap())
    });
    g.finish();
}

criterion_group!(benches, bench_encode, bench_scan, bench_serialized_size);
criterion_main!(benches);
