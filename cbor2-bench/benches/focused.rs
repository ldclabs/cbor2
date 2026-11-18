//! Workloads for raw items, canonical maps, diagnostic input, flattening and
//! segmented text. Compare the same fixture and wire shape across revisions.
use cbor2::{Cbor, RawValue, Value};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::{collections::BTreeMap, hint::black_box};

#[derive(serde::Deserialize)]
struct KnownFields {
    id: u64,
}

#[derive(serde::Serialize)]
struct ArrayRecord {
    id: u64,
    enabled: bool,
}

// Serializes through `collect_str`, like chrono and other Display-based types.
struct Displayed(u64);

impl serde::Serialize for Displayed {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

fn review(c: &mut Criterion) {
    let array = Value::Array((0..1024u64).map(Value::from).collect());
    c.bench_function("review/value_array", |b| {
        b.iter(|| black_box(&array).deserialized::<Vec<u64>>().unwrap())
    });
    let record = Value::Map(vec![("id".into(), 42.into()), ("extra".into(), array)]);
    c.bench_function("review/value_ignore", |b| {
        b.iter(|| black_box(&record).deserialized::<KnownFields>().unwrap().id)
    });

    let mut map = vec![0xbf];
    for n in 0..128u64 {
        cbor2::to_writer(&n, &mut map).unwrap();
        cbor2::to_writer(&n, &mut map).unwrap();
    }
    map.push(0xff);
    c.bench_function("review/pretty_indefinite_map", |b| {
        b.iter(|| cbor2::to_cdn_pretty(black_box(map.as_slice())).unwrap())
    });
    let text = format!("h'{}'", "ab".repeat(4096));
    c.bench_function("review/hex_literal", |b| {
        b.iter(|| cbor2::cdn_to_vec(black_box(&text)).unwrap())
    });
    let map = Value::Map((0..1024u64).rev().map(|n| (n.into(), n.into())).collect());
    c.bench_function("review/canonical_writer", |b| {
        let mut out = Vec::new();
        b.iter(|| {
            out.clear();
            cbor2::to_canonical_writer(black_box(&map), &mut out).unwrap();
            black_box(&out);
        })
    });
    let records: Vec<_> = (0..1024u64)
        .map(|id| ArrayRecord { id, enabled: true })
        .collect();
    c.bench_function("review/canonical_struct", |b| {
        b.iter(|| cbor2::to_canonical_vec(black_box(&records)).unwrap())
    });

    let entries: Vec<_> = (0..256u64)
        .map(|n| {
            cbor2::cbor!({
                "id": n,
                "level": "info",
                "message": "request completed",
                "ok": true,
                "ratio": 0.5,
                "tags": ["api", "v1"],
            })
            .unwrap()
        })
        .collect();
    let bytes = cbor2::to_vec(&entries).unwrap();
    c.bench_function("review/value_decode", |b| {
        b.iter(|| cbor2::from_slice::<Value>(black_box(&bytes)).unwrap())
    });

    let displayed: Vec<_> = (0..1024u64).map(|n| Displayed(n * 7919)).collect();
    c.bench_function("review/collect_str", |b| {
        b.iter(|| cbor2::to_vec(black_box(&displayed)).unwrap())
    });

    let mut sequence = Vec::new();
    for n in 0..1024u64 {
        cbor2::to_writer(&(n * 65537), &mut sequence).unwrap();
    }
    c.bench_function("review/sequence_reader", |b| {
        b.iter(|| {
            cbor2::de::Deserializer::from_reader(black_box(sequence.as_slice()))
                .into_iter::<u64>()
                .map(Result::unwrap)
                .sum::<u64>()
        })
    });
    c.bench_function("review/sequence_slice", |b| {
        b.iter(|| {
            cbor2::de::Deserializer::from_slice(black_box(sequence.as_slice()))
                .into_iter::<u64>()
                .map(Result::unwrap)
                .sum::<u64>()
        })
    });

    let mut group = c.benchmark_group("review/array_encode");
    for n in [16, 1024, 10000] {
        let flags = vec![true; n];
        group.bench_with_input(BenchmarkId::new("bool", n), &flags, |b, value| {
            b.iter(|| cbor2::to_vec(black_box(value)).unwrap())
        });
        let small = vec![7u8; n];
        group.bench_with_input(BenchmarkId::new("u8", n), &small, |b, value| {
            b.iter(|| cbor2::to_vec(black_box(value)).unwrap())
        });
        let wide = vec![u64::MAX; n];
        group.bench_with_input(BenchmarkId::new("u64", n), &wide, |b, value| {
            b.iter(|| cbor2::to_vec(black_box(value)).unwrap())
        });
        let records: Vec<_> = (0..n)
            .map(|n| ArrayRecord {
                id: n as u64,
                enabled: true,
            })
            .collect();
        group.bench_with_input(BenchmarkId::new("struct", n), &records, |b, value| {
            b.iter(|| cbor2::to_vec(black_box(value)).unwrap())
        });
    }
    group.finish();
}

#[derive(Cbor)]
struct OpenRecord {
    #[cbor(key = 1)]
    name: String,
    body: RawValue,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn focused(c: &mut Criterion) {
    let payload = vec![0x55; 1024 * 1024];
    let raw = RawValue::serialized(serde_bytes::Bytes::new(&payload)).unwrap();
    let mut raw_group = c.benchmark_group("focused/raw");
    raw_group.throughput(Throughput::Bytes(raw.as_bytes().len() as u64));
    raw_group.bench_function("encode_slice_1MiB", |b| {
        let mut out = vec![0; raw.as_bytes().len()];
        b.iter(|| {
            let bytes = cbor2::to_slice(black_box(&raw), &mut out).unwrap();
            black_box(bytes);
        });
    });
    raw_group.bench_function("decode_1MiB", |b| {
        b.iter(|| cbor2::from_slice::<RawValue>(black_box(raw.as_bytes())).unwrap())
    });
    raw_group.bench_function("size_1MiB", |b| {
        b.iter(|| cbor2::serialized_size(black_box(&raw)).unwrap())
    });
    raw_group.finish();

    let mut canonical = c.benchmark_group("focused/canonical");
    for n in [16, 256, 1024] {
        let map = Value::Map(
            (0..n)
                .rev()
                .map(|i| (Value::from(i as u64), Value::from(i as u64)))
                .collect(),
        );
        canonical.bench_with_input(BenchmarkId::new("integer_keys", n), &map, |b, value| {
            b.iter(|| cbor2::to_canonical_vec(black_box(value)).unwrap())
        });
    }
    let composite = Value::Map(
        (0..128u64)
            .rev()
            .map(|i| {
                (
                    Value::Array(vec![i.into(), "long compound key".repeat(8).into()]),
                    i.into(),
                )
            })
            .collect(),
    );
    canonical.bench_function("compound_keys", |b| {
        b.iter(|| cbor2::to_canonical_vec(black_box(&composite)).unwrap())
    });
    let mixed = Value::Map(
        (0..1024u64)
            .map(|index| {
                let i = (index * 619) % 1024;
                let key = match i % 3 {
                    0 => Value::from(u64::MAX - i),
                    1 => Value::from(format!("k{i}")),
                    _ => Value::from(format!("long text key {i}")),
                };
                (key, i.into())
            })
            .collect(),
    );
    let length_first = cbor2::to_canonical_vec_with(&mixed, cbor2::KeyOrder::LengthFirst).unwrap();
    assert_ne!(length_first, cbor2::to_canonical_vec(&mixed).unwrap());
    canonical.bench_function("length_first_mixed_1024", |b| {
        b.iter(|| {
            cbor2::to_canonical_vec_with(black_box(&mixed), cbor2::KeyOrder::LengthFirst).unwrap()
        })
    });
    canonical.finish();

    let mut cdn = c.benchmark_group("focused/cdn");
    for n in [2000, 8000, 16000] {
        for (kind, text) in [
            ("hex", format!("0x{}", "f".repeat(n))),
            ("decimal", "9".repeat(n)),
        ] {
            cdn.throughput(Throughput::Bytes(text.len() as u64));
            cdn.bench_with_input(BenchmarkId::new(kind, n), &text, |b, text| {
                b.iter(|| cbor2::cdn_to_vec(black_box(text)).unwrap())
            });
        }
    }
    for (name, value) in [("small_positive_array", 42), ("small_negative_array", -7)] {
        let text = format!("[{}]", vec![value.to_string(); 1000].join(","));
        assert_eq!(
            cbor2::cdn_to_vec(&text).unwrap(),
            cbor2::to_vec(&vec![value; 1000]).unwrap()
        );
        cdn.throughput(Throughput::Elements(1000));
        cdn.bench_function(name, |b| {
            b.iter(|| cbor2::cdn_to_vec(black_box(&text)).unwrap())
        });
    }
    cdn.finish();

    let record = OpenRecord {
        name: "record".into(),
        body: RawValue::serialized(serde_bytes::Bytes::new(&payload[..4096])).unwrap(),
        extra: [("count".into(), 7.into())].into(),
    };
    let bytes = cbor2::to_vec(&record).unwrap();
    let mut flatten = c.benchmark_group("focused/flatten");
    flatten.bench_function("encode", |b| {
        b.iter(|| cbor2::to_vec(black_box(&record)).unwrap())
    });
    flatten.bench_function("decode", |b| {
        b.iter(|| cbor2::from_slice::<OpenRecord>(black_box(&bytes)).unwrap())
    });
    flatten.finish();

    let text = format!("ilts<<{}>>", vec!["\"chunk\""; 1024].join(","));
    let bytes = cbor2::cdn_to_vec(&text).unwrap();
    let mut strings = c.benchmark_group("focused/text");
    strings.throughput(Throughput::Bytes(bytes.len() as u64));
    strings.bench_function("slice_1024_chunks", |b| {
        b.iter(|| cbor2::from_slice::<String>(black_box(&bytes)).unwrap())
    });
    strings.bench_function("reader_1024_chunks", |b| {
        b.iter(|| cbor2::from_reader::<String, _>(black_box(bytes.as_slice())).unwrap())
    });
    strings.finish();

    let integers: Vec<_> = (0..1024u64).map(|n| n * 65537).collect();
    let bytes = cbor2::to_vec(&integers).unwrap();
    c.bench_function("focused/integer_decode", |b| {
        b.iter(|| cbor2::from_slice::<Vec<u64>>(black_box(&bytes)).unwrap())
    });
}

criterion_group!(benches, focused, review);
criterion_main!(benches);
