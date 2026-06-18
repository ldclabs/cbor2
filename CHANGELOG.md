# Changelog

## [1.0.6] - 2026-06-18

### Added

* Added `diagnostic_pretty_with_key_comments`, which pretty-prints CBOR
  diagnostic notation while annotating integer map keys with matching
  string names from a `Cbor::KEYS`-style table.
* Documented integer-key comment diagnostics in the README, Simplified Chinese
  README and agent cookbook, and updated the CWT example to show CWT claim
  names beside their wire integer keys.

### Changed

* Bumped `cbor2` to `1.0.6`.

## [1.0.5] - 2026-06-15

### Added

* `#[derive(Cbor)]` now supports `#[serde(flatten)]` on map-shaped structs,
  so CWT claim sets can carry extension fields beside declared integer-key
  claims.
* Flattened extension maps can use key types that serialize as integer or text
  labels, such as COSE `Label` / `CoseMap`, and preserve those integer labels
  on CBOR round trips.
* `cbor encode --hex` prints lowercase hex instead of raw CBOR bytes, making
  generated fixtures easy for agents to copy, paste and diff.
* Added `cbor validate`, which validates one or more complete CBOR items and
  prints `valid` on success.

### Changed

* `examples/cwt.rs` now models extension claims as
  `CoseMap(BTreeMap<Label, Value>)`, matching the shape used by `cose2`.
* Bumped `cbor2`, `cbor2-cli` and `cbor2-derive` to `1.0.5`.
* Switched the project license metadata and docs to MIT-only and removed the
  old alternate license.

## [1.0.4] - 2026-06-15

### Added

* Added agent-facing integration docs: `AGENTS.md`,
  `docs/agent-cookbook.md` and the runnable `examples/agent_patterns.rs`
  tour for API selection, exact-item validation, byte strings, borrowed
  deserialization, raw values, CBOR sequences and canonical encoding.
* Added `examples/cwt.rs`, an RFC 8392 CWT claims-set example with registered
  integer claim keys, tag 61 encoding, natural JSON names,
  `skip_serializing_if` claim omission and tagged/untagged decode through one
  derived type.
* Added a `Makefile` with local `lint`, `fix` and `test` shortcuts.

### Changed

* `#[cbor(tag = N)]` still writes tag `N` on encode but now treats tags as
  transparent on decode, so one derived type accepts both tagged and untagged
  COSE/CWT-style payloads without a separate "bare" struct.
* Expanded the COSE/CWT documentation and examples to point at `cose2` wire
  shapes and the new CWT claims example.
* The crates.io publishing workflow now uses Trusted Publishing/OIDC instead
  of a stored `CARGO_REGISTRY_TOKEN`, and crate packages exclude `.github/`.
* The detached benchmark suite now compares with `cbor4ii` instead of
  `serde_cbor_2`.
* Bumped `cbor2`, `cbor2-cli` and `cbor2-derive` to `1.0.4`.

## [1.0.3] - 2026-06-14

### Added

* `from_slice` now supports borrowed deserialization for definite-length
  text and byte strings, including nested struct fields such as `&str` and
  borrowed `serde_bytes` payloads. Segmented indefinite-length strings still
  decode for owned targets but cannot be borrowed.
* `#[derive(Cbor)]` supports `#[cbor(array)]` on named structs, encoding
  fields as a compact CBOR array in declaration order while keeping field
  names for JSON and other serde formats. `cbor2::Cbor` now exposes
  `T::ARRAY`.
* Added `cbor2::async_io` helpers for async byte streams: read one complete
  CBOR item into a buffer, deserialize owned values from that item, or write
  a serialized/validated item without attempting unsupported async serde.
  The `futures` and `tokio` features add matching adapters for
  `futures_io::AsyncRead`/`AsyncWrite` and
  `tokio::io::AsyncRead`/`AsyncWrite`. The `AsyncRead`/`AsyncWrite` traits
  return `Send` futures, so the helpers stay `Send` through generic code and
  can be driven by multi-threaded executors such as `tokio::spawn`.
* Added Simplified Chinese READMEs for `cbor2`, `cbor2-cli` and
  `cbor2-derive`, with language switch links from the English READMEs.
* Added a detached `cbor2-bench` workspace comparing cbor2 with
  `ciborium`, `serde_cbor`, `cbor4ii` and `minicbor`, plus README benchmark
  tables for `std`, `no_std + alloc` and `no_std + no_alloc` paths.

### Changed

* Faster header encoding: `core::Encoder::push` now writes a fixed-size
  array per argument width instead of a runtime-length slice of a scratch
  buffer, letting the writer lower each 1-9 byte header to direct stores
  rather than a general `memcpy`. On the comparative benchmarks this cut
  integer-array encoding by ~37% and structured-map encoding by ~24%, and
  made `serialized_size` about 3× faster, with byte-identical output.
* `de::Deserializer<R>` is now parameterized by its byte [`Source`] rather
  than the raw reader, mirroring `serde_json`: `from_reader` builds a
  `Deserializer<ReaderSource<R>>` (copying) and `from_slice` builds a
  `Deserializer<SliceSource<'de>>` (borrowing). Code using the `from_reader`,
  `from_slice` and free `from_*` functions is unaffected; explicit
  deserializer annotations and slice-specific construction should use the
  source-based forms. The slice recursion-limit constructor is
  `Deserializer::from_slice_with_recursion_limit`.
* Bumped `cbor2`, `cbor2-cli` and `cbor2-derive` to `1.0.3`.

## [1.0.2] - 2026-06-13

### Fixed

* `#[derive(Cbor)]` now uses a fresh internal deserializer lifetime to avoid collisions with user-defined lifetimes, and reports a clear error for the serde-reserved `'de` lifetime name.
* Borrowing deserialization entry points now handle segmented text strings, segmented byte strings and integer arrays consistently with the owning string/byte-buffer paths.
* `Value`-backed deserialization accepts byte-string field identifiers, matching the streaming deserializer behavior for CBOR map keys.

### Added

* Release automation now builds `cbor` binaries for Linux and macOS targets, publishes GitHub release assets with checksums, and updates the `ldclabs/homebrew-tap` formula for `cbor2-cli`.
* Documented Homebrew installation for the `cbor` command in the crate, CLI and workspace READMEs.

### Changed

* Bumped `cbor2`, `cbor2-cli` and `cbor2-derive` to `1.0.2`.

## [1.0.1] - 2026-06-13

### Added

* `diagnostic_pretty` renders raw CBOR diagnostic notation with two-space indentation while preserving wire-level details such as indefinite-length markers.

### Changed

* Diagnostic notation now emits printable non-ASCII text directly for readability, while still escaping control characters and special string syntax.
* The `cbor` CLI display path now uses wire-level pretty diagnostic notation, preserving indefinite-length markers for bare display and `decode --diag`.

## [1.0.0] - 2026-06-13

The first stable release, completing the rewrite that shipped as 0.5.0.

### Added

* `no_std` support behind the new `std` (default) and `alloc` features.
  `default-features = false, features = ["alloc"]` keeps the full API
  minus the `std::io` blanket implementations and the `HashMap`
  conversions; readers and writers are byte slices, `Vec<u8>`, or custom
  `cbor2::io` trait implementations. Without `alloc` the crate is a
  serialization/validation core: `to_writer`/`to_slice`/`serialized_size`,
  `validate`, the `tag` wrappers and the `core` header codec (the serde
  deserializer needs a heap; error messages composed at runtime are
  reduced to static ones).
* `to_slice`: encodes into a caller-provided buffer and returns the
  written prefix, without allocating. Available in every configuration.
* A `RawValue` type holding one item as validated, undecoded bytes (in
  the spirit of `serde_json::value::RawValue`): serialization splices
  the bytes into the stream untouched and deserialization captures them
  byte for byte — exact even for non-preferred spellings — for
  signature payloads, pass-through items and deferred decoding.
  `TryFrom` converts in both directions between `RawValue` and `Value`
  (decoding and encoding respectively).
* CBOR tags on containers: `#[cbor(tag = 18)]` wraps a struct in a tag
  (required on decode), alongside the integer map keys of 0.5.0.
* `Value` conversions to and from the common std types: `From` covers
  the primitive scalars, `Option`, byte arrays/vectors,
  `String`/`&str`/`Cow<str>`, `HashMap`/`BTreeMap` (any `Into<Value>`
  keys and values) and `FromIterator` into an array; `TryFrom<Value>`
  extracts every variant's payload, range-checked integers (the 128-bit
  forms accept bignums) and typed `HashMap`/`BTreeMap` with serde-style
  error messages.

* A `cbor2::Cbor` trait implemented by `#[derive(Cbor)]` (sharing its
  name with the derive macro, serde-style), exposing the declared
  protocol details at runtime: `T::KEYS` (the `field name → integer key`
  pairs) and `T::TAG` as allocation-free constants, plus a
  `keys(&self) -> BTreeMap<String, i128>` convenience method with
  `alloc`.

* The tools package, renamed from `cbor_conv` to `cbor2-cli` (published
  for the first time, also as 1.0.0), now ships a single `cbor` command
  line tool. Bare `cbor` shows each CBOR item as one line of diagnostic
  notation (RFC 8949 §8) with full wire fidelity — indefinite-length
  markers, `undefined`, bignums as plain integers; `cbor decode`
  pretty-prints items as JSON or, with `--diag`, as indented diagnostic
  notation; `cbor encode` converts JSON values to CBOR (replacing the
  old `json2cbor`/`cbor2json` binaries). The CBOR-reading commands take
  their input from stdin, a file, a hex string or a base64/base64url
  string, and everything is covered by end-to-end tests.

### Changed

* The serde `Serializer`/`Deserializer` now run over the `cbor2::io`
  reader/writer traits. With the default `std` feature these are
  implemented for every `std::io::Write`/`Read` and `cbor2::io::Error`
  *is* `std::io::Error`, so existing code is unaffected.
* The `derive` feature's `#[cbor2::int_keys]` attribute macro is
  replaced by `#[derive(cbor2::Cbor)]`, which generates the serde
  `Serialize`/`Deserialize` impls itself (serde's derives must not be
  repeated alongside it). Field names and the type name stay untouched —
  the protocol details ride on a hidden serde-`remote` shadow type — so
  the same types serialize to plain JSON with the original field names
  and no tag: `serde_json::to_string(&v)` just works.
* Error types implement the error trait through `serde::ser::StdError`,
  which is `std::error::Error` whenever `std` is available.

## 0.5.0 (2026-06-12)

A from-scratch rewrite, published as **`cbor2`** (with the companion
`cbor2-derive`): the `cbor` name on crates.io stays with the legacy 0.4
release. The crate now targets RFC 8949 (which obsoleted
RFC 7049) and is built on serde; the `rustc-serialize` based 0.4 API has
been removed entirely.

### Added

* serde `Serializer`/`Deserializer` over `std::io::Write`/`Read`, with
  `to_vec`, `to_writer`, `from_slice` and `from_reader` entry points.
* A dynamic `Value` type with `Value::serialized`/`Value::deserialized`,
  plus the `cbor!` macro for building values in JSON-like syntax.
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
* Integer map keys for structs (COSE, RFC 9052): the `derive` feature
  provides the `#[cbor2::int_keys]` attribute macro, which maps fields
  annotated `#[cbor(key = 1)]` to integer map keys, while a plain
  `#[serde(rename = "1")]` stays a text key, so the two cannot be
  confused. serde field attributes such as `alias` combine freely with
  integer keys. (Extension over ciborium, which has no integer-key
  support.)
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
