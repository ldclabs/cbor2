# cbor2: full-featured CBOR for Rust

Most CBOR libraries for Rust are serializers — they turn your structs into bytes and back, and stop there. But [CBOR (RFC 8949)](https://www.rfc-editor.org/rfc/rfc8949) has a lot more surface than JSON: semantic tags, bignums, deterministic encoding, a diagnostic notation, indefinite-length streams. The moment you need any of that — COSE, signed payloads, a canonical hash — you're usually hand-rolling it on top of a serializer.

**cbor2** is an attempt to ship that whole surface as one serde-native toolkit, while staying competitive on speed and scaling down to `no_std` without an allocator.

A bit of lineage first, because credit matters: cbor2 descends from Andrew Gallant's original `cbor` crate. Version 0.5 was a from-scratch rewrite on serde + RFC 8949, and 1.0 stabilizes it. There are other good crates in this space — `ciborium` and `minicbor` are solid, `cbor4ii` is fast — and `serde_cbor`, the one most people still reach for, has been unmaintained since 2021. cbor2 isn't trying to dethrone any of them; it's aimed at the "I need the full protocol, not just a serializer" niche.

## The same struct, two wire formats

This is the part I'm most happy with. COSE (RFC 9052 — the signing/encryption format behind WebAuthn, CWT, etc.) keys its maps with **integers**, not strings, and wraps messages in tags. serde's data model can't express either. The usual answer is a second set of types, or manual encoding.

cbor2 lets one struct do both:

```rust
use cbor2::Cbor;

#[derive(Cbor)]
#[cbor(tag = 98)]
struct CoseSign {
    #[cbor(key = 1)]
    kty: u8,
    #[cbor(key = 3)]
    alg: i8,
    comment: String, // no key → stays a text key on the wire
}
```

Encode it with cbor2 and you get the compact, integer-keyed, tag-98 form COSE expects. Hand the *same value* to `serde_json::to_string` and you get ordinary `{"kty": ..., "alg": ..., "comment": ...}` — field names intact, no tag. JSON for your logs and debug endpoints, COSE on the wire, one type.

Under the hood `#[derive(Cbor)]` generates the serde impls itself via a hidden `#[serde(remote)]` shadow type and a container-name marker, so the real type's field and type names are never touched. No registries, no link-time tricks — it works on `wasm32-unknown-unknown` too.

## When the exact bytes matter

For signatures and content-addressing, *which* bytes you produce is the whole point. cbor2 ships deterministic encoding (RFC 8949 §4.2.1 — sorted map keys, shortest-form integers/floats, normalized bignums and NaN):

```rust
let canonical = cbor2::to_canonical_vec(&value)?;
```

And for the other direction — verifying a signature over bytes you didn't produce — there's `RawValue`, which captures one item as validated-but-undecoded bytes:

```rust
#[derive(Serialize, Deserialize)]
struct Signed {
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
    payload: cbor2::RawValue, // captured byte-for-byte
}
// verify `signature` over `signed.payload.as_bytes()`, *then* decode it
```

You verify over the exact wire bytes, even non-preferred spellings, before trusting the contents. Round-tripping through a typed value would silently re-encode and break the signature.

There's also a dynamic `Value` with a `cbor!` macro, RFC 8949 §8 diagnostic notation (and a `cbor` CLI that prints it), and a `validate` that checks an input is exactly one well-formed item.

## All the way down to `no_alloc`

cbor2 is `no_std`. With `alloc` you get the full heap-backed API; without it you still get serialization, validation, and exact sizing — none of which allocate:

```rust
let mut buf = [0u8; 256];
let item = cbor2::to_slice(&value, &mut buf)?; // encodes into your buffer
let size = cbor2::serialized_size(&value)?;    // exact length, no output buffer
cbor2::validate(&item[..])?;                   // well-formed? (incl. UTF-8)
```

Here's the honest caveat, and it's worth saying plainly: **no serde-based CBOR crate can deserialize without a heap** — the deserializer needs a scratch buffer. cbor2 included. Only `minicbor`, which isn't serde-based, decodes typed values with no allocator. What cbor2 offers instead is its low-level `core::Decoder` pull API, which reads CBOR with zero allocation if you're willing to drive it by hand:

```rust
use cbor2::core::{Decoder, Header};

let mut dec = Decoder::from(&bytes[..]);
let Header::Array(Some(n)) = dec.pull()? else { /* ... */ };
// pull headers; read string/byte bodies into your own &mut [u8]
```

So the no-alloc story is: zero-alloc encode, validate, and sizing through the high-level API; manual decode through `core::Decoder`. Not serde-typed decode. I'd rather you know that up front than discover it mid-project.

## Is it fast?

Yes, but I'm not going to claim it's the fastest, because it isn't, and you'd check. I built a [detached benchmark workspace](https://github.com/ldclabs/cbor2/tree/main/cbor2-bench#results) that compares cbor2 against ciborium, serde_cbor, cbor4ii, and minicbor across all three deployment modes (`std`, `no_std + alloc`, `no_std + no_alloc`) on byte-identical payloads where possible, so the integer-array and byte-string rows are exact apples-to-apples.

The honest summary: cbor2's encode and decode land in the top tier and beat ciborium across the board, but `serde_cbor` and `cbor4ii` edge it on some encode rows, and `minicbor` leads structured decode thanks to a more compact wire form. Where cbor2 is distinctly strong is `no_std + no_alloc` — fastest fixed-buffer encode of the serde crates, plus the `validate`/`serialized_size` primitives nobody else ships. (Building the suite turned up fun trivia too, like cbor4ii's decoder rejecting an `f32`-narrowed float for an `f64` field — so each crate decodes bytes it produced itself.)

The full capability matrix and per-scenario tables, with methodology, are in the [benchmark README](https://github.com/ldclabs/cbor2/tree/main/cbor2-bench#results).

## What it doesn't do

- No typed decode without `alloc` (the serde-ecosystem reality above).
- It's brand new, so adoption is tiny and there are surely rough edges.
- If you want the absolute fastest structured decode and can use a non-serde derive, `minicbor` is the better tool — and that's fine.

## Try it

```toml
[dependencies]
cbor2 = "1"
```

- crates.io: <https://crates.io/crates/cbor2>
- docs: <https://docs.rs/cbor2>
- repo: <https://github.com/ldclabs/cbor2>

Bug reports, design critiques, and "why didn't you just use X" are all genuinely welcome — that's why I'm posting it.
