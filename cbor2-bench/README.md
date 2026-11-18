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
the library's dependency graph or MSRV. A separate stable-toolchain CI job
runs fixture assertions, benchmark smoke tests and parser tests; it does not
collect performance timings.

## Running

```sh
cd cbor2-bench

cargo bench                 # everything
cargo bench --bench alloc   # one scenario
cargo bench --bench std -- 'encode/log_batch'   # one criterion filter
cargo bench --bench focused -- review          # Value, diagnostics, hex and capacity workloads
cargo bench --bench derive                     # equal-wire derive/flatten workloads

cargo run --release --bin sizes   # encoded-size table only
```

Results land in `target/criterion/` (HTML reports under
`target/criterion/report/index.html`). To regenerate the markdown tables in
this file, capture a run and feed it to the bundled parser:

For a published comparison, run from a clean checkout and retain the commit,
compiler, command, complete dependency lockfile and raw output together:

```sh
# Generate the local lockfile first if this is a new checkout.
test -f Cargo.lock || cargo generate-lockfile
run_dir="target/benchmark-runs/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$run_dir"
git rev-parse HEAD > "$run_dir/commit.txt"
git status --short > "$run_dir/status.txt"
rustc -Vv > "$run_dir/rustc.txt"
uname -a > "$run_dir/platform.txt"
cp Cargo.lock "$run_dir/Cargo.lock"
cat > "$run_dir/command.sh" <<'SH'
cargo bench --locked -- --noplot --warm-up-time 0.5 --measurement-time 2 --sample-size 60
SH
sh "$run_dir/command.sh" > "$run_dir/stdout.log" 2> "$run_dir/stderr.log"
python3 parse_results.py "$run_dir/stdout.log" > "$run_dir/tables.md"
```

The parser reports Criterion's time **point estimate** (slope, or mean when
slope is unavailable), not the sample median, in Criterion's own units
(including `ps` for sub-nanosecond results). It includes all five targets,
including `derive`, and ignores unsupported groups without reusing another
benchmark's identity. Missing measurements appear as `—`.

For a quick correctness check without collecting timings:

```sh
cargo test --all-targets
python3 -B -m unittest discover -s . -p 'test_*.py'
```

## What is measured

### Three comparison scenarios and two regression targets

The comparison targets select in-memory, reader/writer, and fixed-buffer
API paths on a `std` host. They do not certify `no_std` builds: dependencies
still have their host features enabled. In particular, minicbor has distinct
`skip` implementations with and without `alloc`; this suite measures the
alloc-capable version on definite-length fixtures that need no heap-backed stack.
The `focused` and `derive` targets track cbor2 regressions across revisions.

| binary                            | scenario            | encode path                                     | decode path                             |
| --------------------------------- | ------------------- | ----------------------------------------------- | --------------------------------------- |
| [`alloc`](benches/alloc.rs)       | fresh buffer        | grow a fresh `Vec<u8>` (`to_vec`)               | borrow from a `&[u8]` (`from_slice`)    |
| [`std`](benches/std.rs)           | reused writer       | stream into a **reused** buffer via `io::Write` | copy through `io::Read` (`from_reader`) |
| [`no_alloc`](benches/no_alloc.rs) | fixed buffer        | fill a fixed `&mut [u8]`, zero allocation       | *scan only* — see capability matrix     |

### Three payload shapes

Defined in [`src/lib.rs`](src/lib.rs), generated from a seeded SplitMix64 PRNG
so every crate and every run sees byte-identical input:

- **`int_array`** — `Vec<u64>` of 1024 values spread across every CBOR
  integer header width. Pure header throughput.
- **`log_batch`** — 128 structured telemetry records mixing text, integers,
  a float, a bool and a nested string list. The "real document" case. The
  serde crates encode it as text-keyed maps; minicbor uses its idiomatic
  positional array, so its bytes are smaller (see sizes table).
- **`blob`** — one 4 KiB CBOR byte string (major type 2), via
  `serde_bytes::ByteBuf` / `minicbor::bytes::ByteVec`. The COSE / crypto
  payload case.

## Capability matrix

Capabilities below refer to the listed versions' high-level APIs. Building
without `alloc` support is different from a particular operation performing
zero allocations in a host build.

| operation | cbor2 | ciborium | serde_cbor | cbor4ii | minicbor |
| --- | :---: | :---: | :---: | :---: | :---: |
| encode → `Vec` | ✅ | ✅ | ✅ | ✅ | ✅ |
| encode → fixed `&mut [u8]` | ✅ | ✅ | ✅ | ✅⁷ | ✅ |
| decode from slice | ✅ | ✅¹ | ✅ | ✅ | ✅ |
| decode via `io::Read` | ✅ | ✅ | ✅ | ✅ | ❌² |
| typed decode with no `alloc` support | ❌³ | ❌ | ✅⁸ | ❌³ | ✅ |
| structural scan with no `alloc` support | ✅⁴ | — | ✅⁸ | — | ✅⁵ |
| exact size without writing encoded bytes | ✅⁶ | — | — | — | ✅⁹ |

1. Ciborium has no borrowing slice decoder; its reader API copies input.
2. Minicbor's decoder accepts slices rather than `io::Read`.
3. These entries describe the serde front ends, which require `alloc` support.
   cbor2's low-level [`core::Decoder`] does support manual decoding without it.
4. cbor2 provides `validate` and `validate_slice`: both check structure, UTF-8,
   nesting limits and complete consumption of exactly one item.
5. Minicbor `Decoder::skip` also checks text UTF-8, but skips one value rather
   than validating exact-buffer consumption. Its no-alloc implementation has
   restrictions on nested indefinite-length containers.
6. cbor2's `serialized_size` uses the existing serde `Serialize` implementation.
7. The cbor4ii path here uses `to_writer` over a slice and requires `std`.
8. Serde_cbor offers `de::from_slice_with_scratch` and `de::from_mut_slice`
   without `alloc`, including typed borrowed values and `IgnoredAny` scans.
   Targets must themselves avoid allocation; the caller supplies any scratch.
9. Minicbor offers `len` / `len_with` using its separate `CborLen` trait.

For example, cbor2's low-level decoder can read an integer array without a heap:

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
Historical Criterion time point estimates (lower is better). Absolute
numbers are machine-dependent — reproduce with `cargo bench`; regenerate the
tables with `python3 parse_results.py bench_results.log`. Recorded on an
**Apple M1 Pro (macOS 26.5, rustc 1.95.0, criterion 0.5.1)**.

These timings predate the result-parser fixes and the changes that make
fixed/reused-buffer benchmarks consume their output bytes. They have not been
regenerated and should not be used as current rankings. The size table has
been rechecked; missing newer timing columns are left blank rather than inferred.

#### Encoded size (bytes)

`int_array` and `blob` are **byte-identical across all five crates**, so those
rows are exact apples-to-apples comparisons. `log_batch` differs by design:
minicbor encodes a positional array (37% smaller, part of why it is faster
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

Minicbor uses its slice decoder in both scenarios. These owned-output
fixtures still allocate their decoded vectors, strings and byte buffers.

#### `no_alloc` — fixed-buffer encode (zero allocation)

| op / payload       | cbor2   | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------ | ------- | -------- | ---------- | ------- | -------- |
| `encode/int_array` | 1.69 µs | 7.82 µs  | 1.38 µs    | 4.61 µs | 2.55 µs  |
| `encode/log_batch` | 4.87 µs | 20.8 µs  | 6.39 µs    | 13.7 µs | 3.96 µs  |
| `encode/blob`      | 60.4 ns | 61.3 ns  | 58.6 ns    | 61.1 ns | 70.9 ns  |

cbor4ii has no public `no_std` slice serializer; here it fills the buffer via
`to_writer` over a `&mut [u8]` (std::io), whose many small writes make it much
slower than its own `to_vec` — the no-alloc encode is not where it shines.

#### `no_alloc` — structural scan

Both cbor2's validators and minicbor's `skip` check text UTF-8. Their contracts
still differ: cbor2 checks exactly one complete item, whereas `skip` advances
over one value. Serde_cbor also supports no-alloc reads (see the matrix), but
is not included in these scan measurements. `validate_slice` has no historical
measurement in this table; new parser output includes it.

| payload | cbor2 `validate` | cbor2 `validate_slice` | minicbor `skip` |
| --- | --- | --- | --- |
| `int_array` | 5.50 µs | — | 4.59 µs |
| `log_batch` | 97.2 µs | — | 13.9 µs |
| `blob` | 110 ns | — | 11.3 ns |

#### `no_alloc` — `cbor2::serialized_size` (cbor2 only)

Exact encoded length with no output buffer; O(1) for a byte string.

| payload     | `serialized_size` |
| ----------- | ----------------- |
| `int_array` | 834 ns            |
| `log_batch` | 1.33 µs           |
| `blob`      | 0.97 ns           |

### Reading the numbers

Compare crates within one API scenario and account for encoded shape.
Integer arrays and blobs are checked for byte-identical output; log batches
have different map/array layouts and float widths. Scan comparisons additionally
have different validation contracts. Use a fresh recorded run for performance
conclusions, and assess allocation counts separately from timing.

<!-- RESULTS:END -->

## Focused regression workloads

`cargo bench --bench focused` covers raw-item encode/decode/sizing,
canonical integer and compound keys, length-first sorting of mixed keys, CDN
arrays of small signed integers and individual large literals, flattened
records and 1,024-chunk strings. The fixtures keep the same logical and wire
shape when comparing revisions. `parse_results.py` also emits these results
and includes the `validate_slice` structural-scan column.

`cargo bench --bench derive` compares integer-keyed derives and flattening
with direct serialization of identical wire bytes. Both regression targets
are included in the parser output.

Comparison targets share `Fixtures`, which prepares each payload once with an
`Encoded` helper. Before timing, it checks complete CBOR validity,
slice/reader round trips, fixed-buffer output against each codec's vector
encoding, and cbor2 size calculation. Integer and blob groups also assert
cross-codec byte equality. Reused/fixed-buffer timings
pass the encoded bytes through `black_box`, not just the output length.

Allocation contracts are exercised by `cargo test -p cbor2 --all-features
--test allocations`: raw sizing and caller-buffer writes allocate zero bytes,
slice capture allocates one copy, and an array-shaped Value allocates one
container. Timing and allocation results should be assessed separately.

## Caveats

- Each crate is benchmarked on **its own idiomatic encoding**, not on
  byte-identical output: minicbor's positional arrays are smaller than the
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
