# Agent Guide for cbor2

This file is the short contract for code agents generating or migrating
`cbor2` integrations. Prefer these rules over guessing from generic serde or
CBOR examples.

## Cargo Setup

The default build (`cbor2 = "1"`) is `std` and already covers `to_vec`,
`to_writer`, `from_slice`, `from_reader`, `validate`, `validate_slice`,
`Value`, `Simple`, `RawValue`, `cbor!`, the canonical encoders, diagnostics
and the runtime-neutral `async_io` helpers. Add a feature only for the rows that
need one:

| Need | Manifest line |
| --- | --- |
| `#[derive(cbor2::Cbor)]` | `cbor2 = { version = "1", features = ["derive"] }` |
| CDN hash literals with `no_std` + heap | `cbor2 = { version = "1", default-features = false, features = ["cdn-hash"] }` |
| All CDN extensions (requires `std`) | `cbor2 = { version = "1", features = ["cdn"] }` |
| tokio async adapters | `cbor2 = { version = "1", features = ["tokio"] }` |
| futures async adapters | `cbor2 = { version = "1", features = ["futures"] }` |
| `no_std` + heap | `cbor2 = { version = "1", default-features = false, features = ["alloc"] }` |
| `no_std`, no heap | `cbor2 = { version = "1", default-features = false }` |

Plain serde structs (`#[derive(Serialize, Deserialize)]`, used by most recipes
here) also need `serde = { version = "1", features = ["derive"] }` in the
manifest. Generating code that uses a feature-gated API without enabling the
feature is the most common compile failure — set the manifest first.

## API Selection

| Task | Use | Do not use |
| --- | --- | --- |
| Encode a serde value to memory | `cbor2::to_vec` | Manual `Value` construction unless the shape is dynamic |
| Encode a serde value to a writer | `cbor2::to_writer` | Building a `Vec` first when streaming is enough |
| Decode from an in-memory buffer | `cbor2::from_slice` | `from_reader` if borrowed fields are expected |
| Decode from `Read` | `cbor2::from_reader` | Borrowed output types |
| Require exactly one item in a buffer | `cbor2::validate_slice` before/after decode (`validate` for readers) | Assuming `from_slice` rejects trailing bytes |
| Decode a CBOR sequence | `cbor2::de::Deserializer::from_slice(bytes).into_iter()` (`from_reader` for streams) | Repeated `from_slice` on the same buffer |
| Preserve exact encoded bytes | `cbor2::RawValue` | Decode/re-encode through typed structs |
| Dynamic or unknown shape | `cbor2::Value` or `cbor2::cbor!` | Untyped maps of JSON strings |
| Preserve CBOR simple values | `cbor2::Simple` or `Value::Simple` | Collapsing registered simple values into ad hoc integers |
| Deterministic bytes for signatures | `to_canonical_vec` / `to_canonical_writer` | Plain `to_vec` on maps with unspecified order |
| COSE integer keys, arrays, or tags | `#[derive(cbor2::Cbor)]` (feature `derive`) | `serde(rename = "1")` for integer keys |
| Async read/write of a typed value | `cbor2::async_io::{read_value, write_value}` | Treating serde itself as async |
| Async read from an untrusted stream | `cbor2::async_io::read_value_with_limit` / `read_item_with_limit` | Unbounded helpers without an outer message limit |
| Async item when you must borrow or inspect raw bytes | `cbor2::async_io::read_item` then `from_slice` | `read_value` when the buffer must outlive the call |

## CLI Selection

The `cbor2-cli` crate installs a `cbor` binary. Prefer text-safe commands when
you are an AI/code agent working in a terminal transcript:

| Task | Command |
| --- | --- |
| Inspect pasted CBOR hex/base64 or a file | `cbor <INPUT>` |
| Inspect base64url starting with `-` | `cbor -- -Ds=` (shows `simple(59)`) |
| Convert CBOR to JSON for jq-like tools | `cbor decode --json <INPUT>` |
| Preserve wire details while pretty-printing | `cbor decode <INPUT>` or bare `cbor <INPUT>` |
| Convert JSON to copyable CBOR bytes | `echo '{"a":1}' \| cbor encode --json --hex` |
| Convert CDN to copyable CBOR bytes | `printf "{1: h'dead'}" \| cbor encode --cdn --hex` |
| Pipe raw CBOR bytes to another binary command | `cbor encode` |
| Check one or more complete CBOR items | `cbor validate <INPUT>` |

For agent-generated examples, prefer `cbor encode --hex` over raw
`cbor encode`; raw binary stdout is hard to quote, diff and paste reliably.
Use `--json` when the input is intended to be strict JSON, or `--diag`/`--cdn`
when it is intended to be CDN.

CLI behavior to preserve:

- Hex/base64 detection applies only to inline arguments. CBOR-reading commands
  read raw CBOR bytes from files and stdin. Arguments containing `/` or `\`
  are paths; use base64url or externally decode standard base64 containing `/`.
  Use `--` before an input starting with `-`. Base64 accepts correct padding
  or no padding and requires zero unused tail bits.
- `decode --json` is a lossy projection that keeps CBOR map order. If CBOR
  map keys become the same JSON object key (including duplicate text keys),
  fail with status 1 instead of overwriting a value. Do not output the failing item; earlier sequence items
  may already have been written. Use diagnostic output to inspect such maps.
- Decode, validation and `encode --json` process one item at a time. JSON
  numbers keep their original spelling until CBOR encoding. JSON projection
  renders bignums as signed decimal strings, including object keys. Default
  CDN encoding reads the whole source before parsing. `encode --json` keeps
  JSON member order and duplicate names, producing the same bytes as CDN
  input. Keep streaming output buffered and flush complete items before
  reading more input (not after every item); keep `encode --hex` output
  scratch space bounded rather than building a second, full-size hex string.
- Data errors exit with status 1 and usage errors with status 2. A downstream
  consumer closing the output pipe ends quietly with success; recognize
  `BrokenPipe` wrapped by serde_json or cbor2 as well as direct I/O errors.

## Non-Negotiable Semantics

- `from_slice` and `from_reader` deserialize one leading CBOR item. They are
  not exact-buffer validators. Use `validate_slice` (or `validate` for
  readers) when trailing data must fail.
- `from_slice` is the borrowed path. Definite-length text and byte strings can
  deserialize as borrowed `&str` and `serde_bytes` values from the input.
- `from_reader` copies because it cannot borrow from a generic stream.
- Indefinite-length text and byte strings can decode into owned targets, but
  cannot be borrowed as one contiguous slice.
- Plain `Vec<u8>` and `&[u8]` are serde sequences and encode as CBOR arrays.
  Use `serde_bytes::ByteBuf`, `serde_bytes::Bytes`, or
  `#[serde(with = "serde_bytes")]` for CBOR byte strings.
- `#[derive(cbor2::Cbor)]` generates serde `Serialize` and `Deserialize`.
  Do not also derive serde's `Serialize` or `Deserialize` on the same type.
- `#[cbor(key = 1)]` creates an integer map key. `#[serde(rename = "1")]`
  creates the text key `"1"`.
- `#[cbor(array)]` is for named structs whose CBOR wire shape is a field-order
  array. Do not combine it with per-field `#[cbor(key = ...)]`. Conditional or
  one-directional skipping is rejected for positional fields; use `Option`
  placeholders or symmetric `#[serde(skip)]`.
- The Cbor derive resolves serde through cbor2; a direct serde dependency is
  only needed when the caller itself uses serde APIs or derives.
- For untrusted CDN text, use `cdn_to_vec_with_limit` or
  `cdn_sequence_to_vec_with_limit` with an application-appropriate source-byte
  limit before expensive arbitrary-precision decimal conversion.
- `#[cbor(tag = N)]` wraps the container in CBOR tag `N` on encode and treats
  tag layers as transparent on decode, so the same type accepts tagged *or*
  untagged input. A tagged newtype consumes its declared outer tag and hands
  the intact payload to its field type — no separate "bare" struct plus a `From` impl.
- Generic CBOR simple values are represented by `cbor2::Simple` and
  `Value::Simple`. Use `Simple::new(N)` when a registered simple value such as
  SD-CWT `simple(59)` must appear as a map key.
- `async_io::read_value`/`write_value` frame and (de)serialize one CBOR item in
  a single call. Drop to `read_item`/`write_item` only to borrow from or inspect
  the raw item bytes; serde itself stays synchronous on the buffered item.
- For untrusted async peers, prefer `read_value_with_limit` or
  `read_item_with_limit` unless an outer transport layer already enforces a
  message size limit.
- The `async_io` read helpers are not cancellation-safe: a read future
  dropped before completion (for example, losing a `select!` race against a
  timeout) can leave the stream mid-item. Discard the connection after a
  cancelled read; do not issue another read on it. Put timeouts around the
  connection, not around an individual read future.
- The bare `async_io` traits have no impls of their own. Enable `tokio` or
  `futures` and call `async_io::tokio::*` / `async_io::futures::*` to drive real
  `tokio::io` / `futures_io` streams.

## Migration Cheatsheet

| Existing code | cbor2 replacement |
| --- | --- |
| `serde_cbor::to_vec(value)` | `cbor2::to_vec(value)` |
| `serde_cbor::from_slice(bytes)` | `cbor2::from_slice(bytes)` |
| `ciborium::ser::into_writer(value, writer)` | `cbor2::to_writer(value, writer)` |
| `ciborium::de::from_reader(reader)` | `cbor2::from_reader(reader)` |
| `ciborium`/`serde_cbor` dynamic values | `cbor2::Value` plus `Value::serialized` / `Value::deserialized` |
| Signature payload decoded then re-encoded | `cbor2::RawValue` |

When migrating durable data, check byte-string-sensitive types explicitly.
Some serde visitors distinguish `visit_bytes` from `visit_byte_buf`; use
`serde_bytes`, a compatibility helper, or a `cbor2::Value` bridge when a type
expects CBOR byte strings.

## Recipes

See `docs/agent-cookbook.md` for copyable recipes and common mistakes.
Run `cargo run --example agent_patterns` for a compact executable tour of the
rules above.

## Benchmark Maintenance

`cbor2-bench` is a separate workspace with five targets: `alloc`, `std`,
`no_alloc`, `focused` and `derive`. Root workspace checks do not cover it.

- Reuse `cbor2_bench::Fixtures` (payloads with checked `Encoded` bytes) for
  comparison fixture preparation. Keep round-trip, exact-item, fixed-buffer
  and size assertions outside timed loops.
  Assert cross-codec byte equality for integer arrays and blobs; log records
  intentionally use different map/array layouts and float widths.
- Pass the actual encoded bytes to `black_box` in reused/fixed-buffer encode
  loops, not just their length. Keep each codec's timed API call explicit.
- These are host API-path measurements, not proof of `no_std` compatibility.
  Distinguish zero allocations during a workload from building without
  `alloc` support, and verify capability claims against the pinned versions.
- Criterion's printed time is a slope/mean point estimate, not a median.
  `parse_results.py` must handle all five targets and clear pending identities
  after consuming a time or encountering an unknown group. Missing results
  stay missing; never attribute them to the preceding benchmark.
- For published comparisons, retain the commit, toolchain, command, lockfile
  snapshot and raw logs using the recipe in `cbor2-bench/README.md`. Label old
  timings as historical until rerun; smoke tests do not establish rankings.

## Verification

For changes that affect public API, docs, or examples, use the relevant subset
of:

```bash
cargo fmt --all --check
cargo test --workspace --all-targets --all-features
cargo test -p cbor2 --features derive
cargo test -p cbor2 --no-default-features --lib
cargo test -p cbor2 --no-default-features --features alloc --lib
cargo clippy --workspace --all-targets --all-features -- -D warnings
git diff --check
```

Examples are executable documentation and CI runs all of them; after changing
one, or the behavior it shows, run:

```bash
for example in examples/*.rs; do cargo run --quiet --all-features --example "$(basename "$example" .rs)" > /dev/null; done
```

Integration tests share their test doubles in `tests/it/util.rs`, and
`tests/it/audit_crosscheck.rs` cross-checks the independent decoders,
validators and encoders on generated items. Extend these instead of adding
per-module copies or one-off comparisons.

For CLI behavior changes, run `cargo test -p cbor2-cli`; use
`cargo +1.89 test -p cbor2-cli --locked` when checking the declared MSRV.
For benchmark or result-parser changes, run these separately from the
repository root (the test command smoke-runs all five targets without timing):

```bash
cargo fmt --manifest-path cbor2-bench/Cargo.toml --all --check
cargo test --manifest-path cbor2-bench/Cargo.toml --all-targets
cargo clippy --manifest-path cbor2-bench/Cargo.toml --all-targets -- -D warnings
python3 -B -m unittest discover -s cbor2-bench -p 'test_*.py'
```

Run release benchmarks for affected workloads when changing their measured
paths. The separate stable-toolchain benchmark CI job only checks correctness,
formatting and lint; it does not collect performance results.

## Releases

All three crates share `[workspace.package] version`. Bump it, move the
`CHANGELOG.md` entries under the new version, and run
`scripts/check-release-version.sh vX.Y.Z` before tagging; the release and
crates.io workflows run the same check on the tag before building or
publishing. Raise the `cbor2-derive` requirement in the root `Cargo.toml` when
the derive starts calling newer runtime entry points.
