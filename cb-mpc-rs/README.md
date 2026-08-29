# Coinbase cb-mpc Rust bindings

This workspace wraps Coinbase cb-mpc's public C API. The cryptographic core is
C++17; these crates do not claim to be a pure-Rust MPC implementation.

## Crates

- `cb-mpc-sys`: bindgen output, C allocator ownership, raw output slots,
  transport callbacks, panic barriers, and calls to `include/cbmpc/c_api/`.
- `cb-mpc`: safe threshold-policy, key-share, session, refresh, signing, and
  backup/restore types.

Private key blobs and detached scalars are held in `SecretBytes`. It has
redacted `Debug` output, is not `Clone`, and zeroizes its allocation before
deallocation. `EcdsaKeyShare::to_bytes` returns `SecretBytes`, so serialized
shares do not silently become ordinary, non-zeroizing vectors.

The previous `cblib` and `network` crates were intentionally removed. They
bound C++ implementation headers, committed target-specific generated bindings,
allowed callback panics to cross FFI, handed callback allocations to
`Vec::from_raw_parts`, and created dangling `cmem_t`/`cmems_t` views. They are
not compatibility layers for the current Coinbase C API.

## Build

Coinbase cb-mpc currently requires its custom OpenSSL 3.6.4 build. Build it with
the repository script for your platform, then set the installation root:

```console
export CBMPC_OPENSSL_ROOT=/path/to/openssl-3.6.4
cargo test --workspace
```

`cb-mpc-sys/build.rs` builds the upstream static `cbmpc` target and generates
narrow bindings from the public C headers for the active Rust target. It does
not expose any C++ ABI.

## Catomicals integration boundary

An orchestration backend implements this safe Rust trait:

```rust
pub trait ThresholdSigner {
    type Error: std::error::Error + Send + Sync + 'static;

    fn public_key_compressed(&self) -> Result<Vec<u8>, Self::Error>;
    fn sign_prehash(&self, digest: [u8; 32]) -> Result<Vec<u8>, Self::Error>;
}
```

Catomicals should depend on the safe crate only:

```toml
[dependencies]
cb-mpc = { path = "../cb-mpc/cb-mpc-rs/cb-mpc" }
```

The backend owns party selection, network transport, session coordination, and
key-share storage. Catomicals receives compressed SEC1 public keys and DER ECDSA
signatures; it never handles C pointers or native allocation.

## Real protocol test

`cb-mpc/tests/ecdsa_2_of_3.rs` runs the real upstream secp256k1 implementation:

1. three-party DKG with a 2-of-3 access structure;
2. two-party signing and independent `k256` DER verification;
3. serialized key-share restoration and detached-scalar reattachment;
4. three-party refresh preserving the public key;
5. signing and verification with a different two-party quorum after refresh.
