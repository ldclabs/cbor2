# Changelog

## 0.5.0 (unreleased)

A from-scratch rewrite, published as **`cbor2`** (with the companion
`cbor2-derive`): the `cbor` name on crates.io stays with the legacy 0.4
release. The crate now targets RFC 8949 (which obsoleted
RFC 7049) and is built on serde; the `rustc-serialize` based 0.4 API has
been removed entirely.

### Added

* serde `Serializer`/`Deserializer` over `std::io::Write`/`Read`, with
  `to_vec`, `to_writer`, `from_slice` and `from_reader` entry points.
* A dynamic `Value` type with `Value::serialized`/`Value::deserialized`,
  plus the `cbor!` macro for building values in JSON-like syntax: `:`
  between keys and values, exactly like `serde_json::json!` (the
  ciborium-style `=>` is accepted as well), with arbitrary CBOR values —
  integers included — as map keys. `Value` converts to and from the
  common std types: `From` covers the primitive scalars, `Option`,
  byte arrays/vectors, `String`/`&str`/`Cow<str>`, `HashMap`/`BTreeMap`
  (any `Into<Value>` keys and values) and `FromIterator` into an array;
  `TryFrom<Value>` extracts every variant's payload, range-checked
  integers (the 128-bit forms accept bignums) and typed
  `HashMap`/`BTreeMap` with serde-style error messages.
* Tag support via `tag::{AllowAny, AllowExact, RequireAny, RequireExact}`;
  `u128`/`i128` encode as bignums (tags 2/3) when out of 64-bit range.
* Indefinite-length *encoding* (for unsized sequences/maps) and decoding
  (arrays, maps, segmented byte and text strings) — the feature the old
  README listed as "the big thing missing".
* Preferred serialization: smallest lossless width for integers and floats,
  including IEEE 754 half-precision.
* `Deserializer::into_iter` for decoding CBOR sequences (RFC 8742).
* Diagnostic notation (RFC 8949 §8): `diagnostic` renders raw CBOR as
  human-readable text byte-for-byte equal to the Appendix A examples,
  preserving indefinite-length forms, `undefined` and unassigned simple
  values, and writing bignums as plain integers; `Value` implements
  `Display` with the same notation, and `Debug` pretty-prints it with
  two-space indentation.
* Allocation-free helpers: `validate` checks an input for well-formedness
  (RFC 8949 §5.3.1, plus text UTF-8 validity) and `serialized_size`
  computes the exact encoded size of a value without buffering output.
  `collect_str` no longer buffers formatted output either.
* Integer map keys and tags for structs (COSE, RFC 9052): the `derive`
  feature provides `#[derive(cbor2::Cbor)]`, which generates the serde
  `Serialize`/`Deserialize` impls with fields annotated
  `#[cbor(key = 1)]` mapped to integer map keys and the container
  optionally wrapped in a CBOR tag (`#[cbor(tag = 18)]`, required on
  decode), while a plain `#[serde(rename = "1")]` stays a text key, so
  the two cannot be confused. serde attributes such as `alias`, `with`
  and `default` combine freely. Field names and the type name stay
  untouched — the protocol details ride on a hidden serde-`remote`
  shadow type — so the same types serialize to plain JSON with the
  original field names and no tag: `serde_json::to_string(&v)` just
  works. (Extension over ciborium, which has no integer-key support.)
* A `RawValue` type holding one item as validated, undecoded bytes (in
  the spirit of `serde_json::value::RawValue`): serialization splices
  the bytes into the stream untouched and deserialization captures them
  byte for byte — exact even for non-preferred spellings — for
  signature payloads, pass-through items and deferred decoding.
* Deterministic encoding via `to_canonical_vec`, `to_canonical_writer` and
  `Value::canonicalize`: map key sorting, duplicate key rejection, bignum
  reduction to preferred form and NaN normalization. Both deterministic key
  orderings are supported through `KeyOrder` and the `*_with`/
  `canonicalize_with` variants: the default `KeyOrder::Bytewise` implements
  the core requirements of RFC 8949 §4.2.1, while `KeyOrder::LengthFirst`
  implements the legacy "Canonical CBOR" order of RFC 7049 §3.9 (RFC 8949
  §4.2.3) and matches ciborium's canonical module byte for byte.
* A configurable recursion limit (default 256) and allocation-safe handling
  of forged length headers.
* The low-level `core` module: a pull/push `Header` codec.
* Wire compatibility with `ciborium` 0.2, verified by tests.

### Removed

* Everything from 0.4: `Encoder`, `Decoder`, `Cbor*` abstract syntax types,
  `ToCbor`/`ToJson`, and the `rustc-serialize` dependency.

### Changed

* The `cbor2json`/`json2cbor` tools are now implemented with `serde_json`.
* Minimum supported Rust version: 1.85. Edition 2021.
* CI moved from Travis to GitHub Actions.
