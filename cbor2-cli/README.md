# cbor2-cli

Inspect, convert and debug CBOR
([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949)) from the terminal. This
crate installs the `cbor` command, built on
[`cbor2`](https://crates.io/crates/cbor2).

English | [简体中文](README.zh-CN.md)

```bash
cargo install cbor2-cli   # installs the `cbor` binary
```

Or install it from the ldclabs Homebrew tap:

```bash
brew install ldclabs/tap/cbor2-cli   # installs the `cbor` binary
```

```text
Usage: cbor [COMMAND] [INPUT]

Commands:
  (none)  Show each CBOR item as pretty diagnostic notation (§8)
  decode  Convert CBOR items to pretty-printed JSON, or to
          pretty-printed diagnostic notation with --diag
  encode  Convert JSON values, or CDN text with --diag, to CBOR items
  validate
          Validate one or more complete CBOR items
```

## Why cbor2-cli

| Need                  | Command support                                                                                                                      |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Inspect pasted CBOR   | Run `cbor <hex-or-base64>` to render RFC 8949 diagnostic notation.                                                                   |
| Preserve wire details | Bare `cbor` captures each item as raw bytes, so indefinite lengths, segmented strings, `undefined` and simple values remain visible. |
| Decode for JSON tools | `cbor decode` pretty-prints CBOR as JSON, one document per item.                                                                     |
| Encode fixtures       | `cbor encode` turns JSON values into CBOR bytes; `cbor encode --diag` reads Concise Diagnostic Notation.                            |
| Copy bytes safely     | `cbor encode --hex` prints copyable lowercase hex for agents and docs; combine it with `--diag` for CDN fixtures.                    |
| Work with sequences   | Multiple JSON or CDN values become a CBOR sequence; CBOR sequences decode item by item.                                              |
| Validate inputs       | `cbor validate <hex-or-file>` checks one or more complete CBOR items and prints `valid` on success.                                  |
| Script reliably       | Data errors exit with status 1, usage errors with status 2.                                                                          |

`INPUT` is a file path, a hex string (optionally `0x`-prefixed), a
base64/base64url string, or `-` for stdin; stdin is the default. An
argument containing a path separator is always a file path. Everything
streams: multiple JSON or CDN values become a CBOR sequence (RFC 8742), and a
CBOR sequence becomes one output document or line per item. Data errors
exit with status 1, usage errors with status 2.

## Agent-friendly usage

For code agents, prefer text-first commands unless a pipeline needs raw bytes:

```bash
cbor validate a1616101
echo '{"a":1}' | cbor encode --hex
printf "{1: h'dead'}" | cbor encode --diag --hex
cbor decode a1616101
cbor decode --diag bf616101ff
```

Use raw `cbor encode` only when piping directly into another binary command.
Use `cbor encode --hex` when the result needs to be pasted into a test, a
prompt, a review comment or another `cbor` invocation. Use
`cbor encode --diag --hex` for fixtures that are easier to write as CDN than
as JSON.

## Show: `cbor`

The everyday command. It renders each item as the human-readable text
form of RFC 8949 §8 — what CBOR specs and test vectors are written in —
and it is exact: every item is captured as its wire bytes, so
indefinite-length items keep their `_` markers, segmented strings appear
as `(_ ...)`, `undefined` and unassigned simple values appear as
themselves, byte strings render as `h'...'` and bignums print as plain
integers, exactly as in RFC 8949 Appendix A. Very large bignum payloads
fall back to explicit tag/bytes notation to keep rendering bounded.

```bash
$ cbor a201020326                  # hex, pasted straight from a spec
{
  1: 2,
  3: -7
}

$ cbor 0x8301820203820405          # 0x-prefixed works too
[
  1,
  [
    2,
    3
  ],
  [
    4,
    5
  ]
]

$ cbor oWFhAQ                      # base64url, padded or not
{
  "a": 1
}

$ cbor message.cbor                # a file
16([
  h'a1010a',
  {
    5: h'89f52f65a1c580933b5261a78c'
  },
  h'5974e1b9...'
])

$ cbor bf61610161629f0203ffff      # wire details survive
{_
  "a": 1,
  "b": [_
    2,
    3
  ]
}
```

## decode

`cbor decode` pretty-prints each item as JSON, or — with `-d`/`--diag` — as
indented diagnostic notation. The diagnostic path reads raw item bytes, so it
preserves indefinite lengths and other wire details; the JSON path decodes
through `Value` and therefore uses JSON-compatible spelling.

```bash
$ cbor decode a1018202036466697665f5
{
  "1": [
    2,
    3
  ]
}
"five"
true

$ cbor decode --diag a101820203
{
  1: [
    2,
    3
  ]
}
```

JSON conversion is best-effort where CBOR is richer: byte strings become
lowercase hex strings, non-string map keys are JSON-encoded into strings,
non-finite floats and `undefined` become `null`, integers beyond the
64-bit ranges become strings, and tags are dropped (keeping the inner
value).

## encode

`cbor encode` reads JSON text (from a file or stdin) and writes each
value as a CBOR item. Add `--diag` to read Concise Diagnostic Notation
instead, and `--hex` for copyable lowercase hex text:

```bash
$ echo '{"name": "example", "ok": true}' | cbor encode | cbor
{"name": "example", "ok": true}

$ echo '{"name": "example", "ok": true}' | cbor encode | xxd -p
a2646e616d65676578616d706c65626f6bf5

$ echo '{"name": "example", "ok": true}' | cbor encode --hex
a2646e616d65676578616d706c65626f6bf5

$ printf "{ /kty/ 1: 4, /k/ -1: h'6684523a' }" | cbor encode --diag --hex
a2010420446684523a

$ printf "bytes<<\"sig:\", h'deadbeef'>>" | cbor encode --diag --hex
487369673adeadbeef

$ printf "same<<float'47110815', 0x1.22102ap+15>>" | cbor encode --diag --hex
fa47110815
```

## validate

`cbor validate` checks that the input contains one or more complete CBOR
items. It prints `valid` on success, exits with status 1 for malformed data
and status 2 for usage errors:

```bash
$ cbor validate a1616101
valid
```

## License

Licensed under the MIT License.
