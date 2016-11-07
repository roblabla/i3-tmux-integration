#[macro_use]
extern crate log;
extern crate termion;
#[macro_use]
extern crate chan;

use std::os::unix::net::UnixStream;
use termion::raw::IntoRawMode;
use std::thread;
use std::io::{Read, Write};

fn readers(mut stream: UnixStream) -> (chan::Receiver<([u8;4096], usize)>, chan::Receiver<([u8;4096], usize)>) {
    let (tx1, rx1) = chan::sync(0); // TODO: might want to make this a sync channel instead of rdv
    let (tx2, rx2) = chan::sync(0);
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        loop {
            let read = match stream.read(&mut bytes) {
                Ok(read) => read,
                Err(_) => {
                    error!("Got an error reading from stdin");
                    break
                }
            };
            tx1.send((bytes, read));
        }
    });
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        loop {
            let read = match std::io::stdin().read(&mut bytes) {
                Ok(read) => read,
                Err(_) => {
                    info!("Got an error reading from stdout");
                    break
                }
            };
            tx2.send((bytes, read));
        }
    });
    (rx1, rx2)
}

fn main() {
    let mut sock = UnixStream::connect(std::env::args_os().nth(1).unwrap()).unwrap();
    let mut paneid = std::env::args().nth(2).unwrap().parse::<u32>().unwrap();
    let mut raw_stdout = std::io::stdout().into_raw_mode().unwrap();
    let (stdout, stdin) = readers(sock.try_clone().unwrap());

    loop {
        chan_select! {
            stdout.recv() -> val => {
                let (bytes, read) = match val {
                    Some(x) => x,
                    None => break
                };
                if bytes.read_u8::<NativeEndian>() == 0 && bytes.read_u32::<NativeEndian>() == paneid {
                    raw_stdout.write(&bytes[..read]).unwrap();
                }
            },
            stdin.recv() -> val => {
                let (bytes, read) = match val {
                    Some(x) => x,
                    None => break
                };
                sock.write_u8::<NativeEndian>(1).unwrap();
                sock.write_u32::<NativeEndian>(paneid).unwrap();
                sock.write(&bytes[..read]).unwrap();
            }
        }
    }
}
