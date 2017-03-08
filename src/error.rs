#[repr(i32)]
#[derive(Debug, Clone, Copy)]
/// These are the errors we will return when calling the .so API.
///
/// Here is the basic idea for error codes in here:
///
/// * ret == 0 : no error
/// * ret > 0 : API user error
/// * ret < 0 : unexpected error coming from this software
pub enum FfiErrorCode {
    InvalidCommand = 1,
    None = 0,
    Unknown = -1,
    Disconnected = -2,
    LibAvDisconnected = -3,
    LibAvInternal = -4,
    VideoDecodingError = -5,
    NoHevcStream = -6,
    X11DLOpenError = -7,
    X11Internal = -8,
    /// this is detected at initialisation,
    /// however we can only return NULL or a pointer right now
    /// (and no error code), so this is unused
    // WrongLibavVersion = -9,
    Bug = -42,
    Unreachable = -43,
    ShutdownError = -64,
}

// ecode stands for error_code
pub fn error_to_ecode(error: Error) -> FfiErrorCode {
    match error {
        Error(ErrorKind::LibavInternal(_, _), _) => FfiErrorCode::LibAvInternal,
        Error(ErrorKind::X11Other(_), _) => FfiErrorCode::Bug,
        Error(ErrorKind::X11Internal(_), _) => FfiErrorCode::X11Internal,
        Error(ErrorKind::EOF, _) => FfiErrorCode::Unreachable,
        Error(ErrorKind::NoValidVideoStream, _) => FfiErrorCode::NoHevcStream,
        Error(ErrorKind::X11DLOpenError(_), _) => FfiErrorCode::X11DLOpenError,
        Error(ErrorKind::WrongLibavVersion, _) => FfiErrorCode::Unreachable,
        Error(_, _) => FfiErrorCode::Unknown,
    }
}

// ecode stands for error_code
#[inline]
pub fn result_to_ecode(result: Result<()>) -> FfiErrorCode {
    match result {
        Ok(_) => FfiErrorCode::None,
        Err(e) => error_to_ecode(e),
    }
}

pub type FfiResult = ::std::result::Result<(), FfiErrorCode>;

pub fn ffi_result_to_int(ffi_result: FfiResult) -> ::std::os::raw::c_int {
    let r = match ffi_result {
        Ok(_) => FfiErrorCode::None,
        Err(error_code) => error_code,
    };
    r as ::std::os::raw::c_int
}

pub use error_chain::ChainedError;

// error_chain is a very cool rust package (or crate) that allows us to handle errors in a
// friendly fashion. It allows chaining errors with result.chain_err(|| ErrorKind::XX ), and
// returning early errors with bail!(ErrorKind::XX ); (which is a rough equivalent of return
// Err(ErrorKind::XX) with some extra code to be more generic
error_chain!{
    errors {
        LibavInternal(code: i32,s: &'static str) {
            description("libav call failed")
                display("libav call `{}` failed with {}", s, code)
        }
        X11Other(s: String) {
            description("unexpected X11 result")
            display("unexpected X11 result: {}", s)
        }
        X11Internal(code: u8) {
            description("X11 returned non-zero status code")
            display("internal X11 error: {}", code)
        }
        Ioctl(which: &'static str) {
            description("ioctl call failed")
            display("ioctl call to `{}` failed", which)
        }
        Amcodec {
            description("amcodec error")
            display("a call to amcodec driver failed")
        }
        FbPermission {
            description("not enough permissions to write on fb0")
            display("not enough permissions to write on fb0")
        }
        Disconnected {
            description("channel disconnected")
        }
        WrongLibavVersion {
            description("wrong libav version")
        }
        EOF
        NoValidVideoStream
    }

    foreign_links {
        X11DLOpenError(::x11_dl::error::OpenError);
    }
}
