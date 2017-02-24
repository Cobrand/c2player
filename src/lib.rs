// nightly is required because of this
// maybe stable 1.18 will stabilize this ?
#![feature(untagged_unions)]

// necessary for error-chain
#![recursion_limit = "1024"]

extern crate libavformat;

#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate ioctl;
extern crate libc;
extern crate x11_dl;

mod utils;
mod amcodec_sys;
mod amcodec;
mod error;
mod player;
mod x11helper;
mod libavhelper;

use player::{FfiPlayer, Message};

use libc::{c_int, c_uint, c_char, c_void, c_float};
use std::mem;
use utils::*;
use error::*;

#[no_mangle]
pub extern fn aml_video_player_create() -> *mut c_void {
    // get a sender which will allow the player to
    // send messages to another thread
    let player : FfiPlayer = match player::player_start() {
        Ok(player) => player,
        Err(e) => {
            println!("Error when initializing Player : {}", e.display());
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
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let video_url = unsafe {
        ::std::ffi::CStr::from_ptr(video_url)
    };
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(
        Message::Load(tx, video_url.to_string_lossy().into_owned())
    );
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_seek(player: *mut c_void, pos: c_float) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(
        Message::Seek(tx, pos as f64)
    );
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_wait_until_end(player: *mut c_void) -> c_int {
    let mut ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let ret = ffi_player.wait_for_video_status();
    mem::forget(ffi_player);
    ret
}

#[no_mangle]
pub extern fn aml_video_player_show(player: *mut c_void) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(Message::Show(tx));
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_hide(player: *mut c_void) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(Message::Hide(tx));
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_play(player: *mut c_void) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(Message::Play(tx));
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}
#[no_mangle]
pub extern fn aml_video_player_pause(player: *mut c_void) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(Message::Pause(tx));
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_set_fullscreen(player: *mut c_void, fullscreen: c_int) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(Message::SetFullscreen(tx, fullscreen >= 1));
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_resize(player: *mut c_void, width: c_uint, height: c_uint) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(Message::SetSize(tx, (width as u16, height as u16)));
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_set_pos(player: *mut c_void, x: c_int, y: c_int) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    let (tx, rx) = single_use_channel::<FfiErrorCode>();
    ffi_player.send_message(Message::SetPos(tx, (x as i16, y as i16)));
    mem::forget(ffi_player);
    rx.recv().unwrap_or(FfiErrorCode::Disconnected) as c_int
}

#[no_mangle]
pub extern fn aml_video_player_destroy(player: *mut c_void) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    ffi_player.send_message(Message::Shutdown);
    ffi_result_to_int(ffi_player.join())
}
