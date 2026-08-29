# Rust bindings verification record

Date: 2026-08-29

## Version anchors

- Base branch: `neeboo/main` at
  `1b7c527ed78758cbc456dcca1c0e8e4507df798d`.
- Coinbase source: `upstream/master` at
  `a34e9e84197e6833ab89260e41fb26209eeaa69d`.
- OpenSSL: Coinbase's required custom 3.6.4 build, installed outside the
  repository and selected through `CBMPC_OPENSSL_ROOT`.
- Host used for this record: Apple arm64, AppleClang 21.0.0.

## Results

The upstream static `cbmpc` target built successfully from the imported C++
source.

The complete Rust workspace passed 11 tests with no failures:

- 3 safe-layer secret ownership and redacted-debug tests;
- 1 real secp256k1 2-of-3 lifecycle test;
- 3 callback/job tests in `cb-mpc-sys`;
- 4 allocation, out-pointer, borrowed-buffer, and null-safe-drop tests.

The lifecycle test performs three-party DKG, two-party signing, independent
`k256` verification, serialization and restoration, scalar detach and attach,
three-party refresh, then signing and verification with a different quorum.
It calls the upstream C++ implementation through the public C ABI; it does not
mock the cryptographic protocol.

The upstream ECDSA-MPC C API selection `^CApiEcdsaMp` passed all 83 tests. The
selection covers DKG, signing, refresh, general access structures, argument
validation, invalid blobs, invalid output pointers, and other negative cases.

The same 83-test selection passed again under AddressSanitizer and
UndefinedBehaviorSanitizer using the repository's `make sanitize` settings:

```text
-fsanitize=address,undefined -fno-sanitize=enum
ASAN_OPTIONS=detect_leaks=0
UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1
100% tests passed, 0 tests failed out of 83
```

`cargo fmt --check` passed. A source audit found no `unsafe` blocks in the
safe `cb-mpc` crate, no `Vec::from_raw_parts` allocation transfer, and no
internal or C++ headers in the bindgen allowlist.

## Checks not run

Valgrind was not installed on this macOS host. Rust Miri was also unavailable
for the installed stable toolchain and cannot execute the linked C++ protocol
path. ASan and UBSan therefore provide the native-memory check for this record;
neither unavailable check is reported as passing.

## Intentional migration removals

The old `cb-mpc-rs/cblib` and `cb-mpc-rs/network` crates are intentionally
removed. They bound implementation-specific C++ headers and contained the
allocator, out-pointer, lifetime, null-drop, and callback-unwind hazards listed
in `rust-bindings-upstream.md`. Their replacement is the `cb-mpc-sys` /
`cb-mpc` split documented in `cb-mpc-rs/README.md`.
