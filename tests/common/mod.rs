//! Shared support code for the integration tests.
//!
//! `tests/` binaries do not share modules automatically — each `tests/*.rs`
//! file is its own crate root — so shared code lives in this directory and is
//! pulled in with a plain `mod common;` declaration. A subdirectory of
//! `tests/` is not itself compiled as a test binary, so this module exists
//! only inside the binaries that ask for it.
//!
//! Every consumer uses a subset of what the modules below offer, so the
//! `dead_code` lint would fire in each binary for the parts that binary
//! happens not to call.

#![allow(dead_code)]

pub mod source_audit;
