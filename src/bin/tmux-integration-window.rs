extern crate nix;
extern crate byteorder;
extern crate i3_tmux_integration;

use byteorder::{WriteBytesExt, NativeEndian};
use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg};
use nix::sys::uio::IoVec;
use std::os::unix::net::UnixDatagram;
use std::os::unix::io::{AsRawFd};
use std::io::Cursor;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let paneid = std::env::args().nth(2).unwrap().parse::<u64>().unwrap();
    let master_socket = UnixDatagram::unbound().unwrap();
    let mut slice = [0u8; 8];
    Cursor::new(slice.as_mut()).write_u64::<NativeEndian>(paneid).unwrap();
    let iov = [IoVec::from_slice(&slice)];
    let arr = [0, 1];
    let cmsg = [ControlMessage::ScmRights(&arr[0..2])];
    master_socket.connect(path).unwrap();
    sendmsg(socket.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None).unwrap();
    loop {
        std::thread::sleep(std::time::Duration::new(500, 0));
    }
}
