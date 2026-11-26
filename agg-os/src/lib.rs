//! The operating system calls and the other unsafe code of the agg-gui
//! workspace's crates, each behind a safe function or type.
//!
//! # The workspace's rule on `unsafe`
//!
//! This crate holds the workspace's unsafe code, where it is reviewed and
//! linted as such. A member whose manifest sets `unsafe_code = "forbid"`
//! cannot contain any: the compiler refuses it. `tests/unsafe_placement.rs`
//! checks that every member sets it except this crate and the members the
//! test lists by name, which still call the operating system directly.
//!
//! Everything public here is safe to call: a caller that uses only safe Rust
//! cannot cause undefined behavior through it. So a wrapper never takes a raw
//! pointer, a raw handle, or a size that the unsafe code then trusts; it
//! takes Rust values whose validity the type system already guarantees, and
//! checks anything else itself.
//!
//! Each area is a module behind a Cargo feature of the same purpose, none on
//! by default, so a consumer compiles only the bindings it uses. Every
//! `unsafe` block carries a `// SAFETY:` comment saying why it is sound;
//! clippy's `undocumented_unsafe_blocks` enforces it here, and
//! `unsafe_op_in_unsafe_fn` makes each unsafe operation inside an
//! `unsafe fn` its own block.
//!
//! | Module | Feature | What it is |
//! |---|---|---|
//! | `alloc_count` | `counting-alloc` | A global allocator that counts allocations |
//! | `thread_id` | `thread-id` | The calling thread's id as the OS knows it |

#[cfg(feature = "counting-alloc")]
pub mod alloc_count;

#[cfg(feature = "thread-id")]
pub mod thread_id;
