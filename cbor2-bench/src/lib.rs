//! Shared fixtures for the comparative CBOR benchmarks.
//!
//! Three payload shapes exercise different parts of a codec:
//!
//! * [`int_array`] — a flat `Vec<u64>`, the pure integer-header throughput
//!   case that every crate encodes natively.
//! * [`log_batch`] / [`log_batch_mini`] — a batch of structured telemetry
//!   records mixing text, integers, floats, booleans and nested lists: the
//!   "real document" case. The serde crates encode it as text-keyed maps;
//!   minicbor uses its idiomatic integer-keyed array form. Each crate is
//!   benchmarked on its *own* natural encoding, so the byte sizes differ
//!   slightly — that is part of what the comparison shows.
//! * [`blob`] — a single large byte string (major type 2), the COSE /
//!   crypto-payload case.
//!
//! Data is generated from a tiny deterministic PRNG so every run, and every
//! crate, sees byte-identical input without pulling in `rand`.

use serde::{Deserialize, Serialize};

/// SplitMix64 — a deterministic, dependency-free PRNG for fixtures.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// A structured telemetry record, serde flavor (text-keyed map on the wire).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: u64,
    pub level: u8,
    pub target: String,
    pub message: String,
    pub line: u32,
    pub success: bool,
    pub latency_ms: f64,
    pub labels: Vec<String>,
}

/// The same record for minicbor, encoded as an integer-keyed CBOR array
/// (`#[cbor(array)]`) — minicbor's idiomatic, compact form.
#[derive(Clone, Debug, PartialEq, minicbor::Encode, minicbor::Decode)]
#[cbor(array)]
pub struct LogEntryMini {
    #[n(0)]
    pub timestamp: u64,
    #[n(1)]
    pub level: u8,
    #[n(2)]
    pub target: String,
    #[n(3)]
    pub message: String,
    #[n(4)]
    pub line: u32,
    #[n(5)]
    pub success: bool,
    #[n(6)]
    pub latency_ms: f64,
    #[n(7)]
    pub labels: Vec<String>,
}

impl From<&LogEntry> for LogEntryMini {
    fn from(e: &LogEntry) -> Self {
        Self {
            timestamp: e.timestamp,
            level: e.level,
            target: e.target.clone(),
            message: e.message.clone(),
            line: e.line,
            success: e.success,
            latency_ms: e.latency_ms,
            labels: e.labels.clone(),
        }
    }
}

const TARGETS: [&str; 6] = [
    "net::http::server",
    "db::pool",
    "auth::token",
    "cbor2::de",
    "runtime::worker",
    "telemetry::export",
];

const WORDS: [&str; 12] = [
    "request",
    "completed",
    "timeout",
    "retry",
    "cache",
    "miss",
    "handshake",
    "expired",
    "queued",
    "flush",
    "deadline",
    "exceeded",
];

const LABELS: [&str; 8] = [
    "prod",
    "region=eu",
    "shard=3",
    "tls",
    "h2",
    "ipv6",
    "warm",
    "canary",
];

/// Builds a deterministic batch of `n` log records.
pub fn log_batch(n: usize) -> Vec<LogEntry> {
    let mut rng = SplitMix64::new(0xC0DE_CAFE);
    (0..n)
        .map(|i| {
            let r = rng.next_u64();
            let target = TARGETS[(r as usize) % TARGETS.len()].to_string();
            let word_count = 4 + (r >> 8) as usize % 6;
            let mut message = String::with_capacity(word_count * 8);
            for w in 0..word_count {
                if w > 0 {
                    message.push(' ');
                }
                message.push_str(WORDS[(rng.next_u64() as usize) % WORDS.len()]);
            }
            let label_count = (r >> 16) as usize % 4;
            let labels = (0..label_count)
                .map(|_| LABELS[(rng.next_u64() as usize) % LABELS.len()].to_string())
                .collect();
            LogEntry {
                timestamp: 1_700_000_000_000 + i as u64 * 37,
                level: (r >> 24) as u8 % 5,
                target,
                message,
                line: (r >> 32) as u32 % 4096,
                success: r & 1 == 0,
                latency_ms: (r >> 40) as f64 / 1024.0,
                labels,
            }
        })
        .collect()
}

/// The minicbor view of [`log_batch`].
pub fn log_batch_mini(batch: &[LogEntry]) -> Vec<LogEntryMini> {
    batch.iter().map(LogEntryMini::from).collect()
}

/// A flat array of `n` pseudo-random `u64`s spanning every header width.
pub fn int_array(n: usize) -> Vec<u64> {
    let mut rng = SplitMix64::new(0x1234_5678);
    (0..n)
        .map(|i| match i % 5 {
            // Spread across the CBOR integer header widths.
            0 => rng.next_u64() % 24,
            1 => rng.next_u64() % 256,
            2 => rng.next_u64() % 65_536,
            3 => rng.next_u64() % (1 << 32),
            _ => rng.next_u64(),
        })
        .collect()
}

/// A single byte string of `n` pseudo-random bytes (CBOR major type 2).
pub fn blob(n: usize) -> Vec<u8> {
    let mut rng = SplitMix64::new(0xDEAD_BEEF);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        out.extend_from_slice(&rng.next_u64().to_le_bytes());
    }
    out.truncate(n);
    out
}

/// Workload sizes shared by every scenario, so the three benchmark binaries
/// report comparable numbers.
pub const LOG_BATCH_LEN: usize = 128;
pub const INT_ARRAY_LEN: usize = 1024;
pub const BLOB_LEN: usize = 4096;
