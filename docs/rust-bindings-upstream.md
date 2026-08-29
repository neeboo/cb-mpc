# Rust bindings upstream anchor

The Rust bindings in this branch target Coinbase `cb-mpc` commit:

```text
a34e9e84197e6833ab89260e41fb26209eeaa69d
```

The branch starts from `neeboo/main` (local remote name `origin`) at:

```text
1b7c527ed78758cbc456dcca1c0e8e4507df798d
```

Those repositories have unrelated Git histories. The C++ source, build files,
public headers, tests, and vendor references are imported as an auditable tree
update. The `cb-mpc-rs/` directory is maintained on top of that source tree.

## ABI boundary

Rust binds only headers under `include/cbmpc/c_api/`. It does not bind headers
from `include-internal/`, `include/cbmpc/api/`, or C++ classes.

The initial safe wrapper uses these public symbols:

- `cbmpc_malloc`, `cbmpc_free`, `cbmpc_cmem_free`, `cbmpc_cmems_free`
- `cbmpc_ecdsa_mp_dkg_ac`
- `cbmpc_ecdsa_mp_refresh_ac`
- `cbmpc_ecdsa_mp_sign_ac`
- `cbmpc_ecdsa_mp_get_public_key_compressed`
- `cbmpc_ecdsa_mp_get_public_share_compressed`
- `cbmpc_ecdsa_mp_detach_private_scalar`
- `cbmpc_ecdsa_mp_attach_private_scalar`

All C output buffers are copied before being released through the matching
Coinbase C API free function. Rust never constructs a `Vec` from a pointer
allocated by C or C++.

Transport callbacks allocate receive buffers with `cbmpc_malloc`; the upstream
transport adapter returns them through the callback's `free` function. Callback
panics are caught in the sys crate and converted to a C error code.

## Build dependency

This upstream snapshot requires its custom OpenSSL 3.6.4 build. The Rust sys
crate accepts `CBMPC_OPENSSL_ROOT`; the referenced directory must contain
`include/openssl` and `lib/libcrypto.a` (or `lib64/libcrypto.a` on Linux).

The implementation remains the upstream C++ cryptographic core. The Rust crates
are bindings and safe ownership/lifetime adapters; they are not a pure-Rust MPC
implementation.

## Legacy Rust wrapper migration

The old `cb-mpc-rs/cblib` and `cb-mpc-rs/network` crates are deliberately
deleted on the refresh branch. Their API targeted internal C++ classes and a
committed, platform-specific bindgen snapshot. It also contained invalid
out-pointer calls, null dereferences during `Drop`, callback allocator transfer
through `Vec::from_raw_parts`, and flattened C views whose Rust backing vectors
were dropped before the FFI call.

The replacement workspace is split into `cb-mpc-sys` and `cb-mpc`. Only the sys
crate contains generated C declarations or `unsafe`; the safe crate exchanges
owned byte vectors and typed errors.
