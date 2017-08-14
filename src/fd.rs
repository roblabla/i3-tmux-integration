use std::io::{Read, Write, Result};
use std::os::unix::io::{RawFd, AsRawFd};
use libc::{self, c_void};
use std;
use mio::*;
use mio::unix::EventedFd;

#[derive(Debug)]
pub struct Fd(RawFd);

impl Fd {
    // TODO: Mark unsafe
    pub fn new(fd: RawFd) -> Fd {
        Fd(fd)
    }
}

impl Read for Fd {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let ret = unsafe {
            libc::read(self.0, buf.as_mut_ptr() as *mut c_void, buf.len())
        };
        if ret == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(ret as usize)
        }
    }
}

impl Write for Fd {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let ret = unsafe {
            libc::write(self.0, buf.as_ptr() as *const c_void, buf.len())
        };
        if ret == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(ret as usize)
        }
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

impl Evented for Fd {
    fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> Result<()> {
        EventedFd(&self.0).register(poll, token, interest, opts)
    }

    fn reregister(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> Result<()> {
        EventedFd(&self.0).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> Result<()> {
        EventedFd(&self.0).deregister(poll)
    }
}

impl AsRawFd for Fd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}
