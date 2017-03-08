/*
 * Most of these calls are self explanatory or can be explained
 * easily when looking at the X11 documentation.
 */

use error::*;

use x11_dl::xlib;
use libc::{c_int, c_long, c_ulong, c_uint, c_char, c_uchar};
use std::ffi::CString;
use std::{mem, ptr};
use std::sync::{Arc, atomic};

struct Display(pub *mut xlib::Display);

// pointers are not Send-able across threads by default, these two lines allow us to unsafely
// override this fact (which is safe in our case: X11 API is thread safe)
unsafe impl Send for Display {}
unsafe impl Sync for Display {}

pub struct X11Helper {
    display: Display,
    // Xlib is a very large struct, so allocate it on the heap with Box
    // once instead of moving it on the stack every time
    xlib: Box<xlib::Xlib>,
    window: c_ulong,
    root_window: c_ulong,
}

impl Drop for X11Helper {
    fn drop(&mut self) {
        unsafe {
            (self.xlib.XCloseDisplay)(self.display.0);
        }
    }
}

impl X11Helper {
    pub fn new(display_name: *const c_char) -> Result<X11Helper> {
        let xlib = Box::new(xlib::Xlib::open()?);

        let display = unsafe {(xlib.XOpenDisplay)(display_name)};
        if display.is_null() {
            bail!(ErrorKind::X11Other(String::from("XOpenDisplay failed")));
        };

        let screen = unsafe { (xlib.XDefaultScreen)(display) };
        let root = unsafe {(xlib.XRootWindow)(display, screen)};

        let mut attributes: xlib::XSetWindowAttributes = unsafe { mem::zeroed() };
        attributes.background_pixel = 0; // < Set the whole 32 bits to 0,
        // making it effectively transparent for the framebuffer
        attributes.event_mask = 0;
        let mut visual_info_template : xlib::XVisualInfo = unsafe { mem::zeroed() };
        visual_info_template.depth = 32; // < this is the part which will allow us to set the alpha component of every pixel to 0
        visual_info_template.screen = unsafe {(xlib.XDefaultScreen)(display)};
        let window = unsafe {
            (xlib.XCreateWindow)(display, root,
                                 0, 0, 800, 600,
                                 0, 0,
                                 xlib::InputOutput as c_uint, ptr::null_mut(),
                                 xlib::CWBackPixel | xlib::CWEventMask, &mut attributes)
        };
        Ok(X11Helper {
            display: Display(display),
            xlib: xlib,
            window: window,
            root_window: root,
        })
    }

    pub fn set_borderless(&self, borderless: bool) -> Result<()> {
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
        let wm_hints : xlib::Atom = unsafe { (self.xlib.XInternAtom)(self.display.0, motif_wm_hints_str.as_ptr(), 1) };
        if wm_hints == 0 {
            bail!(ErrorKind::X11Other(String::from("XInternAtom returned None")));
        }
        let mwh_hints = MwmHints {
            flags: (1 << 1), // "decorations" set
            functions: 0, // should be ignored
            decorations: if borderless { 0 } else { 1 }, // will disable decorations ie "border" from window managers
            input_mode: 0, // idk what this do
            status: 0, // idk what this do
        };
        let mwh_hints_ptr =&mwh_hints as *const _ as *const u8 as *mut u8;
        // idk why 32 im just getting this from the SDL2 source code
        let r = unsafe {
            (self.xlib.XChangeProperty)(self.display.0,
                                        self.window,
                                        wm_hints,
                                        wm_hints,
                                        32,
                                        xlib::PropModeReplace,
                                        mwh_hints_ptr,
                                        (mem::size_of::<MwmHints>() / mem::size_of::<c_uchar>()) as i32)
        };
        if r == 0 {
            Ok(())
        } else {
            Err(Error::from_kind(ErrorKind::X11Internal(r as u8)))
        }
    }

    pub fn set_fullscreen(&self, fullscreen: bool) -> Result<()> {
        let wm_state_str = CString::new("_NET_WM_STATE").unwrap();
        let wm_state_fullscreen_str = CString::new("_NET_WM_STATE_FULLSCREEN").unwrap();
        let wm_state = unsafe {(self.xlib.XInternAtom)(self.display.0, wm_state_str.as_ptr(), 0)};
        let fullscreen_atom = unsafe {(self.xlib.XInternAtom)(self.display.0, wm_state_fullscreen_str.as_ptr(), 0)};
        let mut xclient_message_event : xlib::XClientMessageEvent = unsafe { mem::zeroed() };
        xclient_message_event.type_ = xlib::ClientMessage;
        xclient_message_event.window = self.window;
        xclient_message_event.message_type = wm_state;
        xclient_message_event.format = 32;
        xclient_message_event.data = xlib::ClientMessageData::new();
        {
            let mut l : &mut [c_long] = xclient_message_event.data.as_longs_mut();
            l[0] = if fullscreen { 1 } else { 0 };
            l[1] = fullscreen_atom as c_long;
        }
        let r = unsafe {
            (self.xlib.XSendEvent)(
                self.display.0,
                self.root_window, 
                0,
                xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask,
                &mut xclient_message_event as *mut _ as *mut xlib::XEvent)
        };
        if r != 0 {
            bail!(ErrorKind::X11Internal(r as u8))
        }
        Ok(())
    }

    // this is the X11 event loop.
    // We are not doing anything special in there, but we still need to run this (otherwise X11
    // doesn't do anything)
    pub fn event_loop(&self, keep_running: Arc<atomic::AtomicBool>) {
        // Hook close requests.
        let wm_delete_window_str = CString::new("WM_DELETE_WINDOW").unwrap();
        let wm_delete_window = unsafe {(self.xlib.XInternAtom)(self.display.0, wm_delete_window_str.as_ptr(), xlib::False)};

        let mut protocols = [wm_delete_window];

        unsafe {
            (self.xlib.XSetWMProtocols)(self.display.0, self.window, protocols.as_mut_ptr(), protocols.len() as c_int);

            (self.xlib.XMapWindow)(self.display.0, self.window);
        }

        // since this will be modified by XNextEvent, we dont care if its
        // initialized or not
        let mut event: xlib::XEvent = unsafe {mem::uninitialized()};

        loop {
            use std::{thread, time};

            let n_events = unsafe {(self.xlib.XPending)(self.display.0)};
            for _ in 0..n_events {
                unsafe {
                    (self.xlib.XNextEvent)(self.display.0, &mut event);
                }
            };
            if !keep_running.load(atomic::Ordering::SeqCst) {
                break;
            };
            thread::sleep(time::Duration::from_millis(50));
        }
        println!("x11_thread: shutting down ...");
    }

    pub fn show(&self) {
        unsafe {
            (self.xlib.XRaiseWindow)(self.display.0, self.window);
        }
    }

    pub fn hide(&self) {
        unsafe {
            (self.xlib.XLowerWindow)(self.display.0, self.window);
        }
    }

    pub fn set_pos(&self, x: i16, y: i16) {
        let mut window_changes : xlib::XWindowChanges = unsafe {mem::uninitialized()};
        window_changes.x = x as c_int;
        window_changes.y = y as c_int;
        let mask = xlib::CWX | xlib::CWY; // x and y
        unsafe {
            (self.xlib.XConfigureWindow)(self.display.0, self.window, mask as c_uint, &mut window_changes as *mut _);
        }
    }

    pub fn set_size(&self, w: u16, h: u16) {
        let mut window_changes : xlib::XWindowChanges = unsafe {mem::uninitialized()};
        window_changes.width = w as c_int;
        window_changes.height = h as c_int;
        let mask = xlib::CWWidth | xlib::CWHeight; // w and h
        unsafe {
            (self.xlib.XConfigureWindow)(self.display.0, self.window, mask as c_uint, &mut window_changes as *mut _);
        }
    }
}
