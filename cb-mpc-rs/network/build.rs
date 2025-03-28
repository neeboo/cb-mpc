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
    src_include.push("src");    // 添加 src 目录

    // 转换为绝对路径并检查存在性
    let src_include = src_include
        .canonicalize()
        .expect("Failed to resolve src include path");

    println!("Using src include directory: {}", src_include.display());

    let openssl_include = PathBuf::from("/usr/local/opt/openssl@3.2.0/include");
    println!("Using OpenSSL include directory: {}", openssl_include.display());

    // 编译 C++ 代码
    cc::Build::new()
        .cpp(true)
        .flag("-std=c++14")
        .include(&src_include) // 添加 cb-mpc/src 目录
        .include("include")    // 添加本地 include 目录
        .include(&openssl_include)
        .file("include/network.cpp")
        .compile("network");   // 输出库名为 libnetwork.a

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
