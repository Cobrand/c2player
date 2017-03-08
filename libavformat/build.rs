extern crate bindgen;

use std::env;
use std::path::Path;

fn main() {
    if cfg!(feature="generate_avformat_rs") {
        let out_dir = env::var("OUT_DIR").unwrap();
        let aarch64 : bool = match env::var("TARGET") {
            Ok(target) => target == "aarch64-unknown-linux-gnu",
            // env variable not found
            Err(_) => false,
        };
        if aarch64 {
            let _ = bindgen::builder()
                .header("/usr/aarch64-linux-gnu/usr/include/aarch64-linux-gnu/libavformat/avformat.h")
                .clang_arg("-I/usr/aarch64-linux-gnu/usr/include/aarch64-linux-gnu/")
                .generate().unwrap()
                .write_to_file(Path::new(&out_dir).join("avformat.rs"));
        } else {
            let _ = bindgen::builder()
                .header("/usr/include/libavformat/avformat.h")
                .generate().unwrap()
                .write_to_file(Path::new(&out_dir).join("avformat.rs"));
        }
    }
    println!("cargo:rustc-flags=-l avformat");
}
