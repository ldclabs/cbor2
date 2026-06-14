# cbor2 comparative benchmarks

A standalone [criterion](https://docs.rs/criterion) suite that measures
[`cbor2`](https://crates.io/crates/cbor2) against the other actively used Rust
CBOR implementations:

| crate      | version | serde? | derive model                          |
| ---------- | ------- | ------ | ------------------------------------- |
| **cbor2**  | (local) | yes    | optional `#[derive(Cbor)]` over serde |
| ciborium   | 0.2.2   | yes    | serde derive                          |
| serde_cbor | 0.11.2  | yes    | serde derive (unmaintained)           |
| cbor4ii    | 1.2.2   | yes    | serde derive                          |
| minicbor   | 2.2.2   | no     | own `#[derive(Encode, Decode)]`       |

This crate is **detached from the parent `cbor2` workspace** (it declares its
own `[workspace]`), so criterion and the four comparison crates never enter
the library's dependency graph, CI matrix, or MSRV.

## Running

```sh
cd cbor2-bench

cargo bench                 # everything
cargo bench --bench alloc   # one scenario
cargo bench --bench std -- 'encode/log_batch'   # one criterion filter

cargo run --release --bin sizes   # encoded-size table only
```

Results land in `target/criterion/` (HTML reports under
`target/criterion/report/index.html`). To regenerate the markdown tables in
this file, capture a run and feed it to the bundled parser:

```sh
cargo bench -- --noplot --warm-up-time 0.5 --measurement-time 2 --sample-size 60 \
  | tee bench_results.log
python3 parse_results.py bench_results.log
```

## What is measured

### Three scenarios → three benchmark binaries

The three deployment modes the `cbor2` library supports are each exercised
through the *API path* that mode actually uses. (The comparison runs on a
`std` host — the point is to measure the encode/decode paths that the `std`,
`no_std + alloc`, and `no_std + no_alloc` configurations select, not to
re-measure the same call three times.)

| binary                            | scenario            | encode path                                     | decode path                             |
| --------------------------------- | ------------------- | ----------------------------------------------- | --------------------------------------- |
| [`alloc`](benches/alloc.rs)       | `no_std + alloc`    | grow a fresh `Vec<u8>` (`to_vec`)               | borrow from a `&[u8]` (`from_slice`)    |
| [`std`](benches/std.rs)           | `std`               | stream into a **reused** buffer via `io::Write` | copy through `io::Read` (`from_reader`) |
| [`no_alloc`](benches/no_alloc.rs) | `no_std + no_alloc` | fill a fixed `&mut [u8]`, zero allocation       | *scan only* — see capability matrix     |

### Three payload shapes

Defined in [`src/lib.rs`](src/lib.rs), generated from a seeded SplitMix64 PRNG
so every crate and every run sees byte-identical input:

- **`int_array`** — `Vec<u64>` of 1024 values spread across every CBOR
  integer header width. Pure header throughput.
- **`log_batch`** — 128 structured telemetry records mixing text, integers,
  a float, a bool and a nested string list. The "real document" case. The
  serde crates encode it as text-keyed maps; minicbor uses its idiomatic
  integer-keyed array, so its bytes are smaller (see sizes table).
- **`blob`** — one 4 KiB CBOR byte string (major type 2), via
  `serde_bytes::ByteBuf` / `minicbor::bytes::ByteVec`. The COSE / crypto
  payload case.

## Capability matrix

Performance aside, the scenarios differ in *what is even possible*. This is
the single most important takeaway:

| operation                             | cbor2 | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------------------------- | :---: | :------: | :--------: | :-----: | :------: |
| encode → `Vec` (alloc)                |   ✅   |    ✅     |     ✅      |    ✅    |    ✅     |
| encode → fixed `&mut [u8]` (no_alloc) |   ✅   |    ✅     |     ✅      |   ✅⁷    |    ✅     |
| decode from slice (alloc)             |   ✅   |    ✅¹    |     ✅      |    ✅    |    ✅     |
| decode via `io::Read` (std)           |   ✅   |    ✅     |     ✅      |    ✅    |    ❌²    |
| **decode without alloc**              |  ❌³   |    ❌     |     ❌      |   ❌³    |    ✅     |
| no-alloc structural scan / validate   |  ✅⁴   |    ❌     |     ❌      |    ❌    |    ✅⁵    |
| exact size without encoding           |  ✅⁶   |    ❌     |     ❌      |    ❌    |    ❌     |

1. ciborium has no borrowing slice decoder; `from_reader(&bytes[..])` is its
   only form, so its "slice" decode copies.
2. minicbor has no `io::Read` decoder at all — it is slice-only by design.
3. The ❌ is about the **serde** path only: cbor2's `Deserializer` (like every
   serde-based CBOR crate's) needs a heap scratch buffer. cbor2's low-level
   [`core::Decoder`] pull API *does* decode without `alloc` — it is exactly
   what the no-alloc `validate` is built on; see the example under the matrix.
4. [`cbor2::validate`](https://docs.rs/cbor2/latest/cbor2/fn.validate.html).
5. minicbor `Decoder::skip` (and full no-alloc typed decode into borrowed
   `&str`/`&[u8]`).
6. [`cbor2::serialized_size`](https://docs.rs/cbor2/latest/cbor2/fn.serialized_size.html).
7. cbor4ii has no public `no_std` slice serializer; it fills a fixed
   `&mut [u8]` through `to_writer`, which needs `std`.

The headline: among **serde** front-ends, only minicbor can deserialize into a
typed value with no heap — every serde-based CBOR crate, cbor2 included, needs
`alloc` for a serde `Deserialize`. But cbor2 is not blind without a heap: its
low-level [`core::Decoder`] reads CBOR with zero allocation (and is what powers
the no-alloc `validate`). So in `no_std + no_alloc` cbor2 gives you zero-alloc
*encoding*, *validation*, exact *sizing*, and manual *decoding* via
`core::Decoder` — just not serde-typed decoding.

```rust
use cbor2::core::{Decoder, Header};

// Decode `[1, 42]` with no heap: pull the array header, then each item.
let mut dec = Decoder::from(&[0x82, 0x01, 0x18, 0x2a][..]);
let Header::Array(Some(n)) = dec.pull().unwrap() else { panic!() };
let mut sum = 0u64;
for _ in 0..n {
    match dec.pull().unwrap() {
        Header::Positive(v) => sum += v,
        _ => panic!("expected an integer"),
    }
}
assert_eq!(sum, 43);
```

(Bodies of byte/text strings are read into a caller-provided `&mut [u8]` with
`Decoder::read_exact`, so even strings decode without allocating.)

[`core::Decoder`]: https://docs.rs/cbor2/latest/cbor2/core/struct.Decoder.html

## Results

<!-- RESULTS:START -->
Median wall-clock time per operation (criterion, lower is better). Absolute
numbers are machine-dependent — reproduce with `cargo bench`; regenerate the
tables with `python3 parse_results.py bench_results.log`. Recorded on an
**Apple M1 Pro (macOS 26.5, rustc 1.95.0, criterion 0.5.1)**.

#### Encoded size (bytes)

`int_array` and `blob` are **byte-identical across all five crates**, so those
rows are exact apples-to-apples comparisons. `log_batch` differs by design:
minicbor encodes an integer-keyed array (37% smaller, part of why it is faster
on it), and cbor4ii is slightly larger because it keeps floats at 64-bit where
the other serde crates narrow them to `f32`.

| payload     | cbor2 | ciborium | serde_cbor | cbor4ii | minicbor |
| ----------- | ----: | -------: | ---------: | ------: | -------: |
| `int_array` |  4081 |     4081 |       4081 |    4081 |     4081 |
| `log_batch` | 19823 |    19823 |      19823 |   20335 |    12399 |
| `blob`      |  4099 |     4099 |       4099 |    4099 |     4099 |

#### `alloc` — `to_vec` / `from_slice`

| op / payload       | cbor2   | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------ | ------- | -------- | ---------- | ------- | -------- |
| `encode/int_array` | 2.79 µs | 6.59 µs  | 1.67 µs    | 2.92 µs | 3.29 µs  |
| `encode/log_batch` | 13.3 µs | 16.1 µs  | 9.54 µs    | 6.09 µs | 4.56 µs  |
| `encode/blob`      | 102 ns  | 131 ns   | 133 ns     | 127 ns  | 130 ns   |
| `decode/int_array` | 5.34 µs | 11.0 µs  | 3.24 µs    | 3.43 µs | 5.23 µs  |
| `decode/log_batch` | 38.5 µs | 66.3 µs  | 34.0 µs    | 36.8 µs | 21.8 µs  |
| `decode/blob`      | 97.5 ns | 224 ns   | 88.5 ns    | 90.1 ns | 91.1 ns  |

#### `std` — streaming `io::Write` (reused buffer) / `io::Read`

| op / payload       | cbor2   | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------ | ------- | -------- | ---------- | ------- | -------- |
| `encode/int_array` | 2.81 µs | 5.81 µs  | 1.19 µs    | 1.49 µs | 1.66 µs  |
| `encode/log_batch` | 7.41 µs | 15.0 µs  | 8.71 µs    | 3.53 µs | 3.65 µs  |
| `encode/blob`      | 64.9 ns | 64.9 ns  | 60.0 ns    | 64.9 ns | 60.9 ns  |
| `decode/int_array` | 6.49 µs | 11.0 µs  | 6.40 µs    | 3.42 µs | 5.22 µs  |
| `decode/log_batch` | 54.1 µs | 66.9 µs  | 57.1 µs    | 60.7 µs | 22.4 µs  |
| `decode/blob`      | 147 ns  | 227 ns   | 231 ns     | 101 ns  | 97.8 ns  |

minicbor's `decode` is slice-only, so its `std/decode` numbers are the same
zero-copy path as `alloc/decode`; the serde crates here pay the copying
`io::Read` source.

#### `no_alloc` — fixed-buffer encode (zero allocation)

| op / payload       | cbor2   | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------ | ------- | -------- | ---------- | ------- | -------- |
| `encode/int_array` | 1.69 µs | 7.82 µs  | 1.38 µs    | 4.61 µs | 2.55 µs  |
| `encode/log_batch` | 4.87 µs | 20.8 µs  | 6.39 µs    | 13.7 µs | 3.96 µs  |
| `encode/blob`      | 60.4 ns | 61.3 ns  | 58.6 ns    | 61.1 ns | 70.9 ns  |

cbor4ii has no public `no_std` slice serializer; here it fills the buffer via
`to_writer` over a `&mut [u8]` (std::io), whose many small writes make it much
slower than its own `to_vec` — the no-alloc encode is not where it shines.

#### `no_alloc` — structural scan (the only no-alloc reads available)

The serde deserializers (ciborium, serde_cbor, cbor4ii) cannot read without
`alloc` at all. cbor2 offers `validate`; minicbor offers `Decoder::skip`. Note
these are not equivalent operations: cbor2's `validate` also verifies every
text segment is valid UTF-8, which `skip` does not.

| payload     | cbor2 `validate` | minicbor `skip` |
| ----------- | ---------------- | --------------- |
| `int_array` | 5.50 µs          | 4.59 µs         |
| `log_batch` | 97.2 µs          | 13.9 µs         |
| `blob`      | 110 ns           | 11.3 ns         |

#### `no_alloc` — `cbor2::serialized_size` (cbor2 only)

Exact encoded length with no output buffer; O(1) for a byte string.

| payload     | `serialized_size` |
| ----------- | ----------------- |
| `int_array` | 834 ns            |
| `log_batch` | 1.33 µs           |
| `blob`      | 0.97 ns           |

### Reading the numbers

- **Byte-identical workloads** (`int_array`, `blob`): on the integer array
  `serde_cbor` is fastest (~1.2–1.7 µs); cbor4ii, cbor2 and minicbor cluster
  around 1.5–3.3 µs and ciborium trails (~6–7 µs). On the single 4 KiB byte
  string everything is within noise; **cbor2 edges it** (102 ns in `alloc`,
  60 ns in `no_alloc`) — it is one length header plus a `memcpy`.
- **Structured `log_batch`** (text-keyed maps; cbor4ii's is ~3% larger, see
  sizes): the strongest serde encoders are **cbor4ii and cbor2** — cbor4ii is
  the fastest of *all* crates in `std` (3.5 µs, past minicbor) and leads the
  serde field in `alloc` (6.1 µs), while in `no_alloc` **cbor2 leads the serde
  field** (4.9 µs, where cbor4ii's `io::Write`-over-slice encoder collapses to
  13.7 µs). On decode **minicbor leads** (~22 µs, helped by its 37%-smaller
  payload); the serde decoders run from serde_cbor (34 µs) up.
- **No-alloc encode**: cbor2 leads the serde field into a fixed buffer (1.7 /
  4.9 µs) — even beating minicbor on the byte-identical integer array (1.7 vs
  2.6 µs), though minicbor's compact array form wins the map (4.0 µs). serde_cbor
  is close (1.4 / 6.4 µs); cbor4ii and ciborium trail, routing through a
  `&mut [u8]` `io::Write` one small write at a time. `serialized_size` is
  effectively constant-time (sub-µs; ~1 ns for a byte string) — a primitive
  unique to cbor2.
- **No-alloc reads**: minicbor's `skip` stays faster than cbor2's `validate`,
  but the two differ in *kind* — `validate` UTF-8-checks every text segment and
  reads through a copying source, while `skip` just advances a borrowed cursor.
  minicbor is also the only crate that decodes a *typed* value with no heap;
  cbor2's `core::Decoder` covers the manual no-heap case (see the matrix).

cbor2 is competitive across the board and **uniquely strong in
`no_std + no_alloc`** (fixed-buffer encode, `validate`, `serialized_size`)
while keeping its full feature set (canonical encoding, `Value`/`RawValue`,
tags, COSE keys, diagnostics, async item I/O). **cbor4ii is the surprise on
structured `std`/`alloc` throughput** — but it has no real no-alloc encode
path, keeps floats at 64-bit, and its decoder rejects another crate's
narrowed floats. minicbor's compact, borrowing design still leads structured
decode.

<!-- RESULTS:END -->

## Caveats

- Each crate is benchmarked on **its own idiomatic encoding**, not on
  byte-identical output: minicbor's integer-keyed arrays are smaller than the
  serde crates' text-keyed maps for `log_batch`. A smaller payload is part of
  what makes a codec fast, so this is intentional — but it means the
  `log_batch` row is not a same-bytes comparison.
- The `alloc` encode benches include the cost of allocating the output `Vec`
  each iteration (what `to_vec` does); the `std` encode benches reuse one
  buffer (what a server does). Compare within a scenario, not across.
- `decode` benches first re-encode each payload with that same crate, so
  every decoder reads bytes it produced. This is not just tidiness: the crates
  differ in preferred encoding — cbor2/ciborium/serde_cbor narrow the
  `latency_ms` float to `f32`, and **cbor4ii's decoder rejects an `f32`-encoded
  value for an `f64` field**, so a shared buffer would not round-trip across
  all of them.
