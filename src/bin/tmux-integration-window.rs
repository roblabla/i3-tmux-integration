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
    // Get the arguments
    let path = std::env::args().nth(1).unwrap();
    let paneid = std::env::args().nth(2).unwrap().parse::<u64>().unwrap();

    // Create a socket to to connect to the i3-tmux-integration service
    let master_socket = UnixDatagram::unbound().unwrap();
    master_socket.connect(path).unwrap();

    // Create the signalfd fd.
    let mut signals = SigSet::empty();
    signals.add(Signal::SIGWINCH);
    signals.thread_block();
    let mut signalfd = SignalFd::new(&signals).unwrap();

    // It turns out signalfd works in a strange way when the fd is passed to
    // another process. When the receiving process reads from that fd, it will
    // get its own signal, not the signalfd's creator's. So what we'll do is
    // create a pipe and send the new size through it.
    let (read, write) = nix::unistd::pipe().unwrap();
    let mut writefile = unsafe { Fd::new(write) };

    // Write the paneid in the packet to send to the i3-tmux-integration service.
    let mut slice = [0u8; 8];
    Cursor::new(slice.as_mut()).write_u64::<NativeEndian>(paneid).unwrap();
    let iov = [IoVec::from_slice(&slice)];

    // Send stdin, stdout, and the pipe's FDs
    let arr = [0, 1, read];
    let cmsg = [ControlMessage::ScmRights(&arr[0..3])];

    // Send the packet
    sendmsg(master_socket.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None).unwrap();

    // Wait for a signal. When we have one, send it through the pipe.
    loop {
        if let Some(_) = signalfd.read_signal().unwrap() {
            let size = i3_tmux_integration::get_terminal_size(STDIN_FILENO).unwrap();
            let size_ptr: *const u8 = &size as *const winsize as *const u8;
            let size_slice: &[u8] = unsafe {
                std::slice::from_raw_parts(size_ptr, std::mem::size_of::<winsize>())
            };
            writefile.write_all(size_slice).unwrap();
        }
    }
}
