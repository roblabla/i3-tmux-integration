use nix;
use nix::sys::ioctl;
use std;
use std::os::unix::io::RawFd;
use libc::winsize;

const TIOCGWINSZ: u64 = 0x00005413;
const TIOCSWINSZ: u64 = 0x00005414;

pub fn get_terminal_size(fd: RawFd) -> nix::Result<winsize> {
    let mut size : winsize = unsafe { std::mem::zeroed() };
    convert_ioctl_res!(unsafe { ioctl::ioctl(fd, TIOCGWINSZ, &mut size) })
        .map(|_| size)
}

pub fn set_terminal_size(fd: RawFd, size: winsize) -> nix::Result<()> {
    convert_ioctl_res!(unsafe { ioctl::ioctl(fd, TIOCSWINSZ, &size) })
        .map(|_| ())
}
