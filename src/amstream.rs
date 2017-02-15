use error::*;
use std::fs::File;
use std::os::unix::io::AsRawFd;
use libc::c_int;

ioctl!(read amstream_ioc_get_version with b'S', 0xc0; c_int);

pub fn version(am_vbuf: &File) -> Result<(u16,u16)> {
    let fd = am_vbuf.as_raw_fd();
    let mut amstream_version : c_int = 0;
    let ret = unsafe {amstream_ioc_get_version(fd, &mut amstream_version)};
    if ret != 0 {
        bail!("ioctl: AMSTREAM_IOC_GET_VERSION ERROR");
    };
    let lower_v = (amstream_version & 0xFFFF) as u16;
    let upper_v = ((amstream_version & 0xFFFF0000) >> 16) as u16;
    Ok((upper_v, lower_v))
}
