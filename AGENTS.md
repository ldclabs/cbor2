# Agent Guide for cbor2

This file is the short contract for code agents generating or migrating
`cbor2` integrations. Prefer these rules over guessing from generic serde or
CBOR examples.

## Cargo Setup

The default build (`cbor2 = "1"`) is `std` and already covers `to_vec`,
`to_writer`, `from_slice`, `from_reader`, `validate`, `Value`, `RawValue`,
`cbor!`, the canonical encoders, diagnostics and the synchronous `async_io`
helpers. Add a feature only for the rows that need one:

| Need | Manifest line |
| --- | --- |
| `#[derive(cbor2::Cbor)]` | `cbor2 = { version = "1", features = ["derive"] }` |
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
| Require exactly one item in a buffer | `cbor2::validate` before/after decode | Assuming `from_slice` rejects trailing bytes |
| Decode a CBOR sequence | `cbor2::de::Deserializer::into_iter` | Repeated `from_slice` on the same buffer |
| Preserve exact encoded bytes | `cbor2::RawValue` | Decode/re-encode through typed structs |
| Dynamic or unknown shape | `cbor2::Value` or `cbor2::cbor!` | Untyped maps of JSON strings |
| Deterministic bytes for signatures | `to_canonical_vec` / `to_canonical_writer` | Plain `to_vec` on maps with unspecified order |
| COSE integer keys, arrays, or tags | `#[derive(cbor2::Cbor)]` (feature `derive`) | `serde(rename = "1")` for integer keys |
| Async read/write of a typed value | `cbor2::async_io::{read_value, write_value}` | Treating serde itself as async |
| Async item when you must borrow or inspect raw bytes | `cbor2::async_io::read_item` then `from_slice` | `read_value` when the buffer must outlive the call |

## Non-Negotiable Semantics

- `from_slice` and `from_reader` deserialize one leading CBOR item. They are
  not exact-buffer validators. Use `validate` when trailing data must fail.
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
  array. Do not combine it with per-field `#[cbor(key = ...)]`.
- `async_io::read_value`/`write_value` frame and (de)serialize one CBOR item in
  a single call. Drop to `read_item`/`write_item` only to borrow from or inspect
  the raw item bytes; serde itself stays synchronous on the buffered item.
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
