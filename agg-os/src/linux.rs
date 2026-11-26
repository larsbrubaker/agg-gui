//! The Linux calls of the agg-gui crates, one submodule per area, each behind
//! its own Cargo feature (the rule they follow: `src/lib.rs`).
//!
//! - [`fork_minidump`] (feature `fork-minidump`): a minidump of this process
//!   written by a forked child that ptraces it, for agg-gui-automation's
//!   all-thread stack dump. Built on libc and minidump-writer 0.13.

#[cfg(feature = "fork-minidump")]
pub mod fork_minidump;
