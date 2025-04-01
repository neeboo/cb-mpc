extern crate bindgen;
extern crate cc;

use std::env;
use std::path::PathBuf;

fn main() {
    // 获取项目根目录
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // 构造 cb-mpc/src 的绝对路径
    let mut src_include = PathBuf::from(&manifest_dir);
    src_include.push("../.."); // 返回到 cb-mpc
    src_include.push("src"); // 添加 src 目录

    // 转换为绝对路径并检查存在性
    let src_include = src_include
        .canonicalize()
        .expect("Failed to resolve src include path");

    println!("Using src include directory: {}", src_include.display());

    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=15.2");

    // let mut bn_cpp = PathBuf::from(&manifest_dir);
    // bn_cpp.push("../.."); // 从 cb-mpc/cb-mpc-rs/network 返回到 cb-mpc
    // bn_cpp.push("src");
    // bn_cpp.push("cbmpc");
    // bn_cpp.push("crypto");
    // bn_cpp.push("base_bn.cpp");
    // bn_cpp = bn_cpp.canonicalize().unwrap();
    // println!("www {}", bn_cpp.to_str().unwrap());
    //
    // let mod_cpp = PathBuf::from(&manifest_dir)
    //     .join("../..") // 从 cb-mpc/cb-mpc-rs/network 返回到 cb-mpc
    //     .join("src")
    //     .join("cbmpc")
    //     .join("crypto")
    //     .join("mod.cpp")
    //     .canonicalize()
    //     .unwrap();
    //
    // let callbacks_cpp = PathBuf::from(&manifest_dir)
    //     .join("../..") // 从 cb-mpc/cb-mpc-rs/network 返回到 cb-mpc
    //     .join("src")
    //     .join("cbmpc")
    //     .join("callbacks.cpp")
    //     .canonicalize()
    //     .unwrap();

    let openssl_include = PathBuf::from("/usr/local/opt/openssl@3.2.0/include");
    println!(
        "Using OpenSSL include directory: {}",
        openssl_include.display()
    );
    println!("cargo:rustc-link-search=native=/usr/local/opt/openssl@3.2.0/lib");
    println!("cargo:rustc-link-lib=dylib=crypto");

    let mut lib_include = PathBuf::from(&manifest_dir);
    lib_include.push("../.."); // 返回到 cb-mpc
    lib_include.push("lib");
    lib_include.push("Release");// 添加 lib 目录

    // 转换为绝对路径并检查存在性
    let lib_include = lib_include
        .canonicalize()
        .expect("Failed to resolve src include path");

    // println!("cargo:rustc-link-search=native={}",lib_include.to_str().unwrap());
    // println!("cargo:rustc-link-lib=static=cbmpc");

    // 编译 C++ 代码
    cc::Build::new()
        .cpp(true)
        .flag("-std=c++17")
        .include(&src_include) // 添加 cb-mpc/src 目录
        .include("include") // 添加本地 include 目录
        .include(&openssl_include)
        .files(&[
            "include/network.cpp",
            // bn_cpp.to_str().unwrap(),
            // mod_cpp.to_str().unwrap(),
            // callbacks_cpp.to_str().unwrap(),
        ])
        .compile("network"); // 输出库名为 libnetwork.a

    // 生成 Rust 绑定
    let bindings = bindgen::Builder::default()
        .header("include/network.h")
        .clang_arg(format!("-I{}", src_include.display())) // 必须传递给 bindgen
        .clang_arg("-Iinclude")
        .clang_arg(format!("-I{}", openssl_include.display())) // 添加 OpenSSL 头文件目录
        .generate()
        .expect("Failed to generate bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Failed to write bindings");
}
