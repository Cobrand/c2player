use error::*;
use super::x11helper::{self, XDisplay};
use super::video::VideoEndReason;

use std::fs::File;
use std::sync::{Arc, Mutex, atomic};
use std::{ptr, mem, thread};
use std::sync::mpsc::{self, Receiver, Sender};
use x11_dl::xlib;
use libc::{c_int, c_long, c_ulong, c_void, c_char};
use std::ffi::CString;
use std::thread::JoinHandle;

pub struct FfiPlayer {
    main_thread: JoinHandle<()>,
    x11_event_loop_thread: JoinHandle<()>,
    video_status_queue: Receiver<VideoEndReason>,
    sender: Sender<Message>,
    keep_running: Arc<atomic::AtomicBool>,
}

impl FfiPlayer {
    pub fn new(main_thread: JoinHandle<()>, x11_thread: JoinHandle<()>, video_rx: Receiver<VideoEndReason>, sender: Sender<Message>, keep_running: Arc<atomic::AtomicBool>) -> FfiPlayer {
        FfiPlayer {
            main_thread: main_thread,
            x11_event_loop_thread: x11_thread,
            video_status_queue: video_rx,
            sender: sender,
            keep_running: keep_running,
        }
    }

    pub fn join(self) {
        if let Err(_) = self.main_thread.join() {
            println!("Main Thread panicked");
        };
        if let Err(_) = self.x11_event_loop_thread.join() {
            println!("X11 Event Thread panicked");
        };
    }

    pub fn send_message(&self, message: Message) -> bool {
        match self.sender.send(message) {
            Ok(_) => true,
            Err(e) => {
                println!("Receiving end of the channel disconnected: {}", e);
                false
            }
        }
    }

    pub fn wait_for_video_status(&mut self) -> c_int {
        match self.video_status_queue.recv() {
            Ok(VideoEndReason::Error) => 1,
            Ok(VideoEndReason::EOF) => 0,
            Err(e) => {
                println!("Video status channel disconnected : {}", e);
                -1
            }
        }
    }
}

#[derive(Debug)]
pub enum Message {
    SetSize(u16, u16),
    SetPos(i16, i16),
    SetFullscreen(bool),
    Shutdown
}

// pub struct Player {
//     // X11 properties
//     pub xlib: Arc<xlib::Xlib>,
//     pub screen: c_int,
//     pub display: Arc<x11helper::XDisplay>,
//     pub window: c_ulong,

//     #[cfg(target_arch = "aarch64")]
//     // amlogic
//     pub amstream_vbuf: File,
// }

fn x11_init_event_loop_thread(xlib: &Arc<xlib::Xlib>, display: &Arc<XDisplay>, window: c_ulong, keep_running: Arc<atomic::AtomicBool>) -> JoinHandle<()> {
    let display = display.clone();
    let xlib = xlib.clone();
    thread::spawn(move || {
        x11helper::event_loop(xlib, display, window, keep_running);
    })
}

pub fn player_start() -> Result<FfiPlayer> {
    let xlib = xlib::Xlib::open()?;
    let xlib = Arc::new(xlib);
    
    // Open display connection.
    let display = Arc::new(x11helper::XDisplay::new(xlib.clone(), ptr::null())?);

    // Create window.
    let screen = unsafe { (xlib.XDefaultScreen)(display.as_ptr()) };
    let root = unsafe {(xlib.XRootWindow)(display.as_ptr(), screen)};
    
    let window = x11helper::create_window(&xlib, display.as_ptr(), root);

    x11helper::set_window_borderless(&xlib, display.as_ptr(), window);

    // Set window title.
    let title_str = CString::new("hello-world").unwrap();
    unsafe {
        (xlib.XStoreName)(display.as_ptr(), window, title_str.as_ptr() as *mut c_char);
    }

    let (sender, receiver) = mpsc::channel::<Message>();
    let (video_status_sender, video_status_rx) = mpsc::channel::<VideoEndReason>();
    let x11_sender = sender.clone();
    let keep_running = Arc::new(atomic::AtomicBool::new(true));
    let x11_thread = x11_init_event_loop_thread(&xlib, &display, window, keep_running.clone());

    let main_thread = {
        let keep_running = keep_running.clone();
        thread::spawn(move || {
            'mainloop: for message in receiver.iter() {
                println!("Main thread: received message {:?}", message);
                match message {
                    Message::Shutdown => {break 'mainloop},
                     => {}
                };
            };
            keep_running.store(false, atomic::Ordering::SeqCst);
            println!("Finishing main loop ...");
        })
    };

    Ok(FfiPlayer::new(main_thread, x11_thread, video_status_rx, sender, keep_running))
}

// impl Player {
//     pub fn new() -> Result<Player> {
//         let xlib = xlib::Xlib::open()?;
//         let xlib = Arc::new(xlib);
        
//         // Open display connection.
//         let display = Arc::new(x11helper::XDisplay::new(xlib.clone(), ptr::null())?);

//         // Create window.
//         let screen = unsafe { (xlib.XDefaultScreen)(display.as_ptr()) };
//         let root = unsafe {(xlib.XRootWindow)(display.as_ptr(), screen)};
        
//         let window = x11helper::create_window(&xlib, display.as_ptr(), root);

//         x11helper::set_window_borderless(&xlib, display.as_ptr(), window);

//         // Set window title.
//         let title_str = CString::new("hello-world").unwrap();
//         unsafe {
//             (xlib.XStoreName)(display.as_ptr(), window, title_str.as_ptr() as *mut c_char);
//         }

//         // // Hook close requests.
//         // let wm_protocols_str = CString::new("WM_PROTOCOLS").unwrap();
//         // let wm_delete_window_str = CString::new("WM_DELETE_WINDOW").unwrap();

//         // let wm_protocols = unsafe {(xlib.XInternAtom)(display.as_ptr(), wm_protocols_str.as_ptr(), xlib::False)};
//         // let wm_delete_window = unsafe {(xlib.XInternAtom)(display.as_ptr(), wm_delete_window_str.as_ptr(), xlib::False)};

//         // let mut protocols = [wm_delete_window];

//         // unsafe {
//         //     (xlib.XSetWMProtocols)(display.as_ptr(), window, protocols.as_mut_ptr(), protocols.len() as c_int);

//         //     // Show window.
//         //     (xlib.XMapWindow)(display.as_ptr(), window);
//         // }
        
//         // x11helper::set_window_fullscreen(&xlib, display.as_ptr(), root, window, true);
        
//         // {
//         //     // cast to usize is required because ptrs cannot be sent between threads
//         //     // in normal circumstances.
//         //     // This OP is basically unsafe
//         //     let display = display.clone();
//         //     let xlib = xlib.clone();
//         //     thread::spawn(move || {
//         //         // since this will be modified by XNextEvent, we dont care if its
//         //         // initialized or not
//         //         let mut event: xlib::XEvent = unsafe {mem::uninitialized()};

//         //         loop {
//         //             unsafe {
//         //                 (xlib.XNextEvent)(display.as_ptr(), &mut event);
//         //             }

//         //             println!("{} : {:?}", event.get_type(), event.pad);
//         //             // {
//         //             //     xlib::ClientMessage => {
//         //             //         let xclient = xlib::XClientMessageEvent::from(event);

//         //             //         if xclient.message_type == wm_protocols && xclient.format == 32 {
//         //             //             let protocol = xclient.data.get_long(0) as xlib::Atom;

//         //             //             if protocol == wm_delete_window {
//         //             //                 break;
//         //             //             }
//         //             //         }
//         //             //     },
//         //             //     _ => ()
//         //             // }
//         //         }
//         //     });
//         // }

//         #[cfg(target_arch = "aarch64")]
//         {
//             let amstream_vbuf = File::open("/dev/amstream_vbuf")
//                 .chain_err(|| "Unable to open /dev/amstream_vbuf")?;
//             Ok(Player {
//                 xlib: xlib,
//                 screen: screen,
//                 display: display,
//                 window: window,
//                 amstream_vbuf: amstream_vbuf,
//             })
//         }

//         #[cfg(not(target_arch = "aarch64"))]
//         {
//             Ok(Player {
//                 xlib: xlib,
//                 screen: screen,
//                 display: display,
//                 window: window,
//             })
//         }
//     }

//     pub fn run(self) -> mpsc::Sender<C2Message> {
//         let (sender, receiver) = mpsc::channel::<C2Message>();
//         let x11_sender = sender.clone();
//         self.x11_init_event_loop_thread(x11_sender);

//         let player = self;
//         println!("About to spawn main thread");
//         thread::spawn(move || {
//             use std::io::{self, Write};
//             println!("Spawned Main thread");
//             let stdout = io::stdout();
//             'mainloop: for message in receiver.iter() {
//                 let mut handle = stdout.lock();
//                 writeln!(&mut handle, "Main thread: received message {:?}", message);
//                 match message {
//                     C2Message::Shutdown => {break 'mainloop},
//                     _ => {}
//                 };
//             };
//             let mut handle = stdout.lock();
//             writeln!(&mut handle, "Main thread: shutting down");
//             println!("Main thread: shutting down");
//             unsafe {
//                 (player.xlib.XDestroyWindow)(player.display.as_ptr(), player.window);
//             }
//         });
//         sender
//     }

//     pub fn x11_init_event_loop_thread(&self, sender: mpsc::Sender<C2Message>) {
//         let display = self.display.clone();
//         let xlib = self.xlib.clone();
//         let window = self.window.clone();
//         thread::spawn(move || {
//             x11helper::event_loop(xlib, display, window, sender);
//         });
//     }
// }

// // impl ::std::ops::Drop for Player {
// //     fn drop(&mut self) {
// //         unsafe {
// //             (self.xlib.XDestroyWindow)(self.display.as_ptr(), self.window);

// //             // Shut down.
// //             // (self.xlib.XCloseDisplay)(self.display.as_ptr());
// //         }
// //     }
// // }
