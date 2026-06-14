#!/usr/bin/env python3
"""Parse a criterion run log into markdown tables for the README."""
import re
import sys

ID_RE = re.compile(r"^(alloc|std|no_alloc)/\S+")
# median is the middle value inside `time:   [low  median  high]`
TIME_RE = re.compile(r"time:\s*\[\s*(\S+ \S+)\s+(\S+ \S+)\s+(\S+ \S+)\s*\]")

results = {}  # full bench id -> median string
last_id = None

with open(sys.argv[1]) as fh:
    for line in fh:
        line = line.rstrip("\n")
        if "Benchmarking" in line or line.strip().startswith("Found"):
            continue
        m = TIME_RE.search(line)
        if m:
            left = line[: m.start()].strip()
            bid = left if ID_RE.match(left) else last_id
            if bid:
                results[bid] = m.group(2)
            continue
        if ID_RE.match(line.strip()):
            last_id = line.strip()


def fmt(v):
    return v if v else "—"


CRATES = ["cbor2", "ciborium", "serde_cbor", "cbor4ii", "minicbor"]
PAYLOADS = ["int_array", "log_batch", "blob"]


def table(prefix, ops, crates=CRATES, op_label="op"):
    header = "| " + op_label + " / payload | " + " | ".join(crates) + " |"
    sep = "|" + "---|" * (len(crates) + 1)
    rows = [header, sep]
    for op in ops:
        for p in PAYLOADS:
            cells = []
            for cr in crates:
                key = f"{prefix}/{op}/{p}/{cr}"
                cells.append(fmt(results.get(key)))
            rows.append(f"| `{op}/{p}` | " + " | ".join(cells) + " |")
    return "\n".join(rows)


print("### alloc  (`no_std + alloc`)\n")
print(table("alloc", ["encode", "decode"]))
print("\n### std\n")
print(table("std", ["encode", "decode"]))
print("\n### no_alloc — encode (fixed buffer, zero allocation)\n")
print(table("no_alloc", ["encode"]))
print("\n### no_alloc — structural scan (the only no-alloc reads available)\n")
scan = ["cbor2 (validate)", "minicbor (skip)"]
hdr = "| payload | " + " | ".join(scan) + " |"
print(hdr)
print("|" + "---|" * (len(scan) + 1))
for p in PAYLOADS:
    cells = [fmt(results.get(f"no_alloc/scan/{p}/{c}")) for c in scan]
    print(f"| `{p}` | " + " | ".join(cells) + " |")
print("\n### no_alloc — `cbor2::serialized_size` (no output buffer)\n")
print("| payload | cbor2::serialized_size |")
print("|---|---|")
for p in PAYLOADS:
    print(f"| `{p}` | {fmt(results.get('no_alloc/serialized_size (cbor2)/' + p))} |")
print(f"\n_({len(results)} measurements parsed)_")
