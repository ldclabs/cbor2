# Changelog

## [1.1.7] - Unreleased

### Fixed

* Tagged newtypes preserve their payload tags. Typed decoders count transparent
  tags against the nesting limit and accept leading-zero bignum padding within
  a separate 4096-byte primitive payload budget.
* Reader sequence iteration ends after its first error.
* Derived flattened fields preserve CBOR extension values and reject label
  collisions on encode. The derive resolves renamed Cargo dependencies.
* CLI JSON encoding preserves integer and float spellings. JSON decoding uses
  signed decimal strings for out-of-range integers and decimal integer keys.
* Relative CRI references distinguish missing sections and encode leading
  parent-directory segments in the discard count.

### Performance

* Encode ordinary CDN integer literals without allocating a magnitude buffer.
  Use one length-first sort for canonical maps with identical wire and
  equivalence keys. Cap initial Value container capacities at 1024 entries
  after reading the first entry.
* Simplify async framing offsets and add focused small-integer and mixed-key
  benchmark fixtures.

## [1.1.6] - 2026-09-24

### Fixed

* Decode a bignum tag (2 or 3) around anything but a byte string as an
  ordinary tag in `Value` and other self-describing targets. Such items are
  well-formed, and `Value` builds and encodes them (e.g. `2(null)`), but
  decoding them previously failed the whole item.
* Match byte decoding when converting built-in `Value::Simple` values to bool,
  unit, Option and unit enum variants, while preserving the generic Simple form.
* Report CDN application-extension evaluation errors at the source argument's
  opening delimiter, including escaped, computed and CR-normalized arguments.
* Decode `char` from readers with the same error kinds as from slices: a
  multi-character text is an invalid type, not a syntax error.
* Return an error instead of panicking when a tag number is requested from
  untagged tag-protocol input.
* CLI: `decode --json` keeps CBOR map order, and `encode --json` keeps JSON
  member order and duplicate names, producing the same bytes as the default
  CDN input. Both previously sorted object keys, and `encode --json` kept only
  the last duplicate. A directory input is a usage error (status 2) on every
  platform, and `encode --json --hex` ends an empty sequence with a newline
  like CDN input.
* Report sub-nanosecond (`ps`) Criterion estimates in the benchmark result
  parser instead of dropping them.
* Windows installer: edit the raw user `Path` registry value, so other
  `%VARIABLE%` entries stay unexpanded, and let the uninstaller finish after
  removing its own directory.
* Homebrew publishing commits only the formula with a one-off identity,
  leaving a local tap checkout's configuration and other staged changes alone,
  and rejects malformed checksums.
* Examples use `validate_slice` and the slice sequence iterator for in-memory
  buffers, as the agent guide recommends.

### Changed

* Require `cbor2-derive` 1.1 (previously any 1.x release).
* The release and crates.io workflows check that the tag matches the crate
  versions before building or publishing, and CI runs every example.

### Features

* Add `Deserializer::into_iter` for slice deserializers: CBOR sequences held
  in memory decode with borrowing and the slice fast paths, and iteration
  stops after the first error.
* `RawValue::deserialized` can borrow text and byte strings from the raw item.

### Performance

* Use exact size hints for Value arrays and skip ignored Value subtrees directly.
* Format indefinite maps without per-entry strings, avoid redundant canonical
  key sorting in the ordinary bytewise case, and decode hex literals directly
  into bytes without a nibble buffer.
* Add allocation regressions and focused benchmarks.
* Simplify infallible internal helpers and document compatibility entry points
  still required by older derive releases.
* Dispatch `deserialize_any` directly on the pulled header instead of pushing
  it back, so `Value`, untagged enums and flattened structs reach the slice
  fast paths; reader `&str` decoding reuses the scratch buffer.
* Keep the four-bytes-per-element array capacity hint for short arrays, but cap
  it at 4 KiB beyond one byte per element: large arrays of one-byte items no
  longer reserve four times their encoded size, and interleaved runs of the
  capacity workloads showed no slowdown for wide or struct elements.
* Encode `to_canonical_writer*` in one pass, look up integer field keys by
  their canonical decimal text, skip the key table scan for unmarked structs
  and format short `collect_str` output once on the stack.
* Share the slice header argument reads, the CDN list and container parsing,
  and one rejecting serializer for internal tag, simple and raw extractors.
* CLI: buffer all streaming output and flush it only before reading more
  input, serialize JSON directly from `Value`, and buffer input without
  per-read dynamic dispatch. On a 500,000-item sequence, diagnostic decoding
  and `decode --json` take about half the time, `encode --json` 45% less and
  `validate` about 20% less.
* Share derive attribute parsing and shadow construction without changing the
  generated code, and prepare comparison benchmark fixtures once per target.

### Testing

* Cross-check generated well-formed and mutated items across slice and reader
  decoding, validation, diagnostic notation, CDN, both canonical encoders and
  sizing; this found the bignum-tag decoding bug above.
* Exercise the futures adapters with one-byte reads and writes, and check each
  round-trip helper call through the reader, `Value`, sizing and fixed-buffer
  paths as well.
* Replace split-UTF-8 vectors that failed for an unrelated reason, and share
  the test doubles, the synthetic tag-protocol deserializer and the RFC 8949
  Appendix A table instead of keeping per-module copies.

## [1.1.5] - 2026-09-06

### Fixed

* Enforce complete container consumption after serde visitors, including tuples,
  bytes-as-sequences, enums and Value conversions. Nested items no longer leak
  into outer fields; indefinite breaks are consumed exactly once.
* Parse hexadecimal floats with checked exponent scaling and one final rounding;
  preserve NaN sign, signaling bit and payload at the shortest exact width.
  Diagnostic output uses float literals for non-default NaNs.
* Preserve RawValue bytes and Simple values in derived flattened structs;
  reject tagged extension keys that serde cannot represent instead of silently
  stripping their tags; preserve undefined during canonical encoding of
  RawValue, and reject CBOR-equivalent signed-zero map keys. Failed Value
  canonicalization leaves the original value unchanged, and canonical writers
  complete semantic validation before emitting bytes.
* Correct float/same extension argument forms, same map equality, fractional
  epoch conversion, elided UTF-8 concatenation, source CR/raw-string grammar,
  IP address validation and numeric registered-name CRI hosts.
* Support container defaults and recursive Self references in the Cbor derive,
  avoid internal lifetime-prefix collisions, resolve serde without a direct
  consumer dependency, and reject positional field configurations that would
  silently shift data.
* Correct the Homebrew formula smoke test for pretty diagnostic output.

### Features

* Split `cdn-hash` (no_std + alloc) and `cdn-cri` (requires std). `cdn` enables
  both and therefore now explicitly requires std; its previous no_std-only
  combination could not build on bare-metal targets.
* Add `cdn_to_vec_with_limit` and `cdn_sequence_to_vec_with_limit` to bound
  source bytes before parsing untrusted diagnostic input.

### Performance

* RawValue writes and sizing no longer copy to a temporary Vec; slice capture
  copies once, and in-memory validation uses validate_slice.
* Use one collection buffer per Value struct, shared canonical key arenas and
  cached key encodings, linear binary/octal/hex integer parsing and decimal
  limbs, reused text chunk buffers and lookup-based hex output.
* Validate CLI sequences without buffering item payloads; unify application
  extension argument dispatch and async/core header interpretation.
* Add consumer compilation fixtures, allocation contracts, structural and
  numeric differential checks, and focused benchmarks for the affected paths.

## [1.1.4] - 2026-07-16

### Added

* `validate_slice`: the byte-slice counterpart of `validate`. It performs
  exactly the same checks (well-formedness, per-segment text UTF-8, bounded
  nesting, no trailing data) but borrows string bodies directly from the
  input instead of copying them through a scratch buffer, and allocates no
  heap memory. Available in every configuration, `no_std` included.

### Fixed

* Preferred float serialization: the negative canonical quiet NaN (`f64`
  bits `0xfff8_0000_0000_0000`) now encodes as the 3-byte half-width form
  `f9 fe00` instead of the 5-byte `fa ffc00000`. The f16 narrowing dropped
  the NaN sign bit while the widening decoder preserved it, so the
  round-trip probe rejected a value half-precision represents exactly;
  decoding `f9 fe00` and re-encoding is now byte-idempotent, matching the
  shortest-form rule of RFC 8949 §4.1. NaNs with payload bits are encoded
  exactly as before.
* `Value::deserialized` unit targets now skip unknown tags exactly like the
  streaming deserializer, so `7(null)` decodes to `()`, `Option<()>` and unit
  structs through `Value` too, and a tagged `null` payload satisfies a
  map-form unit variant. This closes the last gap of the 1.1.3 integer-target
  fix.
* `#[derive(Cbor)]` now honors a container-level `#[serde(bound = ...)]`,
  including the split `bound(serialize = ..., deserialize = ...)` form: the
  given predicates replace the inferred `T: Serialize` / `T: Deserialize<'de>`
  bounds on the generated outer impls, as they already did on the hidden
  shadow. Previously the attribute was silently ignored there, so the classic
  `PhantomData<T>` + `bound = ""` idiom failed to compile.
* `#[derive(Cbor)]` now rejects `#[serde(from = ...)]` and
  `#[serde(try_from = ...)]` on containers that declare a tag, array shape or
  integer keys, matching the existing `#[serde(into = ...)]` guard: both
  attributes replace the generated `Deserialize` impl, so the declared
  protocol details were silently ignored on decode and such a type could not
  decode its own encoding.
* `Display`/`Debug` for `Value` now bound nesting by the same recursion limit
  as the wire-level diagnostic renderer, returning `fmt::Error` instead of
  exhausting the native stack on a programmatically built value nested deeper
  than the limit. Values decoded from CBOR bytes are unaffected.

### Performance

* Walking an item without materializing it (`validate`, ignored fields,
  `RawValue` capture) no longer zero-initializes a 4 KiB stack buffer per
  string body: the reader path shares one scratch buffer per walk, and the
  slice path (ignored fields and `RawValue` through `from_slice`) skips
  string bodies in place without copying at all. Validating a text-heavy
  document drops to roughly a third of the previous time.
* Encoding into a fresh `Vec` (`to_vec`) no longer hits an inlining cliff
  that turned constant-length header writes into `memcpy` calls; a
  struct-heavy `to_vec` is about twice as fast. Small string bodies also no
  longer pay a `reserve` call each, relying on the writer's amortized
  growth instead.
* `f64` values that cannot narrow losslessly (the common case for measured
  data) are now rejected with one bit mask before the f16/f32 probes, and
  the slice deserializer's header/integer fast paths are inlined: decoding
  a `Vec<u64>` from a slice is about 25% faster, and `serialized_size` over
  float-heavy data more than twice as fast.
* The async `read_body` helper reads directly into the output buffer
  instead of staging every chunk through a 4 KiB stack buffer.

### Documentation

* The `async_io` read helpers now document that they are not
  cancellation-safe (module docs, the four read functions, the adapters and
  `AGENTS.md`): a read future dropped before completion — e.g. losing a
  `select!` race against a timeout — can leave the stream mid-item, so the
  connection must be discarded rather than read again.
* `Write::reserve` and `to_writer` now document that with the `std` feature
  the blanket `std::io::Write` impl cannot forward capacity hints, so
  `to_writer(&value, &mut vec)` skips pre-reservation; `to_vec` remains the
  pre-reserving entry point for encoding into a new `Vec<u8>`.
* The CLI README no longer claims that everything streams: CDN encoding (the
  default `encode` mode) reads its whole input before parsing, as CDN has no
  incremental parser. Decoding, validation and `encode --json` stream item by
  item as before.

## [1.1.3] - 2026-07-04

### Fixed

* Diagnostic notation now renders an empty indefinite-length byte or text
  string as `''_` / `""_` instead of `(_ )`, which cannot carry the string's
  major type and is not parseable CDN (draft-ietf-cbor-edn-literals §2.5.5).
* Preferred serialization now encodes a NaN payload at the narrowest width
  that preserves it: a NaN whose payload fits binary32 is emitted as 4 bytes
  instead of 8 (RFC 8949 §4.1). The new `core::f64_to_f32` helper exposes the
  lossless narrowing, NaN payloads included.
* CDN encoding indicators on `float'..'` literals now re-encode NaN sign and
  payload bit for bit at the requested width, erroring when the payload does
  not fit, instead of silently canonicalizing the NaN.
* Decimal and hexadecimal floating-point literals that overflow the f64 range
  (such as `1e999` or `0x1p+1024`) are now rejected instead of silently
  becoming `Infinity`.
* `IP'..'`/`ip'..'` prefix literals now reject addresses with nonzero bits
  beyond the prefix length (RFC 9164 §4.2) instead of silently masking them.
* `Value::deserialized` integer targets now skip unknown tags exactly like
  the streaming deserializer, so `7(42)` decodes to `u64` through `Value` too.
* The streaming deserializer now accepts an enum encoded as a single-entry
  *indefinite-length* map, matching its handling of every other container.
* Error messages for out-of-range negative integers no longer wrap around
  when the encoded argument exceeds `i64::MAX`.

* `#[derive(Cbor)]` now rejects `#[serde(transparent)]` and
  `#[serde(into = ...)]` on containers that declare a tag, array shape or
  integer keys: both shapes bypass the container marker, so the declared
  protocol details were silently dropped (a `#[cbor(tag = 7)]` +
  `transparent` struct encoded without its tag).
* `#[derive(Cbor)]` now rejects `#[cbor(key = ...)]` on a `#[serde(skip)]`
  field (the field is never on the wire, so the key was dead weight in
  `Cbor::KEYS`); the one-directional `skip_serializing`/`skip_deserializing`
  variants remain allowed.
* `#[derive(Cbor)]` now rejects suffixed integer literals in
  `#[cbor(key = 1u8)]` and `#[cbor(tag = 7u64)]` instead of silently
  ignoring the suffix, and oversized key literals report the CBOR integer
  range instead of a generic overflow error.
* The derive's hidden shadow type now inherits `#[allow]`/`#[expect]` lint
  attributes from the original item, so style lints silenced on the user's
  type no longer re-fire on the generated copy of its names.

### Changed

* `dt'..'` leap-second validation now accepts any month-end `23:59:60` UTC
  instead of consulting a hardcoded table that ended in 2016, so future leap
  seconds parse without a new release.
* Diagnostic output now escapes invisible formatting characters (zero-width
  characters, bidirectional controls, line/paragraph separators) as `\uXXXX`
  so rendered text cannot visually misrepresent the surrounding notation.
* CDN string literals now accept raw DEL (U+007F) and C1 control characters,
  matching the draft's `unescaped` grammar; C0 controls other than LF remain
  rejected.
* Attempting to elide a stream-string chunk (`(_ 'a', ...)`) now reports a
  dedicated error instead of a generic chunk-type error.
* Documented that unknown or reserved encoding indicators are accepted and
  ignored, that `...` and unknown application-extension prefixes encode as
  the draft's provisional CPA tags (888/999/99), that `undefined` decodes to
  `Value::Null`, and that `#[cbor(tag = ...)]` skips tags without validation
  on decode (use `tag::RequireExact` to enforce one).

## [1.1.2] - 2026-06-28

### Added

* Added Windows x86_64 release artifacts for `cbor2-cli`, including a
  `Cbor2CliSetup-windows-x86_64.exe` installer that installs `cbor.exe`, adds
  the install directory to the user `PATH`, and provides an uninstall shortcut.
* Added optional Azure Artifact Signing integration for Windows release
  binaries and installers, with signature verification when signing is
  configured.

### Changed

* `cbor decode` now prints lossless diagnostic notation by default. Use
  `cbor decode --json` for the lossy JSON projection; `--diag` and `--cdn` are
  accepted as explicit spellings of the default.
* `cbor encode` now accepts JSON-compatible values and CDN text by default. Use
  `--json` to force strict JSON input, or `--diag`/`--cdn` to force CDN input.
* Updated the optional CDN hash dependencies to `sha2` 0.11 and `shake` 0.1.
* Bumped `cbor2`, `cbor2-cli` and `cbor2-derive` to `1.1.2`.

## [1.1.1] - 2026-06-28

### Added

* Added the optional `cdn` feature for CDN input extensions that require
  external crates: `hash`, `cri` and `CRI`.
* Added the `bytes` and `same` CDN application extensions from the EDN
  application-extension draft, including examples for byte-string assembly and
  cross-checking alternate literal spellings.
* Exposed `cbor2::cdn` as a public module that groups CDN input helpers,
  output helpers and the shared `cdn::Error` type.

### Changed

* Documented the expanded CDN application-extension support in the README,
  Simplified Chinese README, CLI README files and agent cookbook.
* Bumped `cbor2`, `cbor2-cli` and `cbor2-derive` to `1.1.1`.

## [1.1.0] - 2026-06-26

### Added

* Added Concise Diagnostic Notation (CDN) input support:
  `cdn_to_vec`, `cdn_sequence_to_vec` and `from_cdn`
  parse CDN text into CBOR bytes or serde values. The parser covers comments,
  optional separator commas, base-encoded byte strings, embedded CBOR
  sequences, encoding indicators, tags, simple values, raw strings, and the
  mandatory `dt`, `ip`, `b1` and `t1` application extensions.
* Added CDN-named output helpers: `to_cdn`, `to_cdn_pretty` and
  `to_cdn_pretty_with_key_comments`. The existing `diagnostic*` names remain
  available as compatibility aliases.
* `cbor2-cli` now accepts `cbor encode --diag` to convert CDN text to CBOR;
  combine it with `--hex` for copyable fixture bytes.
* Added full CBOR simple value support through `cbor2::Simple` and
  `Value::Simple`, including values used as map keys. Decoding generic simple
  values now preserves them in `Value`, while explicitly decoding
  `cbor2::Simple` also captures the built-in simple values 20 through 23.
  The existing `cbor!` macro can construct these keys by inlining
  `Simple::new(N).unwrap()` as an expression.
* `cbor2-cli` now renders generic simple values as `"simple(N)"` strings when
  converting CBOR to JSON.

### Fixed

* `core::Encoder::push(Header::Simple(24..=31))` now rejects reserved simple
  values instead of emitting malformed `0xf8` encodings that conforming
  decoders reject.
* Primitive integer deserialization now rejects oversized bignum payloads
  before reading the body even when the payload is padded entirely with leading
  zeroes.

### Changed

* Bumped `cbor2`, `cbor2-cli`, and `cbor2-derive` to `1.1.0`.

## [1.0.7] - 2026-06-26

### Added

* Added bounded async read helpers: `async_io::read_item_with_limit` and
  `async_io::read_value_with_limit`, plus matching `tokio` and `futures`
  adapter functions, for reading one complete CBOR item from untrusted streams
  with an exact encoded-size limit.

### Fixed

* Diagnostic rendering of very large bignum payloads now falls back to explicit
  tag/bytes notation instead of performing expensive arbitrary-precision
  decimal conversion, while preserving lossless output.
* `deserialize_ignored_any` now skips and validates ignored CBOR items without
  materializing large byte or text string payloads.
* Primitive integer deserialization now rejects oversized bignum payloads before
  reading the full body for fixed-width integer targets.

### Changed

* Documented bounded async reads for untrusted peers in the README, Simplified
  Chinese README, `AGENTS.md` and agent cookbook.
* Clarified diagnostic notation and well-formedness wording in the crate docs
  and README files.
* Bumped `cbor2` to `1.0.7`.

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
