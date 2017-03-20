# c2player

Name is subject to change

## Introduction

This is a library made in Rust which aims to have a public C ABI. Hence while the library is made in Rust, it can be used in C, C++, Python, Java, ... or more precisely any language that supports the C ABI.

This project was made for a school project in an french Engineering School. The aim of this sub-repo was to:

* Have a very simple API in C to allow usage from (almost) any language
* Play videos only (for now it is only HEVC videos, but AVC support can be added without much hassle), NO support for audio and NO support for subtitles
* Allow video loading, window resizing, ... on the fly via the C API

Most of how this works is explained in the code itself. `aml_player.h` is what you should include in your C programs if you link `libc2player.so` (from `/target/*/libc2player.so`). The comments are in french but the names of the functions are self explanatory.

**THIS PROJECT IS NOT MAINTAINED ANYMORE**. While I won't maintain this anymore (mostly because I don't have an odroid c2 device with me anymore), I still wrote the code and will answer any question if asked.

## Build

This will only show how to install everything to be set up on Linux only.

You can of course install Rust, Rustup and cross-compile from any windows machine (theorically), but
this has never been tested and will not be explained here. The setup should be roughly the same,
except that the filesystem is of course different in `libavformat/build.rs`.

### Installing Rust

Go to [rustup.rs](https://rustup.rs) and install rustup. This is basically a version manager for the rust compiler:
you can make it so it compile from the stable compiler, from the beta compiler, from the nightly compiler, ... it
also supports cross-compiling. (`rustup target list` for a preview)

It is possible that rustup exists in your package manager as well. This is notably true for Arch-based distribs, where
rustup is simply available under `rustup`.

```sh
$ rustup install nightly
$ cd c2player
$ rustup override nightly
$ cargo build
```

or 

```
cargo build --release
```

for optimisations.

This should build the project as a `.so`. That you can either find in `target/debug/libc2player.so` or in `target/release/libc2player.so`.

Everything you need to link to those so is in `aml_player.h` in the root directory of this repository.

Nightly is actually required because current stable rust doesn't support C union (it only supports tagged union,s which C does not have), so we have to use nightly to enable this experimental feature (which is on its way to stabilization).

There is however one major drawback with this version, it is that it can only compile and run with libavformat version 56 (which is the default one on the ODROID C2 if Ubuntu LTS is installed).
If you want to use another version than version 56 for libavformat, you will need to build with this command : `cargo build --features "libavformat/generate_avformat_rs"`. This can unfortunately fail on the aarch64 architecture, the exact reasons are unknown, but it looks like it's has to do with the fact that this architecture can install and supports multiple architectures at once, which are in totally separate folders.

You can get away with that by cross-compiling from an x86\_64 environment. See "Cross-compiling" for a very basic guideline.

To "generate" the file required to make libavformat work with other versions than v56, you will need a certain version of clang. See "Installing clang" for more info.

### Installing clang

See the [rust-bindgen repository](https://github.com/servo/rust-bindgen) to install the clang needed to use bindgen.

### Cross-compiling

This is merely a guideline, and things might (and probably will) change on your computer. Cross-compiling is a very complex issue, and while rust makes it kind of easier, it is still very hard to deal with.

```
$ rustup target install aarch64-unknown-linux-gnu
```

rustup needs to know where to find a few things to cross compile (most notably a linker), so you will need to put this in your `~/.cargo/config`:

```
[target.aarch64-unknown-linux-gnu]
linker = "/usr/bin/aarch64-linux-gnu-gcc"
ar = "/usr/bin/aarch64-linux-gnu-ar"
```

```
$ cargo build --target aarch64-unknown-linux-gnu
```

This command will probably abort at the link stage because it could not find libavformat.so. It might be another library, but it will probably be a linker error. You have 2 possibilities:

* Pray that the equivalent of `aarch64-libavformat` exists in your package manager (it probably doesn't)
* Copy libavformat.so from your target system to your build system, probably in `/usr/aarch64-linxu-gnu`

For that second step, libavformat has also a bunch of dependencies, so a dirty-but-effective way to make it working is to copy every library from your target environment to your build host environment. Something like `scp -r odroid@XX.XX.XX.XX:/usr/lib/ /usr/aarch64-linux-gnu/usr/lib`

Bindgen may require libavformat headers as well if you want to build with the feature `libavformat/generate_avformat_rs`, so you may want to copy /usr/include from your host system into your `/usr/aarch64-linux-gnu/usr/include` directory as well.

Please note that this is a VERY unconventional way to do it, and doing these steps might still result in an unsuccessful compilation.

# Testing

```
cp target/debug/libc2player.so .
make
./test_c2player
```

You can change test.c to your heart's content. This is only a basic test for developement and debugging purposes.

# Known Issues

* "failed to set x11 window borderless: Error: internal X11 error: 1". It can also happen in other various functions. This message doesn't really matter in the end, (ans we're not 100% this is an error at all, but according to x11 it is), since even though it's displayed as "failed", it still succeeded.
* Cursor is not transparent when hovering the X11 window
* There are 3 to 4 seconds of lag when seeking or loading another file. There must be another way to avoid this lag at least when seeking, but we have yet to find it.
* `player\_show()` when done on a non-fullscreen window will pop up the top and bottom panel. This is an issue with the use of X11 in the software, we might be using XMapWindow wrongly.

# Not tested

What happens when the VPU's internal buffer is full has not been tested. The VPU's internal buffer is large enough for most videos under 2 to 3 minutes long, so this can be considered a minor issue.

# License

This code in under the MIT license, but some things like libavformat are under other licenses (the LGPL license for instance).
