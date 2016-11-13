use std::io::{Read, Write, Result};
use std::os::unix::io::RawFd;
use libc::{self, c_void};
use std;

#[derive(Debug)]
pub struct Fd(pub RawFd);

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