use error::*;

use x11_dl::xlib;
use libc::{c_int, c_long, c_ulong, c_uint, c_char, c_uchar};
use std::ffi::CString;
use std::{mem, ptr};
use std::sync::{Arc, atomic};
use std::sync::mpsc::{Receiver, TryRecvError};
use super::player::Message;

pub struct XDisplay(usize, Arc<xlib::Xlib>);

impl Drop for XDisplay {
    fn drop(&mut self) {
        unsafe {
            (self.1.XCloseDisplay)(self.0 as *mut xlib::Display);
        }
    }
}

impl XDisplay {
    pub fn new(xlib: Arc<xlib::Xlib>, display_name: *const c_char) -> Result<XDisplay> {
        let display = unsafe {(xlib.XOpenDisplay)(display_name)};
        if display.is_null() {
            bail!("XOpenDisplay failed");
        };
        Ok(XDisplay(display as usize, xlib))
    }

    pub fn as_ptr(&self) -> *mut xlib::Display {
        self.0 as *mut xlib::Display
    }
}

pub fn set_window_borderless(xlib: &xlib::Xlib, display: *mut xlib::Display, window: c_ulong) {
    // according to http://stackoverflow.com/a/1909708/3731958
    // this method to hide borders with x11 is deprecated, but it still works 
    // so whatever
    #[repr(C)]
    struct MwmHints {
        pub flags: c_ulong,
        pub functions: c_ulong,
        pub decorations: c_ulong,
        pub input_mode: c_long,
        pub status: c_ulong
    };
    let motif_wm_hints_str = CString::new("_MOTIF_WM_HINTS").unwrap();
    let wm_hints : xlib::Atom = unsafe { (xlib.XInternAtom)(display, motif_wm_hints_str.as_ptr(), 1) };
    if wm_hints != 0 {
        let mwh_hints = MwmHints {
            flags: (1 << 1), // "decorations" set
            functions: 0, // should be ignored
            decorations: 0, // will disable decorations ie "border" from window managers
            input_mode: 0, // idk what this do
            status: 0, // idk what this do
        };
        let mut mwh_hints_ptr = unsafe { &mwh_hints as *const _ as *const u8 as *mut u8 };
        // idk why 32 im just getting this from the SDL2 source code
        unsafe {
            (xlib.XChangeProperty)(display,
                                   window,
                                   wm_hints,
                                   wm_hints,
                                   32,
                                   xlib::PropModeReplace,
                                   mwh_hints_ptr,
                                   (mem::size_of::<MwmHints>() / mem::size_of::<c_uchar>()) as i32);
        }
    } else {
        println!("None returned for XInternAtom(\"MOTIF_WM_HINTS\")");
    }
}

pub fn set_window_fullscreen(xlib: &xlib::Xlib, display: *mut xlib::Display, root_window: c_ulong, window: c_ulong, fullscreen: bool) {
    let wm_state_str = CString::new("_NET_WM_STATE").unwrap();
    let wm_state_fullscreen_str = CString::new("_NET_WM_STATE_FULLSCREEN").unwrap();
    let wm_state = unsafe {(xlib.XInternAtom)(display, wm_state_str.as_ptr(), 0)};
    let fullscreen_atom = unsafe {(xlib.XInternAtom)(display, wm_state_fullscreen_str.as_ptr(), 0)};
    let mut xclient_message_event : xlib::XClientMessageEvent = unsafe { mem::zeroed() };
    xclient_message_event.type_ = xlib::ClientMessage;
    xclient_message_event.window = window;
    xclient_message_event.message_type = wm_state;
    xclient_message_event.format = 32;
    //xclient_message_event.data.l[0] = 1;
    //xclient_message_event.data.l[1] = fullscreen;
    xclient_message_event.data = xlib::ClientMessageData::new();
    {
        let mut l : &mut [c_long] = xclient_message_event.data.as_longs_mut();
        l[0] = if fullscreen { 1 } else { 0 };
        l[1] = fullscreen_atom as c_long;
    }
    unsafe {
        (xlib.XSendEvent)(display,
                          root_window, 
                          0,
                          xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask,
                          &mut xclient_message_event as *mut _ as *mut xlib::XEvent);
    }
}

pub fn create_window(xlib: &xlib::Xlib, display: *mut xlib::Display, root: c_ulong) -> c_ulong {
    let mut attributes: xlib::XSetWindowAttributes = unsafe { mem::zeroed() };
    attributes.background_pixel = 0;
    attributes.event_mask = 0; //xlib::StructureNotifyMask ;

    let mut visual_info_template : xlib::XVisualInfo = unsafe { mem::zeroed() };
    let mut visual_info_number : c_int = 0;
    visual_info_template.depth = 32;
    visual_info_template.screen = unsafe {(xlib.XDefaultScreen)(display)};
    let visual_info_ptr = unsafe {(xlib.XGetVisualInfo)(display, xlib::VisualDepthMask, &mut visual_info_template, &mut visual_info_number)};
    let visual_info = unsafe {*visual_info_ptr.offset(0)};

    let window = unsafe {
        (xlib.XCreateWindow)(display, root,
                             0, 0, 800, 600,
                             0, 0,
                             xlib::InputOutput as c_uint, ptr::null_mut(),
                             xlib::CWBackPixel | xlib::CWEventMask, &mut attributes)
    };
    window
}

pub fn event_loop(xlib: Arc<xlib::Xlib>, display: Arc<XDisplay>, window: c_ulong, keep_running: Arc<atomic::AtomicBool>) {
    // Hook close requests.
    let wm_protocols_str = CString::new("WM_PROTOCOLS").unwrap();
    let wm_delete_window_str = CString::new("WM_DELETE_WINDOW").unwrap();

    let wm_protocols = unsafe {(xlib.XInternAtom)(display.as_ptr(), wm_protocols_str.as_ptr(), xlib::False)};
    let wm_delete_window = unsafe {(xlib.XInternAtom)(display.as_ptr(), wm_delete_window_str.as_ptr(), xlib::False)};

    let mut protocols = [wm_delete_window];

    unsafe {
        (xlib.XSetWMProtocols)(display.as_ptr(), window, protocols.as_mut_ptr(), protocols.len() as c_int);

        // Show window.
        (xlib.XMapWindow)(display.as_ptr(), window);
    }

    // since this will be modified by XNextEvent, we dont care if its
    // initialized or not
    let mut event: xlib::XEvent = unsafe {mem::uninitialized()};

    loop {
        use std::{thread, time, sync};

        let n_events = unsafe {(xlib.XPending)(display.as_ptr())};
        for _ in 0..n_events {
            unsafe {
                (xlib.XNextEvent)(display.as_ptr(), &mut event);
            }
            println!("XNextEvent:{} : {:?}", event.get_type(), event.pad);
        };
        if !keep_running.load(atomic::Ordering::SeqCst) {
            break;
        };
        thread::sleep(time::Duration::from_millis(50));
    }
    println!("x11 thread: shutting down");
}
