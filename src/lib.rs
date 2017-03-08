/*
 * This file is the interface which will be public in the .so library
 */


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

// When this function is called, a struct named FfiPlayer is crated,
// initialized and allocated on the Heap. Its initialization takes
// care of spawning other threads which will communicate between each
// others. 
//
// Box is the equivalent to a unique_ptr in C++, so we must ensure that
// our FfiPlayer allocated on the heap will not be deallocated here (because
// we need it in future calls). `into_raw` noth transforms into a pointer and forgets
// memory-wise the Box, so it isn't deallocated right now
#[no_mangle]
pub extern fn aml_video_player_create() -> *mut c_void {
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

// For almost every other call, we need to retrieve FfiPlayer from the given pointer. It is of
// course very risky since the API user can send us a totally unrelated pointer, but we don't
// really have a choice here ...
//
// Since the command (or Message) is sent to another thread, we get an answer right away saying
// that "the message has been sent", but we would like to know if the command that we just did
// failed or not (for instance with load, if the file exists, ...)
//
// That is why we are sender along with our message a way for someone in another thread to send us
// a status code. We will block this part of the thread until we get an answer via this "Single Use
// Channel" from another thread.
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

// This function is rather special, since we are blocking until an "end of video" message is sent
// to us. Basically this message (which is at the moment always returned when the VPU hits EOF)
// allows us to get the exact moment where a video is finished, so that we can queue the next one
// right up, or shutdown the program right after the video's done.
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

// this is the opposite from "create", we are dereferencing the given pointer,
// sending a Shutdown message (more on that in player.rs), and then we wait for every thread to
// finish and return the appropiate status code if some threads failed to finish properly.
//
// The FfiPlayer allocated on the Heap is deallocated automatically at the end of this function,
// because its destructor deallocates the memory in this case.
#[no_mangle]
pub extern fn aml_video_player_destroy(player: *mut c_void) -> c_int {
    let ffi_player = unsafe {Box::from_raw(player as *mut FfiPlayer)};
    ffi_player.send_message(Message::Shutdown);
    ffi_result_to_int(ffi_player.join())
}
