use error::*;
use std::sync::Arc;
use std::sync::mpsc::{TryRecvError, Sender, Receiver};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{thread, mem};
use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use libc::{c_int, c_uint};
use libavformat as libav;
use super::utils::SingleUseSender as SuSender;

//amcodec_sys contains all the C interface of amcodec and related
use super::amcodec_sys::*;

use super::libavhelper::PacketWrapper as LibavPacket;

// This state will allow us to have a pseudo-state machine
// It is not exactly a state machine, but it still has some very strict rules about the states it
// can change to
//
// If this were really a state machine, the commands would be "play", "pause", "finish" and "stop".
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// A video has not yet / is being buffered
    /// but the video hasnt played yet
    InitialState,
    /// A video is still being buffered,
    /// but the playback is paused
    Paused,
    /// A video is being buffered and
    /// the video is being played
    Playing,
    /// The video is finished being buffered (EOF received)
    /// but the VPU is still non-empty, so we need
    /// to finish the playback until the VPU is empty
    ///
    /// The very simple actual way to get if a file is finished is:
    /// * we got EOF before (which happened cause we are in this State)
    /// * we don't have enough data in the VPU to get another frame, hence we are stuck
    ///
    /// If we are stuck too many times, we can just assume that there is nothing left to play
    /// and the file is actually finished. same_data_len_count actually coutns how many times the
    /// "data_len" variable has been the same.
    Finishing {
        prev_data_len: c_int,
        same_data_len_count: u32,
    },
    /// The video is finished being buffered (EOF received)
    /// but the VPU is still non-empty, but we are currently
    /// Paused, so when playback resume we will be in "Finishing"
    /// State
    PausedFinishing,
    /// The VPU is empty and no video is being buffered at the moment
    /// This means that Amcodec will very soon (next "update") reset
    /// and be in pause state
    ///
    /// true means "Stopped because EOF reached"
    /// false means "Stopped because libav requested an explicit stop"
    Stopped(bool),
}

// All the cfg(not(target_arch = "aarch64")) are dummies so that
// it can compile for x86_64 architectures.
#[cfg(not(target_arch = "aarch64"))]
pub struct FbWrapper;

#[cfg(not(target_arch = "aarch64"))]
impl FbWrapper {
    pub fn new() -> Result<FbWrapper> {
        Ok(FbWrapper)
    }
}

#[cfg(target_arch = "aarch64")]
pub struct FbWrapper {
    screeninfo: FbVarScreeninfo,
}

#[cfg(target_arch = "aarch64")]
pub struct Amcodec {
    hevc_device: File,
    control_device: File,
    state: State,
    pub status_sender: Sender<EndReason>,
}

/// This structure holds the info of the framebuffer before it went transparent:
/// we must enable the alpha byte on the framebuffer for the video to play, but the best would be
/// to restore previous settings
#[cfg(target_arch = "aarch64")]
impl FbWrapper {
    pub fn new() -> Result<FbWrapper> {
        let fb0 = OpenOptions::new().write(true).open("/dev/fb0");
        let stored_screeninfo;
        match fb0 {
            Ok(fb0) => {
                unsafe {
                    let mut screeninfo : FbVarScreeninfo = mem::uninitialized();
                    let ret = fbio_get_vscreen_info(fb0.as_raw_fd(), &mut screeninfo as *mut _ as *mut u8);
                    if ret < 0 {
                        bail!(ErrorKind::Ioctl("fbio_get_vscreen_info"));
                    }
                    stored_screeninfo = screeninfo.clone();
                    screeninfo.red.offset = 16;
                    screeninfo.red.length = 8;
                    screeninfo.green.offset = 8;
                    screeninfo.green.length = 8;
                    screeninfo.blue.offset = 0;
                    screeninfo.blue.length = 8;
                    screeninfo.transp.offset = 24;
                    screeninfo.transp.length = 8;
                    screeninfo.nonstd = 1;
                    screeninfo.activate = 0; // see FB_ACTIVE_NOW
                    let ret = fbio_set_vscreen_info(fb0.as_raw_fd(),&mut screeninfo as *mut _ as *mut u8);
                    if ret < 0 {
                        bail!(ErrorKind::Ioctl("fbio_set_vscreen_info"));
                    }
                }
            },
            Err(io_error) => {
                return Err(io_error).chain_err(|| ErrorKind::FbPermission);
            }
        }
        Ok(FbWrapper {
            screeninfo: stored_screeninfo,
        })
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub struct Amcodec {
    state: State,
    count: u32,
    sender: Sender<EndReason>,
}

/// A dummy for x86_64 and other architectures. Doesn't play a video, but "simulates" one for tests
/// and other stuff.
#[cfg(not(target_arch = "aarch64"))]
impl Amcodec {
    pub fn new(status_sender: Sender<EndReason>) -> Result<Amcodec> {
        Ok(Amcodec {
            sender: status_sender,
            state: State::InitialState,
            count: 1000,
        })
    }

    pub fn version(&self) -> Result<(u16, u16)> {
        Ok((0, 0))
    }

    pub fn update(&mut self) {
        if self.state == State::Playing {
            if self.count == 0 {
                let _r = self.sender.send(EndReason::EOF);
                self.state = State::InitialState;
                self.count = 1000;
            } else {
                self.count -= 1;
            }
        }
    }

    pub fn play(&mut self) {
        self.state = State::Playing;
    }

    pub fn pause(&mut self) {
        self.state = State::Paused;
    }
}

/// dummy version of the main loop
#[cfg(not(target_arch = "aarch64"))]
pub fn main_loop(mut amcodec: Amcodec,
                   rx: Receiver<(Message, SuSender<FfiErrorCode>)>,
                   packet_channel: Receiver<LibavPacket>,
                   status_sender: Sender<EndReason>,
                   keep_running: Arc<AtomicBool>) {
    while keep_running.load(Ordering::SeqCst) == true {
        match rx.try_recv() {
            Ok((Message::Fullscreen, tx)) => {
                tx.send(FfiErrorCode::None);
            }
            Ok((Message::Resize(x, y, width, height), tx)) => {
                tx.send(FfiErrorCode::None);
            },
            Ok((Message::Play, tx)) => {
                amcodec.play();
                tx.send(FfiErrorCode::None);
            },
            Ok((Message::Pause, tx)) => {
                amcodec.pause();
                tx.send(FfiErrorCode::None);
            },
            Err(TryRecvError::Disconnected) => {
                break;
            },
            Err(_) => {}
        };
        amcodec.update();
        thread::sleep(Duration::from_millis(15));
    }
    println!("amcodec_thread: shutting down ...");
}

/// the main loop for the amcodec thread
///
/// * amcodec: Amcodec is created before this thread is spawned because it allows easier
/// error-reporting (such as the driver does not exist)
/// * rx: various messages such as Play, Pause, Resize, ... are sent to this channel
/// this channel also includes a way to answers those requests via a SingleUsageChannel
/// * status_sender: allows us to notify the API's user when an EOF has happened
/// * keep_running: if this becomes false then this thread must abort as soon as possible
#[cfg(target_arch = "aarch64")]
impl Amcodec {
    /// sometimes opening the file won't work right away,
    /// especially when you just closed it
    /// if that happens it will send an EBUSY (16) error.
    /// If we get this error, wait a little bit and try once more.
    /// After a number of tries, we can assume the device is dead and give up
    fn try_open<P: AsRef<Path>>(open_options: &OpenOptions, path: P, tries: u32) -> Result<File> {
        if tries == 0 {
            bail!("{} is busy (os error 16), stopping after multiple tries", path.as_ref().display());
        };
        match open_options.open(path.as_ref()) {
            Err(ref e) if e.raw_os_error() == Some(16) => {
                thread::sleep(Duration::from_millis(50));
                Self::try_open(open_options, path.as_ref(), tries - 1)
            },
            o => o.chain_err(|| format!("failed to open {}", path.as_ref().display()))
        }
    }

    /// This Amcodec creationis kind of cheating: we already know in advance that we only support
    /// HEVC, hence we can make it so HEVC is always enabled. 
    pub fn new(status_sender: Sender<EndReason>) -> Result<Amcodec> {
        let hevc_device = Self::try_open(OpenOptions::new().write(true).read(false), "/dev/amstream_hevc", 100)
            .chain_err(|| ErrorKind::Amcodec)?;
        let control_device = Self::try_open(OpenOptions::new().write(true).read(true), "/dev/amvideo", 100)
            .chain_err(|| ErrorKind::Amcodec)?;
        unsafe {
            let mut aml_ioctl_parm : am_ioctl_parm = mem::zeroed();
            let mut am_sysinfo : dec_sysinfo_t = mem::zeroed();
            aml_ioctl_parm.union.data_vformat = vformat_t::VFORMAT_HEVC;
            aml_ioctl_parm.cmd = AMSTREAM_SET_VFORMAT;
            am_sysinfo.format = vdec_type_t::VIDEO_DEC_FORMAT_HEVC as c_uint;
            let r = amstream_ioc_set(hevc_device.as_raw_fd(), &aml_ioctl_parm as *const _);
            if r < 0 {
                bail!(ErrorKind::Ioctl("amstream_ioc_set"));
            }
            // see amstream_ioc_sysinfo declaration in amcodec_sys for why we need to cast to a c_int
            let r = amstream_ioc_sysinfo(hevc_device.as_raw_fd(), &am_sysinfo as *const _ as *const c_int);
            if r < 0 {
                bail!(ErrorKind::Ioctl("amstream_ioc_sysinfo"));
            }
        }
        let amcodec = Amcodec {
            hevc_device: hevc_device,
            control_device: control_device,
            state: State::InitialState,
            status_sender: status_sender,
        };
        Ok(amcodec)
    }

    pub fn set_fullscreen(&mut self) -> Result<()> {
        let fb0 = OpenOptions::new().read(true).open("/dev/fb0");
        match fb0 {
            Ok(fb0) => {
                unsafe {
                    let mut screeninfo : FbVarScreeninfo = mem::uninitialized();
                    let ret = fbio_get_vscreen_info(fb0.as_raw_fd(), &mut screeninfo as *mut _ as *mut u8);
                    if ret < 0 {
                        bail!(ErrorKind::Ioctl("get_vscreeninfo"));
                    }
                    self.set_video_axis((0, 0, screeninfo.width as u16, screeninfo.height as u16))
                }
            },
            e => e.map(|_| ()).chain_err(|| ErrorKind::FbPermission)
        }
    }

    /// (x, y, width, height)
    pub fn set_video_axis(&mut self, (x, y, width, height): (i16, i16, u16, u16)) -> Result<()> {
        let mut values : [c_int; 4] = [0; 4];
        values[0] = x as c_int;
        values[1] = y as c_int;
        values[2] = x as c_int + width as c_int;
        values[3] = y as c_int + height as c_int;
        let r = unsafe {
            amstream_ioc_set_video_axis(self.control_device.as_raw_fd(), &values as *const c_int)
        };
        if r < 0 {
            bail!(ErrorKind::Ioctl("amstream_ioc_set_video_axis"));
        }
        Ok(())
    }

    pub fn play(&mut self) -> Result<()> {
        let new_state = match self.state {
            State::PausedFinishing => State::Finishing {
                prev_data_len: 0,
                same_data_len_count: 0,
            },
            _ => State::Playing,
        };
        self.set_state(new_state)
    }

    pub fn pause(&mut self) -> Result<()> {
        let new_state = match self.state {
            State::Finishing { .. } => State::PausedFinishing,
            _ => State::Paused,
        };
        self.set_state(new_state)
    }

    /// false : play
    /// true : pause
    fn vpause(&mut self, value: bool) -> Result<()> {
        let value : *const c_int = match value {
            false => 0usize,
            true => 1usize,
        } as *const c_int;
        let r = unsafe {
            amstream_ioc_vpause(self.control_device.as_raw_fd(), value)
        };
        if r < 0 {
            bail!(ErrorKind::Ioctl("ioc_vpause"));
        }
        Ok(())
    }

    // mainly for debug purposes
    #[allow(unused)]
    pub fn get_vb_status(&self) -> Result<String> {
        let mut vb_status : am_ioctl_parm_ex = unsafe { mem::zeroed()};
        vb_status.cmd = AMSTREAM_GET_EX_VDECSTAT;
        let r = unsafe {
            amstream_ioc_get_vb_status(self.hevc_device.as_raw_fd(), &mut vb_status)
        };
        if r < 0 {
            bail!(ErrorKind::Ioctl("amstream_ioc_get_vb_status"));
        };
        Ok(format!("{:#?}", unsafe {vb_status.union.vstatus} ))
    }

    pub fn get_buf_status(&self) -> Result<BufStatus> {
        let mut vb_status : am_ioctl_parm_ex = unsafe { mem::zeroed()};
        vb_status.cmd = AMSTREAM_GET_EX_VB_STATUS;
        let r = unsafe {
            amstream_ioc_get_vb_status(self.hevc_device.as_raw_fd(), &mut vb_status)
        };
        if r < 0 {
            bail!(ErrorKind::Ioctl("amstream_ioc_get_vb_status"));
        };
        Ok(unsafe {vb_status.union.status})
    }

    fn set_state(&mut self, state: State) -> Result<()> {
        if self.state == state {
            return Ok(())
        };
        match state {
            State::Stopped(b) => {
                self.clear_video()?;
                if b {
                    // this will unblock "wait_until_end" calls from the API
                    self.status_sender.send(EndReason::EOF)
                        .chain_err(|| ErrorKind::Disconnected)?;
                } 
            },
            State::Paused => {
                self.vpause(true)?;
            },
            State::Playing => {
                self.vpause(false)?;
            },
            State::PausedFinishing => {
                self.vpause(true)?;
            },
            _ => {}
        };
        self.state = state;
        Ok(())
    }

    // we talked about a pseudo state machine up there, this is the method that allows it
    // to update itself
    pub fn update_state(&mut self) -> Result<bool> {
        let new_state : State = match &self.state {
            &State::Finishing {
                prev_data_len,
                same_data_len_count
            } => {
                let buf_status = self.get_buf_status()?;
                if buf_status.data_len <= 0 ||
                    (prev_data_len == buf_status.data_len && same_data_len_count >= 3) {
                    State::Stopped(true)
                } else {
                    if prev_data_len == buf_status.data_len {
                        State::Finishing {
                            same_data_len_count: same_data_len_count + 1,
                            prev_data_len: buf_status.data_len,
                        }
                    } else {
                        State::Finishing {
                            same_data_len_count: 0,
                            prev_data_len: buf_status.data_len,
                        }
                    }
                }
            },
            s => *s,
        };
        self.set_state(new_state)?;
        if let State::Stopped(_) = self.state {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // write some bytes in the hevc_device driver file
    //
    // this can sometimes fail with an "unavailable" error, sometimes within the middle of a
    // playback even, but this doesn't stop us from playing the video at all
    fn write_codec(&mut self, data: &[u8]) -> Result<()> {
        use std::io::Write;
        // calls `write` until the whole buffer has been written in the file
        self.hevc_device.write_all(data).chain_err(|| ErrorKind::Amcodec)?;
        // ensures that all data writen has been sent to the true sink
        self.hevc_device.flush().chain_err(|| ErrorKind::Amcodec)?;
        Ok(())
    }

    // writing extra_data is actually writing data to the codec ... the only thing is that it must
    // be done before any other data
    #[inline]
    fn write_extra_data(&mut self, extra_data: &[u8]) -> Result<()> {
        self.write_codec(extra_data)
    }

    // clears the buffer output (on the screen), but it doesn't look like it clears the VPU's inner
    // memory
    fn clear_video(&mut self) -> Result<()> {
        let v : c_int = 1;
        let r = unsafe {
            amstream_ioc_clear_video(self.control_device.as_raw_fd(), &v as *const _)
        };
        if r < 0 {
            bail!(ErrorKind::Ioctl("amstream_clear_video"));
        }
        Ok(())
    }

    // unused when operating on video only
    // this was implemented when trying to get the driver working, but is unused now
    #[allow(unused)]
    fn set_tstamp(&mut self, pts: u32) -> Result<()> {
        let mut parm : am_ioctl_parm = unsafe { mem::zeroed() };
        parm.cmd = AMSTREAM_SET_TSTAMP;
        unsafe {
            parm.union.data_32 = pts;
        }
        let r = unsafe {
            amstream_ioc_set(self.hevc_device.as_raw_fd(), &parm)
        };
        if r < 0 {
            bail!(ErrorKind::Ioctl("set_tstamp"));
        };
        Ok(())
    }

    // this s ia key step for the video processing of the VPU, if we don't do this step the VPU
    // only outputs pitch black
    fn process_nal_packets(data: &mut [u8]) -> Result<()> {
        let mut offset : usize = 0;
        while offset < data.len() {
            let (_, mut data) = data.split_at_mut(0);
            let nal_len : u32 = ((data[0] as u32) << 24) | ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
            data[0] = 0;
            data[1] = 0;
            data[2] = 0;
            data[3] = 1;
            offset += nal_len as usize + 4;
        }
        Ok(())
    }

    fn process_libavpacket<'p>(&mut self, pkt: &'p libav::AVPacket) -> Result<()> {
        let mut data : &'p mut [u8] = unsafe {
            ::std::slice::from_raw_parts_mut(pkt.data, pkt.size as usize)
        };
        Self::process_nal_packets(&mut data)?;
        self.write_codec(data)?;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        let new_state = match self.state {
            State::Paused | State::PausedFinishing => State::PausedFinishing,
            State::InitialState | State::Playing | State::Finishing {..} => State::Finishing {
                    prev_data_len: 0,
                    same_data_len_count: 0,
                },
            State::Stopped(b) => State::Stopped(b),
        };
        self.set_state(new_state)
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.state != State::InitialState {
            self.set_state(State::Stopped(false))?;
        };
        Ok(())
    }

    pub fn process_packet(&mut self, data: LibavPacket) -> Result<()> {
        match data {
            LibavPacket::ExtraData(extra_data) => self.write_extra_data(&*extra_data),
            LibavPacket::Packet(p) => self.process_libavpacket(&p.inner),
            LibavPacket::EOF => self.finish(),
            LibavPacket::Stop => self.stop(),
            LibavPacket::Error(e) => Err(e),
        }
    }

    pub fn version(&self) -> Result<(u16, u16)> {
        let mut amstream_version : c_int = 0;
        let ret = unsafe {amstream_ioc_get_version(self.hevc_device.as_raw_fd(), &mut amstream_version)};
        if ret != 0 {
            bail!(ErrorKind::Ioctl("amstream_ioc_get_version"));
        };
        let lower_v = (amstream_version & 0xFFFF) as u16;
        let upper_v = ((amstream_version & 0x7FFF0000) >> 16) as u16;
        Ok((upper_v, lower_v))
    }
}

#[cfg(target_arch = "aarch64")]
impl Drop for FbWrapper {
    fn drop(&mut self) {
        let fb0 = OpenOptions::new().write(true).open("/dev/fb0");
        // restore screen settings
        if let Ok(fb0) = fb0 {
            let ret = unsafe {
                fbio_set_vscreen_info(fb0.as_raw_fd(), &mut self.screeninfo as *mut _ as *mut u8)
            };
            if ret < 0 {
                println!("amcodec: ioctl call to fbio_set_vscreen_info went wrong, status code {}", ret);
            }
        } else {
            // if this happens then this is very weird ... we had permission to set it at the
            // beginning but we can't do it after we're done ? Did someone change our rights while
            // we were playing ?
            println!("amcodec: Unable to restore screen settings for fb0, permission denied");
        }
    }
}

#[derive(Debug)]
pub enum EndReason {
    EOF,
    // the EndReason "Error" is unused for now, but we might find a use later:
    // I haven't found yet an error that was so fatal in the middle of the playback that it stopped
    // the playback totally
    #[allow(unused)]
    Error(String),
}

#[derive(Debug)]
pub enum Message {
    Play,
    Pause,
    Resize(i16, i16, u16, u16),
    Fullscreen,
}

#[cfg(target_arch = "aarch64")]
pub fn main_loop(mut amcodec: Amcodec,
                   rx: Receiver<(Message, SuSender<FfiErrorCode>)>,
                   packet_channel: Receiver<LibavPacket>,
                   status_sender: Sender<EndReason>,
                   keep_running: Arc<AtomicBool>) {
    while keep_running.load(Ordering::SeqCst) == true {
        match rx.try_recv() {
            Ok((Message::Fullscreen, tx)) => {
                if let Err(e) = amcodec.set_fullscreen() {
                    println!("amcodec_thread: error when setting fullscreen: {}", e.display());
                    tx.send(error_to_ecode(e));
                } else {
                    tx.send(FfiErrorCode::None);
                }
            }
            Ok((Message::Resize(x, y, width, height), tx)) => {
                if let Err(e) = amcodec.set_video_axis((x, y, width, height)) {
                    println!("amcodec_thread: error when setting position: {}", e.display());
                    tx.send(error_to_ecode(e));
                } else {
                    tx.send(FfiErrorCode::None);
                }
            },
            Ok((Message::Play, tx)) => {
                if let Err(e) = amcodec.play() {
                    println!("amcodec_thread: error setting playing state: {}", e.display());
                    tx.send(error_to_ecode(e));
                } else {
                    tx.send(FfiErrorCode::None);
                }
            },
            Ok((Message::Pause, tx)) => {
                if let Err(e) = amcodec.pause() {
                    println!("amcodec_thread: error setting paused state: {}", e.display());
                    tx.send(error_to_ecode(e));
                } else {
                    tx.send(FfiErrorCode::None);
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
                println!("amcodec_thread: uh oh ...");
                break;
            },
            // no message
            Err(_) => {}
        };
        match packet_channel.try_recv() {
            Ok(p) => {
                if let Err(e) = amcodec.process_packet(p) {
                    println!("amcodec_thread: error when processing packet: {}", e.display());
                };
            },
            Err(TryRecvError::Disconnected) => {
                // the packet channel is disconnected, but it doesn't mean we should stop palyback
                // yet. Maybe the other thread crashed or something, but we can still keep going
                // our playback
                // However, maybe we would check here if the state is "InitialState", and if it is,
                // we would break our loop as well.
            },
            // no message
            Err(_) => {}
        }
        // Update Amcodec's internal pseudo state machine
        match amcodec.update_state() {
            Err(e) => {
                println!("amcodec_thread: error when updating internal state: {}", e.display());
            },
            Ok(true) => {
                // if it returns Ok(true), we should replace this by a new Amcodec (to "clear" the
                // buffer)
                // I couldn't find any other or better way than to close and reopen the device
                // again to "flush".
                drop(amcodec);
                amcodec = match Amcodec::new(status_sender.clone()) {
                    Ok(amcodec) => amcodec,
                    Err(e) => {
                        println!("amcodec_thread: error when opening amcodec: {}\nAborting.", e.display());
                        return ();
                    }
                };
            },
            Ok(_) => {},
        }
        // small sleep time avoids active waiting
        thread::sleep(Duration::from_millis(10));
    }
    if cfg!(debug_assertions) {
        println!("amcodec_thread: shutting down ...");
    }
}
