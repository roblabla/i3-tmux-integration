extern crate pty;
extern crate libc;
extern crate termios;
extern crate twoway;
#[macro_use(chan_select)]
extern crate chan;

use std::env;
use std::ptr;
use std::io::{self, Read, Write};
use std::ffi::CString;
use std::thread;
use termios::*;
use std::os::unix::ffi::OsStringExt;

const TMUX_DEC : [u8; 7]= [0o33, 'P' as u8, '1' as u8, '0' as u8, '0' as u8, '0' as u8, 'p' as u8];

fn toggle_opost(on : bool) {
    let mut term = Termios::from_fd(0).unwrap();
    if on {
        term.c_oflag |= OPOST;
    } else {
        term.c_oflag &= !OPOST;
    }
    tcsetattr(0, TCSANOW, &mut term).unwrap();
}

enum Modes {
    LookingForTmuxDec,
    ParsingTmux
}

fn print_tmux_msg() {
    toggle_opost(true);
    println!("** tmux mode started **");
    println!("Command Menu");
    println!("----------------------------");
    println!("esc    Detach cleanly.");
    println!("  X    Force-quit tmux mode.");
    println!("  L    Toggle logging.");
    println!("  C    Run tmux command.");
    toggle_opost(false);
}

fn setup_term() {
    let mut term = Termios::from_fd(0).unwrap();
    term.c_lflag &= !(ECHO | ICANON | ECHONL | ISIG | IEXTEN);
    term.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    term.c_cflag &= !(CSIZE | PARENB);
    term.c_cflag |= CS8;
    term.c_oflag &= !OPOST;
    term.c_cc[VMIN] = 1;
    term.c_cc[VTIME] = 0;
    tcsetattr(0, TCSANOW, &mut term).unwrap();
}

// TODO: Figure out safe, correct way to send a slice via chan.
fn readers<'a, 'b>(mut pty_master : pty::ChildPTY) -> (chan::Receiver<([u8;4096], usize)>, chan::Receiver<([u8;4096], usize)>) {
    let (tx1, rx1) = chan::sync(0);
    let (tx2, rx2) = chan::sync(0);
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        loop {
            let read = pty_master.read(&mut bytes).unwrap();
            tx1.send((bytes, read));
        }
    });
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        loop {
            let read = io::stdin().read(&mut bytes).unwrap();
            tx2.send((bytes, read));
        }
    });
    return (rx1, rx2);
}

fn handle_tmux(chr: &[u8]) {
}

fn main() {
    let cmd = CString::new(env::args_os().nth(1).unwrap().into_vec()).unwrap();
    let child = pty::fork().unwrap();
    if child.pid() == 0 {
        let args = [cmd.as_ptr(), ptr::null()].as_mut_ptr();
        unsafe { libc::execvp(cmd.as_ptr(), args) };
    } else {
        setup_term();
        let mut pty_master = child.pty().unwrap();
        let (rx1, rx2) = readers(pty_master.clone());
        let mut mode = Modes::LookingForTmuxDec;
        loop  {
            chan_select! {
                rx1.recv() -> val => {
                    let (bytes, read) = val.unwrap();
                    match mode {
                        Modes::LookingForTmuxDec =>
                            match twoway::find_bytes(&bytes[..read], &TMUX_DEC) {
                                Some(x) => {
                                    io::stdout().write(&bytes[..x]).unwrap();
                                    io::stdout().flush().unwrap();
                                    print_tmux_msg();
                                    // TODO : go in ParseTmux for the rest of the val...
                                    mode = Modes::ParsingTmux;
                                },
                                None => {
                                    io::stdout().write(&bytes[..read]).unwrap();
                                    io::stdout().flush().unwrap();
                                }
                            },
                        Modes::ParsingTmux => {
                            // TODO: Look for "\n", bufferize
                            handle_tmux(&bytes[..read]);
                        }
                    }
                },
                rx2.recv() -> val => {
                    let (bytes, read) = val.unwrap();
                    pty_master.write(&bytes[..read]).unwrap();
                    pty_master.flush().unwrap();
                },
            }
        }
    }
}
