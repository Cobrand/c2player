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

pub fn player_start() -> Result<FfiPlayer> {
    let (version_major, version_minor) = avformat_version();
    // we are only checking the major version here, because breaking changes
    // only happen between major versions, hence even though the minor version changes,
    // we are still "safe" from unexpected behavior
    if version_major != libavformat::LIBAVCODEC_VERSION_MAJOR as u16 {
        println!("Linked avformat version differs from the one the header was built with.\
                This can lead to unexpected behavior and segfaults at times.\
                Aborting");
        bail!(ErrorKind::WrongLibavVersion);
    } else {
        println!("using libavformat version {}.{}", version_major, version_minor);
    };

    let x11_helper = Arc::new(X11Helper::new(ptr::null_mut())?);
    if let Err(e) = x11_helper.set_borderless(true) {
        println!("failed to set x11 window borderless: {}", e.display());
    };

    let (sender, receiver) = mpsc::channel::<Message>();
    let (video_status_sender, video_status_rx) = mpsc::channel::<VideoEndReason>();
    let keep_running = Arc::new(atomic::AtomicBool::new(true));
    
    let x11_thread = {
        let x11_helper = x11_helper.clone();
        let keep_running = keep_running.clone();
        thread::spawn(move || {
            x11_helper.event_loop(keep_running);
        })
    };

    let (packet_sender, packet_receiver) = mpsc::channel::<LibavPacket>();
    let (libav_sender, libav_receiver) = mpsc::channel::<(LibavMessage, SuSender<FfiErrorCode>)>();
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
                        };
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
