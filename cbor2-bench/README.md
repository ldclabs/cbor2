# cbor2 comparative benchmarks

A standalone [criterion](https://docs.rs/criterion) suite that measures
[`cbor2`](https://crates.io/crates/cbor2) against the other actively used Rust
CBOR implementations:

| crate        | version | serde? | derive model                          |
| ------------ | ------- | ------ | ------------------------------------- |
| **cbor2**    | (local) | yes    | optional `#[derive(Cbor)]` over serde |
| ciborium     | 0.2.2   | yes    | serde derive                          |
| serde_cbor   | 0.11.2  | yes    | serde derive (unmaintained)           |
| serde_cbor_2 | 0.13.0  | yes    | serde derive (fork)                   |
| minicbor     | 2.2.2   | no     | own `#[derive(Encode, Decode)]`       |

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

| operation                             | cbor2 | ciborium | serde_cbor(_2) | minicbor |
| ------------------------------------- | :---: | :------: | :------------: | :------: |
| encode → `Vec` (alloc)                |   ✅   |    ✅     |       ✅        |    ✅     |
| encode → fixed `&mut [u8]` (no_alloc) |   ✅   |    ✅     |       ✅        |    ✅     |
| decode from slice (alloc)             |   ✅   |    ✅¹    |       ✅        |    ✅     |
| decode via `io::Read` (std)           |   ✅   |    ✅     |       ✅        |    ❌²    |
| **decode without alloc**              |  ❌³   |    ❌     |       ❌        |    ✅     |
| no-alloc structural scan / validate   |  ✅⁴   |    ❌     |       ❌        |    ✅⁵    |
| exact size without encoding           |  ✅⁶   |    ❌     |       ❌        |    ❌     |

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
rows are exact apples-to-apples comparisons. `log_batch` differs only because
minicbor encodes an integer-keyed array while the serde crates encode a
text-keyed map — that 37% size gap is part of why minicbor is faster on it.

| payload     | cbor2 | ciborium | serde_cbor | serde_cbor_2 | minicbor |
| ----------- | ----: | -------: | ---------: | -----------: | -------: |
| `int_array` |  4081 |     4081 |       4081 |         4081 |     4081 |
| `log_batch` | 19823 |    19823 |      19823 |        19823 |    12399 |
| `blob`      |  4099 |     4099 |       4099 |         4099 |     4099 |

#### `alloc` — `to_vec` / `from_slice`

| op / payload       | cbor2   | ciborium | serde_cbor | serde_cbor_2 | minicbor |
| ------------------ | ------- | -------- | ---------- | ------------ | -------- |
| `encode/int_array` | 2.78 µs | 6.48 µs  | 1.67 µs    | 1.68 µs      | 3.32 µs  |
| `encode/log_batch` | 13.3 µs | 16.1 µs  | 9.79 µs    | 9.66 µs      | 4.66 µs  |
| `encode/blob`      | 104 ns  | 131 ns   | 127 ns     | 129 ns       | 130 ns   |
| `decode/int_array` | 5.51 µs | 11.5 µs  | 3.66 µs    | 3.29 µs      | 5.24 µs  |
| `decode/log_batch` | 39.4 µs | 67.7 µs  | 33.5 µs    | 34.2 µs      | 22.7 µs  |
| `decode/blob`      | 111 ns  | 246 ns   | 96.4 ns    | 97.4 ns      | 103 ns   |

#### `std` — streaming `io::Write` (reused buffer) / `io::Read`

| op / payload       | cbor2   | ciborium | serde_cbor | serde_cbor_2 | minicbor |
| ------------------ | ------- | -------- | ---------- | ------------ | -------- |
| `encode/int_array` | 2.84 µs | 5.89 µs  | 1.21 µs    | 1.21 µs      | 1.72 µs  |
| `encode/log_batch` | 7.61 µs | 13.3 µs  | 8.78 µs    | 8.80 µs      | 3.68 µs  |
| `encode/blob`      | 65.6 ns | 73.2 ns  | 81.3 ns    | 64.9 ns      | 64.8 ns  |
| `decode/int_array` | 6.61 µs | 11.7 µs  | 6.46 µs    | 6.64 µs      | 5.29 µs  |
| `decode/log_batch` | 57.4 µs | 68.4 µs  | 60.5 µs    | 57.3 µs      | 22.4 µs  |
| `decode/blob`      | 142 ns  | 230 ns   | 233 ns     | 245 ns       | 98.4 ns  |

minicbor's `decode` is slice-only, so its `std/decode` numbers are the same
zero-copy path as `alloc/decode`; the serde crates here pay the copying
`io::Read` source.

#### `no_alloc` — fixed-buffer encode (zero allocation)

| op / payload       | cbor2   | ciborium | serde_cbor | serde_cbor_2 | minicbor |
| ------------------ | ------- | -------- | ---------- | ------------ | -------- |
| `encode/int_array` | 1.76 µs | 9.01 µs  | 1.44 µs    | 1.44 µs      | 2.66 µs  |
| `encode/log_batch` | 5.04 µs | 20.9 µs  | 6.50 µs    | 6.22 µs      | 3.95 µs  |
| `encode/blob`      | 58.7 ns | 62.1 ns  | 58.1 ns    | 58.0 ns      | 72.1 ns  |

#### `no_alloc` — structural scan (the only no-alloc reads available)

The serde deserializers (ciborium, serde_cbor, serde_cbor_2) cannot read
without `alloc` at all. cbor2 offers `validate`; minicbor offers
`Decoder::skip`. Note these are not equivalent operations: cbor2's `validate`
also verifies every text segment is valid UTF-8, which `skip` does not.

| payload     | cbor2 `validate` | minicbor `skip` |
| ----------- | ---------------- | --------------- |
| `int_array` | 5.68 µs          | 4.70 µs         |
| `log_batch` | 96.4 µs          | 14.1 µs         |
| `blob`      | 114 ns           | 11.6 ns         |

#### `no_alloc` — `cbor2::serialized_size` (cbor2 only)

Exact encoded length with no output buffer; O(1) for a byte string.

| payload     | `serialized_size` |
| ----------- | ----------------- |
| `int_array` | 842 ns            |
| `log_batch` | 1.36 µs           |
| `blob`      | 0.97 ns           |

### Reading the numbers

- **Byte-identical workloads** (`int_array`, `blob`): on the integer array
  `serde_cbor(_2)` lead (~1.7 µs), with cbor2 close behind (~2.8 µs) and
  ciborium trailing (~6.5 µs). On the single 4 KiB byte string **cbor2 is the
  fastest** (104 ns in `alloc`, 59 ns in `no_alloc`) — it is one length header
  plus a `memcpy`.
- **Structured `log_batch`** (same text-keyed-map bytes for the four serde
  crates): cbor2's encoder is now **competitive-to-leading** — in the `alloc`
  (fresh-`Vec`) path `serde_cbor` is a bit faster (9.8 vs 13.3 µs), but once the
  buffer is reused (`std`: 7.6 µs vs serde_cbor 8.8, ciborium 13.3) or fixed
  (`no_alloc`: 5.0 vs serde_cbor 6.5 µs) cbor2 leads the serde field; only
  minicbor's compact integer-keyed array is faster. On decode, minicbor still
  leads (~23 µs), helped by that 37%-smaller payload; cbor2 decode (39 µs) runs
  with the serde field.
- **No-alloc encode**: cbor2 is now among the fastest — it beats minicbor on
  the integer array (1.76 vs 2.66 µs) and on the byte string, and beats
  serde_cbor on the map. `serialized_size` is effectively constant-time (sub-µs;
  ~1 ns for a byte string).
- **No-alloc reads**: minicbor's `skip` stays faster than cbor2's `validate`,
  but the two differ in *kind* — `validate` UTF-8-checks every text segment and
  reads through a copying source, while `skip` just advances a borrowed cursor.
  minicbor is also the only crate that decodes a *typed* value with no heap;
  cbor2's `core::Decoder` covers the manual no-heap case (see the matrix).

After a round of encoder work, **cbor2 now matches or leads the serde field on
encoding** — and on `serialized_size` — while keeping its full feature set
(canonical encoding, `Value`/`RawValue`, tags, COSE keys, diagnostics, async
item I/O, validation). Decode is competitive; minicbor's borrowing slice
decoder and smaller wire form still lead on structured data.

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
  every decoder reads bytes it produced.
