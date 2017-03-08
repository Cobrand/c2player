/**
 * HOW THIS LIB WORKS (SUMMARY) :
 *
 * the amcodec device (/dev/amstream_hevc) needs to be fed raw hevc data to work,
 * and this data can be found an AVPacket's data for the libavformat lib.
 *
 * One thread tries to retrieve AVPacket s while the other writes the raw data in the amcodec
 * device. This part allows us to decode the HEVC stream with the VPU, but we need to "display" the
 * stream on screen. The display part is actually kind of a hack: a borderless transparent window
 * is created (transparent on the framebuffer level and not on the window level) and placed at the
 * position where we wanted the video to be. The VPU's layer will be shown since the X11 window is
 * transparent, allowing us to properly see the video's playback.
 *
 * For now this is kind of a hack because a standalone window is created for this, meaning if can
 * be manipulated in your window manager for instance. The ideal way would have been to accept a
 * X11 window id as a paramter of this library, and create the X11 transparent window as a
 * subwindow. Tests haven't been made, but the problem of the standalone window should disappear if
 * this is implement (unfortunately this isn't for now)
 *
 */

use error::*;
use super::x11helper::X11Helper;
use super::libavhelper::{main_thread as libav_main_thread, Message as LibavMessage, PacketWrapper as LibavPacket};
use super::amcodec::{self, main_loop as amcodec_main_loop, Message as AmcodecMessage, EndReason as VideoEndReason};
use super::utils::SingleUseSender as SuSender;

use std::sync::{Arc, atomic};
use std::{ptr, thread};
use std::sync::mpsc::{self, Receiver, Sender};
use libc::c_int;
use std::thread::JoinHandle;
use libavformat;
use super::libavhelper::avformat_version;

/// This is the struct that will get "forgotten" and sent back to the API every time the user needs
/// do send a command. For all these calls the most important thing here is "sender", but the
/// others are needed for "destroy" as well: we need to wait for all the threads to finish for us
/// to finish, so we need to join every thread in "destroy".
pub struct FfiPlayer {
    pub main_thread: JoinHandle<()>,
    pub x11_event_loop_thread: JoinHandle<()>,
    pub amcodec_thread: JoinHandle<()>,
    pub libav_getter_thread: JoinHandle<()>,
    pub video_status_queue: Receiver<VideoEndReason>,
    pub sender: Sender<Message>,
    pub keep_running: Arc<atomic::AtomicBool>,
}

impl FfiPlayer {
    /// Join all 4 threads and return an error if one didn't return successfully
    pub fn join(self) -> FfiResult {
        let mut error_code = Ok(());
        if let Err(_) = self.main_thread.join() {
            error_code = Err(FfiErrorCode::ShutdownError);
            println!("Main Thread panicked");
        };
        if let Err(_) = self.x11_event_loop_thread.join() {
            error_code = Err(FfiErrorCode::ShutdownError);
            println!("X11 Event Thread panicked");
        };
        if let Err(_) = self.amcodec_thread.join() {
            error_code = Err(FfiErrorCode::ShutdownError);
            println!("Amcodec Thread panicked");
        };
        if let Err(_) = self.libav_getter_thread.join() {
            error_code = Err(FfiErrorCode::ShutdownError);
            println!("Libav Thread panicked");
        };
        error_code
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
            Ok(VideoEndReason::Error(s)) => {
                println!("A fatal error happened when decoding a video packet: {}", s);
                1
            },
            Ok(VideoEndReason::EOF) => 0,
            Err(e) => {
                println!("Video status channel disconnected : {}", e);
                -1
            }
        }
    }
}

/// all the messages possible which can be sent to the main_thread
/// notice that every single one of them has an equivalent in the API
pub enum Message {
    SetSize(SuSender<FfiErrorCode>, (u16, u16)),
    SetPos(SuSender<FfiErrorCode>,(i16, i16)),
    SetFullscreen(SuSender<FfiErrorCode>, bool),
    Show(SuSender<FfiErrorCode>),
    Hide(SuSender<FfiErrorCode>),
    Play(SuSender<FfiErrorCode>),
    Pause(SuSender<FfiErrorCode>),
    Load(SuSender<FfiErrorCode>, String),
    Seek(SuSender<FfiErrorCode>, f64),
    Shutdown
}

// when this is called, we are still in the thread of the user of the API
// we will need to "detach" our core logic
//
// this function returns almost instantly a FfiPlayer (or an error), and spawns
// multiple threads that have one very specific purpose
//
// * libav_thread: receive messages from main thread (such as Load("path")) and send appropriate
// video hevc packets to the amcodec_thread
// * amcodec_thread: receive messages from libav_thread and main_thread and process them (write
// libavpacket in VPU, resize the VPU's output area, ...)
// * x11_thread : handle the event loop
// * main_thread: receive messages from the API and send messages to other threads accordingly
pub fn player_start() -> Result<FfiPlayer> {
    let (version_major, version_minor) = avformat_version();
    // we are only checking the major version here, because breaking changes
    // only happen between major versions, hence even though the minor version changes,
    // we are still "safe" from unexpected behavior
    if version_major != libavformat::LIBAVCODEC_VERSION_MAJOR as u16 {
        println!("Linked avformat version ({}) differs from the one the header was built with ({}). \
                This can lead to unexpected behavior and segfaults at times. \
                Aborting", version_major, libavformat::LIBAVCODEC_VERSION_MAJOR);
        bail!(ErrorKind::WrongLibavVersion);
    } else {
        println!("using libavformat version {}.{}", version_major, version_minor);
    };

    // note that x11_thread doesn't receive messages like other threads: this is because the X11
    // API is thread safe, and thus we can call multiple functions of the same window at once.
    // channels allow us to have the guarentee that 1 message is processed at a time, but we don't
    // really care in x11's case.
    let x11_helper = Arc::new(X11Helper::new(ptr::null_mut())?);
    if let Err(e) = x11_helper.set_borderless(true) {
        println!("failed to set x11 window borderless: {}", e.display());
    };

    // channel from the API to the main_thread
    let (sender, receiver) = mpsc::channel::<Message>();
    // channel from amcodec_thread to the API thread: send when an EOF is reached on the playback
    // side
    let (video_status_sender, video_status_rx) = mpsc::channel::<VideoEndReason>();

    // shared boolean between every thread: when this becomes false every thread will stop as soon
    // as possible
    let keep_running = Arc::new(atomic::AtomicBool::new(true));
    
    let x11_thread = {
        // thread needs to "move" the caught variables in its closure, hence we need to clone these
        // so the clones can get moved, otherwise we get a compile error saying we already used
        // x11_helper (moved in this thread)
        let x11_helper = x11_helper.clone();
        let keep_running = keep_running.clone();
        thread::spawn(move || {
            x11_helper.event_loop(keep_running);
        })
    };

    // channel between libav_thread and amcodec_thread, which is meant for libav to send packets to
    // amcodec
    let (packet_sender, packet_receiver) = mpsc::channel::<LibavPacket>();
   
    // channel beetween main_thread and libav_thread, where messages such as Load("url") are sent
    let (libav_sender, libav_receiver) = mpsc::channel::<(LibavMessage, SuSender<FfiErrorCode>)>();

    // channel between main_thread and amcodec_thread, where messages such as "SetSize(x,y,w,h)"
    // are sent to amcodec_thread
    let (amcodec_sender, amcodec_receiver) = mpsc::channel::<(AmcodecMessage, SuSender<FfiErrorCode>)>();

    let libav_thread = {
        let keep_running = keep_running.clone();
        thread::spawn(move || {
            libav_main_thread(libav_receiver, packet_sender, keep_running);
        })
    };

    let amcodec_thread = {
        let keep_running = keep_running.clone();
        // _fb_wrapper is not used but is the thing that allow us to have a transparent framebuffer
        // as long as it lives we can set some alpha of the framebuffer to 0
        let _fb_wrapper = amcodec::FbWrapper::new()?;
        // we are doing this initialization here instead of in the thread because we can then
        // return an error directly if something went wrong (if this went wrong there is no point
        // in doing anything else)
        let amcodec = amcodec::Amcodec::new(video_status_sender.clone())?;
        let version = amcodec.version()?;
        println!("amcodec_thread: AMSTREAM version {}.{}", version.0, version.1);
        thread::spawn(move || {
            // move fb_wrapper inside the thread so that it is only destroyed after the thread is
            // complete
            let _fb_wrapper = _fb_wrapper;
            amcodec_main_loop(amcodec, amcodec_receiver, packet_receiver, video_status_sender, keep_running);
        })
    };

    let main_thread = {
        // keep track of the current window's dimensions
        let (mut window_x, mut window_y, mut window_w, mut window_h) = (0i16, 0i16, 1920u16, 1080u16);
        let keep_running = keep_running.clone();
        thread::spawn(move || {
            let libav_channel = libav_sender;
            let amcodec_channel = amcodec_sender;
            'mainloop: for message in receiver.iter() {
                match message {
                    Message::Shutdown => {
                        break 'mainloop;
                    },
                    Message::SetFullscreen(tx, b) => {
                        if b == true {
                            if let Err(_) = amcodec_channel.send((AmcodecMessage::Fullscreen, tx.clone())) {
                                println!("main_thread: amcodec_channel disconnected, aborting");
                                tx.send(FfiErrorCode::Disconnected);
                                break 'mainloop;
                            }
                        } else {
                            if let Err(_) = amcodec_channel.send((AmcodecMessage::Resize(window_x, window_y, window_w, window_h), tx.clone())) {
                                println!("main_thread: amcodec_channel disconnected, aborting");
                                tx.send(FfiErrorCode::Disconnected);
                                break 'mainloop;
                            }
                        }
                        if let Err(e) = x11_helper.set_fullscreen(b) {
                            println!("main_thread: failed to set x11 window fullscreen: {}", e.display());
                        };
                    },
                    Message::Show(tx) => {
                        x11_helper.show();
                        tx.send(FfiErrorCode::None);
                    },
                    Message::Hide(tx) => {
                        x11_helper.hide();
                        tx.send(FfiErrorCode::None);
                    },
                    Message::SetPos(tx,(x, y)) => {
                        // when setting a position we must set the position of the X11 window as
                        // well as the position of the VPU's output video
                        window_x = x;
                        window_y = y;
                        if let Err(_) = amcodec_channel.send((AmcodecMessage::Resize(window_x, window_y, window_w, window_h), tx.clone())) {
                            println!("main_thread: amcodec_channel disconnected, aborting");
                            tx.send(FfiErrorCode::Disconnected);
                            break 'mainloop;
                        }
                        x11_helper.set_pos(x, y);
                    },
                    Message::SetSize(tx,(w, h)) => {
                        window_w = w;
                        window_h = h;
                        if let Err(_) = amcodec_channel.send((AmcodecMessage::Resize(window_x, window_y, window_w, window_h), tx.clone())) {
                            println!("main_thread: amcodec_channel disconnected, aborting");
                            tx.send(FfiErrorCode::Disconnected);
                            break 'mainloop;
                        }
                        x11_helper.set_size(w, h);
                        tx.send(FfiErrorCode::None);
                    },
                    Message::Load(tx,url) => {
                        if let Err(_) = libav_channel.send((LibavMessage::Load(url), tx.clone())) {
                            tx.send(FfiErrorCode::LibAvDisconnected);
                        };
                    },
                    Message::Seek(tx, pos) => {
                        if let Err(_) = libav_channel.send((LibavMessage::Seek(pos), tx.clone())) {
                            tx.send(FfiErrorCode::LibAvDisconnected);
                        };
                    },
                    Message::Play(tx) => {
                        if let Err(_) = amcodec_channel.send((AmcodecMessage::Play, tx.clone())) {
                            println!("main_thread: amcodec_channel disconnected, aborting");
                            tx.send(FfiErrorCode::Disconnected);
                            break 'mainloop;
                        };
                    },
                    Message::Pause(tx) => {
                        if let Err(_) = amcodec_channel.send((AmcodecMessage::Pause, tx.clone())) {
                            println!("main_thread: amcodec_channel disconnected, aborting");
                            tx.send(FfiErrorCode::Disconnected);
                            break 'mainloop;
                        };
                    }
                };
            };
            keep_running.store(false, atomic::Ordering::SeqCst);
            if cfg!(debug_assertions) {
                println!("Finishing main loop ...");
            }
        })
    };

    // once every thread is spawned, return FfiPlayer to the API caller
    Ok(FfiPlayer {
        main_thread: main_thread,
        x11_event_loop_thread: x11_thread,
        amcodec_thread: amcodec_thread,
        libav_getter_thread: libav_thread,
        video_status_queue: video_status_rx,
        sender: sender,
        keep_running: keep_running,
    })
}
