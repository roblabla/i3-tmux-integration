use nix;
use std;
use std::os::unix::io::RawFd;
use libc;
use libc::{winsize, TIOCGWINSZ, TIOCSWINSZ};

// TODO: Use ioctl macro
pub fn get_terminal_size(fd: RawFd) -> nix::Result<winsize> {
    let mut size : winsize = unsafe { std::mem::zeroed() };
    convert_ioctl_res!(unsafe { libc::ioctl(fd, TIOCGWINSZ, &mut size) })
        .map(|_| size)
}

pub fn set_terminal_size(fd: RawFd, size: winsize) -> nix::Result<()> {
    convert_ioctl_res!(unsafe { libc::ioctl(fd, TIOCSWINSZ, &size) })
        .map(|_| ())
}
