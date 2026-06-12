//! The single integration-test binary.
//!
//! Keeping all integration tests in one binary makes `cargo test` faster
//! and lets the coverage of generic functions instantiated by the tests
//! merge properly.

mod canonical;
#[cfg(feature = "derive")]
mod cose;
mod de_edge;
mod diag;
mod errors;
mod limits;
mod markers;
mod raw;
mod rfc8949;
mod roundtrip;
mod size;
mod tag;
mod validate;
mod value;
mod value_api;
mod value_serde;
