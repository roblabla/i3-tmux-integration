#[macro_use]
extern crate log;
extern crate termion;
#[macro_use]
extern crate chan;
extern crate byteorder;
extern crate sendfd;

use sendfd::UnixSendFd;
use byteorder::{NativeEndian};
use std::os::unix::net::UnixDatagram;
use termion::raw::IntoRawMode;
use std::thread;
use std::io::{Read, Write};


fn main() {
    let path = std::env::args_os().nth(1);
    //let socket = UnixDatagram::connect();
    /*socket.sendfd(0);
    socket.sendfd(1);
    socket.sendfd(2);*/
}
