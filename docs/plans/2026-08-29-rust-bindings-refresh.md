# Rust Bindings Refresh Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the legacy C++-ABI Rust wrapper with an allocator-safe binding to Coinbase cb-mpc's current public C API and prove a real 2-of-3 secp256k1 ECDSA-MPC lifecycle.

**Architecture:** The `cb-mpc-sys` crate is the only crate that sees bindgen output, raw pointers, C allocation functions, or transport callbacks. The `cb-mpc` crate exposes owned key/session blobs, a threshold policy, and a panic-safe `Transport` trait; it contains no C++ ABI. The C++ implementation at upstream commit `a34e9e84197e6833ab89260e41fb26209eeaa69d` remains the cryptographic source.

**Tech Stack:** Rust 2021, bindgen, CMake/C++17, Coinbase cb-mpc C API, OpenSSL 3, k256 verification, crossbeam-channel test transport.

---

### Task 1: Anchor and import the upstream C API

**Files:**
- Modify: upstream C++ source and public headers to match `upstream/master`
- Create: `docs/rust-bindings-upstream.md`

1. Record origin and upstream commit IDs and the public C functions used by Rust.
2. Replace the legacy core tree with the exact upstream tree while retaining `cb-mpc-rs/`.
3. Build the upstream `cbmpc` static target with tests disabled.
4. Commit the source anchor independently.

### Task 2: Lock the sys memory contract with failing tests

**Files:**
- Replace: `cb-mpc-rs/Cargo.toml`
- Create: `cb-mpc-rs/cb-mpc-sys/Cargo.toml`
- Create: `cb-mpc-rs/cb-mpc-sys/build.rs`
- Create: `cb-mpc-rs/cb-mpc-sys/wrapper.h`
- Create: `cb-mpc-rs/cb-mpc-sys/src/lib.rs`
- Create: `cb-mpc-rs/cb-mpc-sys/tests/memory_contract.rs`

1. Write compile/runtime tests requiring an output slot passed by mutable address, borrowed `cmem_t`/`cmems_t` values that retain backing buffers, and library-owned output that is copied before `cbmpc_cmem_free`.
2. Run the targeted test and confirm failure because the safe sys types do not exist.
3. Generate allowlisted C bindings and compile/link the upstream static library.
4. Implement only the memory view/owner types needed by the tests.
5. Run the targeted test and confirm it passes.

### Task 3: Lock callback and job lifetime behavior

**Files:**
- Modify: `cb-mpc-rs/cb-mpc-sys/src/lib.rs`
- Create: `cb-mpc-rs/cb-mpc-sys/tests/transport_contract.rs`

1. Write failing tests for send/receive/receive-all callbacks, allocation/free symmetry, null-safe drops, and panic containment.
2. Run the tests and confirm the expected missing behavior.
3. Implement `Transport`, callback trampolines using `catch_unwind`, and a borrowed `MpJob` whose names, context, transport table, and pointers live through the C call.
4. Confirm callback panics become C error codes and no panic crosses the FFI boundary.
5. Commit the sys boundary independently.

### Task 4: Add the safe ECDSA-MP API

**Files:**
- Create: `cb-mpc-rs/cb-mpc/Cargo.toml`
- Create: `cb-mpc-rs/cb-mpc/src/lib.rs`
- Create: `cb-mpc-rs/cb-mpc/tests/ecdsa_2_of_3.rs`

1. Write a real 2-of-3 test that requires threshold DKG, public-key agreement, DER signing on one receiver, external secp256k1 verification, key refresh with the same public key, blob serialization/restoration, and detached-share reattachment.
2. Run it and confirm failure because the safe API is missing.
3. Implement `ThresholdPolicy`, `EcdsaKeyShare`, `SessionId`, DKG/sign/refresh/public-key/detach/attach methods over `cb-mpc-sys`.
4. Run the full lifecycle test to green without mocked cryptography.
5. Commit the safe API and real protocol test independently.

### Task 5: Verify the migration

**Files:**
- Modify: `cb-mpc-rs/README.md`

1. Document that the Rust crates wrap the upstream C API and are not a pure-Rust cryptographic implementation.
2. Run `cargo fmt --check`, targeted sys tests, the real ECDSA test, and the complete Rust workspace tests.
3. Run the upstream public C API ECDSA tests and AddressSanitizer/UndefinedBehaviorSanitizer when supported; record any unavailable Valgrind check explicitly.
4. Inspect the final diff for legacy C++ ABI exposure, `Vec::from_raw_parts`, unchecked callback panics, or allocator mismatches.
5. Commit documentation and verification metadata without pushing.
