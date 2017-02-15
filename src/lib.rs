
// necessary for error-chain
#![recursion_limit = "1024"]

extern crate libavformat;

#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate ioctl;
extern crate libc;
extern crate x11_dl;

#[cfg(target_arch = "aarch64")]
mod amstream;
mod error;
mod player;
mod video;
mod x11helper;
use error::*;

use player::{FfiPlayer, Message};

use std::result::Result as StdResult;
use libc::{c_int, c_char, c_void};
use std::fs::File;
use std::mem;
use std::sync::mpsc::Sender;
use libavformat::avformat_version;

#[no_mangle]
pub extern fn aml_video_player_create() -> *mut c_void {
    // get a sender which will allow the player to
    // send messages to another thread
    let player : FfiPlayer = match player::player_start() {
        Ok(player) => player,
        Err(e) => {
            println!("Error when initializing Player : {}", e);
            return ::std::ptr::null_mut();
        }
    };
    let player = Box::new(player);

    // transform Box (= unique_ptr) into a raw pointer,
    // but DO NOT free the content of it so that we can
    // retrieve it later
    Box::into_raw(player) as *mut c_void
}

#[no_mangle]
pub extern fn aml_video_player_load(player: *mut c_void, video_url: *const c_char) -> c_int {
    let sender = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    unimplemented!();
    mem::forget(sender);
    0
}

#[no_mangle]
pub extern fn aml_video_player_destroy(player: *mut c_void) -> c_int {
    println!("aml_video_player_destroy");
    let mut ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    ffi_player.send_message(Message::Shutdown);
    ffi_player.join();
    // match sender.send(C2Message::Shutdown) {
    //     Ok(_) => {
    //         //println!("Successfully shutting down");
    //         0
    //     },
    //     Err(_) => {
    //         println!("ERR: player's receiver (channel) disconnected");
    //         1
    //     },
    // }
    0
}

// #[no_mangle]
// pub extern fn run() -> c_int {
//     #[cfg(target_arch = "aarch64")]
//     {
//         let amstream_vbuf : StdResult<_,_> = File::open("/dev/amstream_vbuf");
//         let amstream_vbuf = match amstream_vbuf {
//             Ok(amstream_vbuf) => amstream_vbuf,
//             Err(_) => return 1,
//         };
//         let (major, minor) = amstream::version(&amstream_vbuf).unwrap();
//         println!("AMSTREAM VERSION {}.{}", major, minor);
//     };
//     println!("avformat version : {}",unsafe {avformat_version()});
//     0
// }
