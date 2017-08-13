extern crate nix;
extern crate libc;
extern crate byteorder;
extern crate i3_tmux_integration;

use i3_tmux_integration::Fd;
use byteorder::{WriteBytesExt, NativeEndian};
use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg};
use nix::sys::uio::IoVec;
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::SignalFd;
use std::os::unix::net::UnixDatagram;
use std::os::unix::io::{AsRawFd};
use std::io::{Write, Cursor};
use libc::{STDIN_FILENO, winsize};

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let paneid = std::env::args().nth(2).unwrap().parse::<u64>().unwrap();
    let master_socket = UnixDatagram::unbound().unwrap();
    let mut signals = SigSet::empty();
    signals.add(Signal::SIGWINCH);
    let mut signalfd = SignalFd::new(&signals).unwrap();
    let mut slice = [0u8; 8];
    Cursor::new(slice.as_mut()).write_u64::<NativeEndian>(paneid).unwrap();
    let iov = [IoVec::from_slice(&slice)];
    let (write, read) = nix::unistd::pipe().unwrap();
    let arr = [0, 1, read];
    let cmsg = [ControlMessage::ScmRights(&arr[0..3])];
    master_socket.connect(path).unwrap();
    sendmsg(master_socket.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None).unwrap();
    let mut writefile = unsafe { Fd::new(write) };
    loop {
        if let Some(_) = signalfd.read_signal().unwrap() {
            let size = i3_tmux_integration::get_terminal_size(STDIN_FILENO).unwrap();
            let size_ptr: *const u8 = &size as *const winsize as *const u8;
            let size_slice: &[u8] = unsafe {
                std::slice::from_raw_parts(size_ptr, std::mem::size_of::<winsize>())
            };
            //writefile.write_all(size_slice).unwrap();
        }
    }
}
