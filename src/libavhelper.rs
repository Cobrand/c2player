use error::*;
use std::sync::Arc;
use std::sync::mpsc::{TryRecvError, Sender, Receiver};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use std::ptr;
use std::ffi::CString;
use std::mem;
use std::os::raw::c_int;
use super::utils::SingleUseSender as SuSender;
use libavformat as libav;

macro_rules! handle_channel_error {
    ( $x: expr, $tx: expr) => {
        if let Err(e) = $x {
            println!("libavthread: channel disconnected: ({})", e);
            $tx.send(FfiErrorCode::Disconnected);
            break;
        }
    };
    ( $x: expr) => {
        if let Err(e) = $x {
            println!("libavthread: channel disconnected: ({})", e);
            break;
        }
    };
}

const EOF : i32 = -1 * (((b'E' as u32) | (('O' as u32) << 8) | (('F' as u32) << 16) | ((' ' as u32) << 24)) as i32);

struct Context {
    pub ctx: *mut libav::AVFormatContext,
    pub hevc_stream: usize,
}

pub fn avformat_version() -> (u16, u16) {
    unsafe {
        let version = libav::avformat_version();
        let major : u16 = ((version >> 16) & 0xFFFFFFFF) as u16;
        let minor_micro : u16 = (version & 0xFFFFFFFF) as u16;
        (major, minor_micro)
    }
}

impl Context {
    pub fn new<S: AsRef<str>>(url: S) -> Result<Context> {
        let mut ctx : *mut libav::AVFormatContext = ptr::null_mut();
        // the &str -> CString automatically adds a null trailing character, so if that doesn't
        // happen the whole language is in trouble ...
        let url = CString::new(url.as_ref())
            .expect("FATAL: expected null-trailing byte, but none found!\
                    File an issue to the Rust core team on github!");
        let ret = unsafe {
            libav::avformat_open_input(&mut ctx as *mut *mut libav::AVFormatContext, url.as_ptr(), ptr::null_mut(), ptr::null_mut())
        };
        if ret < 0 {
            // TODO create another error "FileNotFound" and check
            // if libav's return value is file not found
            
            // bail returns an error: abort if open_input failed
            bail!(ErrorKind::LibavInternal(ret, "avformat_open_input"));
        }
        if let Some(hevc_stream) = Self::retrieve_hevc_stream(ctx) {
            Ok(Context {
                ctx: ctx,
                hevc_stream: hevc_stream,
            })
        } else {
            bail!(ErrorKind::NoValidVideoStream)
        }
    }

    pub fn seek(&mut self, pos: f64) -> Result<()> {
        let r = unsafe {
            libav::av_seek_frame(self.ctx, -1, (pos * (libav::AV_TIME_BASE as f64)) as i64, libav::AVFMT_SEEK_TO_PTS as c_int)
        };
        if r < 0 {
            bail!(ErrorKind::LibavInternal(r, "av_seek_frame"))
        }
        Ok(())
    }

    pub fn get_extra_data(&self) -> Result<Arc<Vec<u8>>> {
        unsafe {
            let stream : *mut _ = *(*self.ctx).streams.offset(self.hevc_stream as isize);
            let codec : *mut _ = (*stream).codec;
            let mut extra_data = Vec::with_capacity((*codec).extradata_size as usize);
            let data : &[u8] = ::std::slice::from_raw_parts((*codec).extradata, (*codec).extradata_size as usize);
            let mut offset = 21;
            let _length_size = (data[offset] & 3) + 1;
            offset += 1;
            let num_arrays = data[offset];
            offset += 1;
            for _ in 0..num_arrays {
                let _type = data[offset] & 0x3f;
                offset += 1;
                let mut cnt : u32 = (data[offset] as u32) << 8;
                offset += 1;
                cnt |= data[offset] as u32;
                offset += 1;
                for _ in 0..cnt {
                    extra_data.push(0);
                    extra_data.push(0);
                    extra_data.push(0);
                    extra_data.push(1);
                    let mut nalu_len = (data[offset] as u32) << 8;
                    offset += 1;
                    nalu_len |= data[offset] as u32;
                    offset += 1;
                    for _ in 0..nalu_len {
                        extra_data.push(data[offset]);
                        offset += 1;
                    }
                }
            }
            println!("amcodec: extra_data size: {}", (*codec).extradata_size);
            Ok(Arc::new(extra_data))
        }
    }

    fn retrieve_hevc_stream(ctx: *mut libav::AVFormatContext) -> Option<usize> {
        unsafe {
            let ret = libav::avformat_find_stream_info(ctx, ptr::null_mut());
            if ret < 0 {
                println!("avformat_find_stream_info returned {}", ret);
                return None
            } else {
                'hevc_search: for i in 0..((*ctx).nb_streams as usize) {
                    let stream : *const libav::AVStream = *(*ctx).streams.offset(i as isize);
                    let codec : *const _ = (*stream).codec;
                    let codec_id = (*codec).codec_id;
                    let codec_type = (*codec).codec_type;
                    match (codec_type, codec_id) {
                        (libav::AVMediaType::AVMEDIA_TYPE_VIDEO, libav::AVCodecID::AV_CODEC_ID_HEVC) => {
                            println!("libav_thread: Stream {} is HEVC ! ({:?}, {:?})", i, libav::AVMediaType::AVMEDIA_TYPE_VIDEO, libav::AVCodecID::AV_CODEC_ID_HEVC);
                            return Some(i);
                        },
                        _ => {
                            println!("libav_thread: Ignoring media_type {:?} and codec {:?}: not HEVC", codec_type, codec_id);
                        }
                    };
                }
            }
        };
        None
    }
    
    pub fn next_frame(&mut self) -> Result<Packet> {
        unsafe {
            let mut packet : libav::AVPacket = mem::uninitialized();
            let ret = libav::av_read_frame(self.ctx as *mut _, &mut packet as *mut _);
            match ret {
                EOF => bail!(ErrorKind::EOF),
                _ if ret >= 0 => {
                    Ok(Packet {
                        inner: packet
                    })
                },
                ret => {
                    bail!("libav: error when reading frame, returned {0:x} ({0})", ret);
                }
            }
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            libav::avformat_close_input(&mut self.ctx as *mut *mut _);
            debug_assert_eq!(self.ctx, ptr::null_mut());
        }
    }
}

#[derive(Debug)]
pub enum Message {
    Load(String),
    Seek(f64),
}

#[derive(Debug)]
pub struct Packet {
    pub inner: libav::AVPacket,
}

#[derive(Debug)]
pub enum PacketWrapper {
    /// Needed before every new file
    ExtraData(Arc<Vec<u8>>),
    /// A standard packet usually describing one frame
    Packet(Packet),
    /// A message describing that the file's done playing,
    /// after this point it should wait for other ExtraData
    EOF,
    /// Send an error to amcodec thread
    Error(Error),
    /// Stop the current playback (to load something else instead for
    /// example)
    Stop,
}

impl Drop for Packet {
    fn drop(&mut self) {
        unsafe {
            libav::av_packet_unref(&mut self.inner as *mut _);
        }
    }
}

unsafe impl Send for Packet {}

pub fn main_thread(rx: Receiver<(Message, SuSender<FfiErrorCode>)>, packet_channel: Sender<PacketWrapper>, keep_running: Arc<AtomicBool>) {
    println!("libavthread starting");
    let mut allow_next_frame = true;
    // unsafe tag is required for C functions calls ... since we are almost doing only that,
    // there is no point to write "unsafe" every other line of code, just write it once
    unsafe {
        // Initialize all the muxers, demuxers and protocols
        libav::av_register_all();
        // Initialize network
        libav::avformat_network_init();
        let mut context : Option<Context> = None;
        while keep_running.load(Ordering::SeqCst) == true {
            match rx.try_recv() {
                Ok((Message::Load(m), tx)) => {
                    handle_channel_error!(packet_channel.send(PacketWrapper::Stop), tx);
                    allow_next_frame = true;
                    context = match Context::new(m.as_str()) {
                        Ok(context) => {
                            match context.get_extra_data() {
                                Ok(extra_data) => {
                                    handle_channel_error!(packet_channel.send(PacketWrapper::ExtraData(extra_data)), tx);
                                },
                                Err(e) => {
                                    println!("libav_thread: warning: get_extra_data failed: {}", e.display());
                                }
                            };
                            tx.send(FfiErrorCode::None);
                            Some(context)
                        },
                        Err(e) => {
                            println!("libav_thread: error when loading url/path `{}`: {}", m.as_str(), e.display());
                            println!("libav_thread: url will be ignored");
                            tx.send(error_to_ecode(e));
                            None
                        }
                    };
                },
                Ok((Message::Seek(pos), tx)) => {
                    if let Some(ref mut context) = context {
                        handle_channel_error!(packet_channel.send(PacketWrapper::Stop), tx);
                        match context.get_extra_data() {
                            Ok(extra_data) => {
                                handle_channel_error!(packet_channel.send(PacketWrapper::ExtraData(extra_data)), tx);
                            },
                            Err(e) => {
                                println!("libav_thread: warning: get_extra_data failed: {}", e.display());
                            }
                        };
                        tx.send(result_to_ecode(context.seek(pos)));
                    } else {
                        // there is no point "Seeking" something when nothing is loaded in the
                        // first place ...
                        tx.send(FfiErrorCode::InvalidCommand);
                    }
                },
                Err(TryRecvError::Disconnected) => {
                    // the other end of the channel has hung up
                    // it can only mean 2 things:
                    // * the other thread has panicked unexpectedly
                    // * this is a data-race: the channel hung up before
                    // we received the fact that keep_running became false
                    //
                    // in both cases breaking the loop is the correct thing to do here
                    println!("libav_thread: uh oh ...");
                    break;
                },
                // no message
                _ => {}
            };
            if allow_next_frame {
                if let Some(ref mut context) = context {
                    match context.next_frame() {
                        Ok(packet) => {
                            if packet.inner.stream_index as usize == context.hevc_stream {
                                handle_channel_error!(packet_channel.send(PacketWrapper::Packet(packet)));
                            }
                        },
                        Err(Error(ErrorKind::EOF,_)) => {
                            handle_channel_error!(packet_channel.send(PacketWrapper::EOF));
                            allow_next_frame = false;
                        },
                        Err(e) => {
                            handle_channel_error!(packet_channel.send(PacketWrapper::Error(e)));
                            allow_next_frame = false;
                        }
                    };
                };
            };
            thread::sleep(Duration::from_millis(5));
        }
    }
    if cfg!(debug_assertions) {
        println!("libav_thread: shutting down ...");
    }
}
