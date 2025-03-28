extern crate bindgen;
extern crate cc;

use std::env;
use std::path::PathBuf;

fn main() {
    // 获取项目根目录
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // 构造 cmem.h 的绝对路径
    let mut cmem_include = PathBuf::from(&manifest_dir);
    cmem_include.push("../.."); // 调整层级
    cmem_include.push("src");
    cmem_include.push("cbmpc");
    cmem_include.push("core");

    // 转换为绝对路径并检查存在性
    let cmem_include = cmem_include
        .canonicalize()
        .expect("Failed to resolve cmem.h path");

    println!("Using cmem.h include directory: {}", cmem_include.display());

    // 编译 C++ 代码
    cc::Build::new()
        .cpp(true)
        .flag("-std=c++14")
        .include(&cmem_include) // 添加 cmem.h 所在目录
        .include("include")     // 添加本地 include 目录
        .file("include/network.cpp")
        .compile("network");    // 输出库名为 libnetwork.a

    // 生成 Rust 绑定
    let bindings = bindgen::Builder::default()
        .header("include/network.h")
        .clang_arg(format!("-I{}", cmem_include.display())) // 必须传递给 bindgen
        .clang_arg("-Iinclude")
        .generate()
        .expect("Failed to generate bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Failed to write bindings");
}