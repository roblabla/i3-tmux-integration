use std;
use raw_termios as termios;
use std::os::unix::io::{RawFd, AsRawFd};
use std::io::Write;
use std::ops::Deref;

pub struct Termios<T: AsRawFd + Write> {
    term: termios::Termios,
    orig: termios::Termios,
    fd: T
}

impl<T: AsRawFd + Write> Termios<T> {
    pub fn new(fd: T) -> std::io::Result<Self> {
        let res = termios::Termios::from_fd(fd.as_raw_fd());
        res.map(|term| Termios {
            term: term,
            orig: term.clone(),
            fd: fd
        })
    }

    pub fn set_raw_mode(&mut self) -> std::io::Result<()> {
        termios::cfmakeraw(&mut self.term);
        self.setattr()
    }

    pub fn setattr(&mut self) -> std::io::Result<()> {
        termios::tcsetattr(self.fd.as_raw_fd(), termios::TCSANOW, &self.term)
    }
}

impl<T: AsRawFd + Write> Drop for Termios<T> {
    fn drop(&mut self) {
        termios::tcsetattr(self.fd.as_raw_fd(), termios::TCSANOW, &self.orig);
    }
}

impl<T: AsRawFd + Write> Deref for Termios<T> {
    type Target = termios::Termios;
    fn deref(&self) -> &termios::Termios {
        &self.term
    }
}

impl<T: AsRawFd + Write> std::io::Write for Termios<T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.fd.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.fd.flush()
    }
}

// TODO: Impl write/read ? But then I need multiple RawFds :|.
