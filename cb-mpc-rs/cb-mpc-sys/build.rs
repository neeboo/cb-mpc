use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to {description}: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}

fn openssl_root() -> PathBuf {
    if let Some(root) = env::var_os("CBMPC_OPENSSL_ROOT") {
        return PathBuf::from(root);
    }

    [
        "/usr/local/opt/openssl@3.6.4",
        "/opt/homebrew/opt/openssl@3.6.4",
        "/tmp/cbmpc-openssl-3.6.4",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|root| root.join("include/openssl/opensslv.h").is_file())
    .expect("Coinbase cb-mpc requires OpenSSL 3.6.4; set CBMPC_OPENSSL_ROOT")
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repository_root = manifest_dir
        .join("../..")
        .canonicalize()
        .expect("cb-mpc repository root");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let native_build = out_dir.join("native-build");
    let openssl_root = openssl_root();
    let build_type = match env::var("PROFILE").as_deref() {
        Ok("release") | Ok("bench") => "Release",
        _ => "Debug",
    };

    run(
        Command::new("cmake")
            .arg("-S")
            .arg(&repository_root)
            .arg("-B")
            .arg(&native_build)
            .arg(format!("-DCMAKE_BUILD_TYPE={build_type}"))
            .arg("-DBUILD_TESTS=OFF")
            .arg(format!("-DCBMPC_OPENSSL_ROOT={}", openssl_root.display())),
        "configure Coinbase cb-mpc",
    );
    run(
        Command::new("cmake")
            .arg("--build")
            .arg(&native_build)
            .arg("--target")
            .arg("cbmpc")
            .arg("--parallel"),
        "build Coinbase cb-mpc",
    );

    let native_lib_dir = repository_root.join("lib").join(build_type);
    let openssl_lib_dir = if openssl_root.join("lib/libcrypto.a").is_file() {
        openssl_root.join("lib")
    } else {
        openssl_root.join("lib64")
    };

    println!(
        "cargo:rustc-link-search=native={}",
        native_lib_dir.display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        openssl_lib_dir.display()
    );
    println!("cargo:rustc-link-lib=static=cbmpc");
    println!("cargo:rustc-link-lib=static=crypto");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
        println!("cargo:rustc-link-lib=framework=CoreServices");
        println!("cargo:rustc-link-lib=framework=IOKit");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=dl");
    }

    let bindings = bindgen::Builder::default()
        .header(manifest_dir.join("wrapper.h").to_string_lossy())
        .clang_arg(format!("-I{}", repository_root.join("include").display()))
        .allowlist_type("cmem_t")
        .allowlist_type("cmems_t")
        .allowlist_type("cbmpc_.*")
        .allowlist_function("cbmpc_.*")
        .allowlist_var("CBMPC_.*")
        .derive_default(true)
        .generate()
        .expect("generate bindings for the public cb-mpc C API");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("write cb-mpc bindings");

    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("wrapper.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repository_root.join("include/cbmpc/c_api").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repository_root.join("src/cbmpc/c_api").display()
    );
    println!("cargo:rerun-if-env-changed=CBMPC_OPENSSL_ROOT");
}

#[allow(dead_code)]
fn _assert_path(_: &Path) {}
