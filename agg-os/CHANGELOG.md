# Changelog

All notable changes to `agg-os` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

## [0.1.0]

### Added

- `counting-alloc` feature: `alloc_count::CountingAlloc`, a global allocator that forwards every
  call to the system allocator and counts each allocation and reallocation, and
  `alloc_count::allocations()`, the count so far.
- `thread-id` feature: `thread_id::current_thread_os_id()`, the calling thread's id as a dump
  records it (its Mach port name on macOS, its kernel tid on Linux, 0 elsewhere).
