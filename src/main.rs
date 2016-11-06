extern crate pty;
extern crate libc;
extern crate termion;
extern crate twoway;
#[macro_use(chan_select)]
extern crate chan;
#[macro_use]
extern crate log;
extern crate fern;
extern crate i3ipc;

use i3ipc::I3Connection;
use std::env;
use std::ptr;
use std::io::{self, Read, Write};
use std::ffi::CString;
use std::thread;
use termion::raw::IntoRawMode;
use std::os::unix::ffi::OsStringExt;
use pty::fork::*;

const TMUX_DEC : [u8; 7]= [0o33, 'P' as u8, '1' as u8, '0' as u8, '0' as u8, '0' as u8, 'p' as u8];

#[derive(Debug)]
enum InputModes {
    LookingForTmuxDec(I3Connection),
    TmuxWaiting(I3Connection, String),
    TmuxCommandBlock(I3Connection, String)
}

impl InputModes {
    fn handle_input(mut self, bytes: &[u8]) -> Self {
        match self {
            InputModes::LookingForTmuxDec(mut ipc) => match twoway::find_bytes(bytes, &TMUX_DEC) {
                Some(x) => {
                    std::io::stdout().write(&bytes[..x]).unwrap();
                    print_tmux_msg();
                    // TODO: Make sure it creates a new workspace
                    ipc.command("workspace tmux").unwrap();
                    self = InputModes::TmuxWaiting(ipc, "tmux".into());
                    self.handle_input(&bytes[x + TMUX_DEC.len()..])
                },
                None => {
                    std::io::stdout().write(bytes).unwrap();
                    std::io::stdout().flush().unwrap();
                    InputModes::LookingForTmuxDec(ipc)
                }
            },
            InputModes::TmuxWaiting(ipc, workspace) => {
                info!("Command : {}", std::str::from_utf8(bytes).unwrap_or("BROKEN_UTF8"));
                let mut size = 0usize;
                for line in bytes.split(|&e| e == '\n' as u8) {
                    size += line.len() + 1;
                    let (cmd, args) = line.split_at(line.iter().position(|&e| e == ' ' as u8).unwrap_or(line.len().saturating_sub(1)));
                    match std::str::from_utf8(cmd).unwrap() {
                        "%begin" => (),
                        "%exit" => {
                            self = InputModes::LookingForTmuxDec(ipc);
                            return self.handle_input(&bytes[size..]);
                        },
                        "%layout-change" => (),
                        "%output" => (),
                        "%session-changed" => (),
                        "%session-renamed" => (),
                        "%sessions-changed" => (),
                        "%unlinked-window-add" => (),
                        "%window-add" => (),
                        "%window-close" => (),
                        "%window-renamed" => (),
                        cmd => {
                            error!("Unknown command \"{}\"", cmd);
                        }
                    }
                }
                InputModes::TmuxWaiting(ipc, workspace)
            },
            InputModes::TmuxCommandBlock(_, _) => {
                self
            }
        }
    }
}

fn print_tmux_msg() {
    println!("** tmux mode started **\r");
    println!("Command Menu\r");
    println!("----------------------------\r");
    println!("esc    Detach cleanly.\r");
    println!("  X    Force-quit tmux mode.\r");
    println!("  L    Toggle logging.\r");
    println!("  C    Run tmux command.\r");
}

// TODO: Figure out safe, correct way to send a slice via chan.
fn readers<'a, 'b>(mut pty_master : Master, pid: libc::pid_t) -> (chan::Receiver<([u8;4096], usize)>, chan::Receiver<([u8;4096], usize)>, chan::Receiver<()>) {
    let (tx1, rx1) = chan::sync(0); // TODO: might want to make this a sync channel instead of rdv
    let (tx2, rx2) = chan::sync(0);
    let (tx3, rx3) = chan::sync(0);
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        loop {
            let read = match pty_master.read(&mut bytes) {
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
            let read = match io::stdin().read(&mut bytes) {
                Ok(read) => read,
                Err(_) => {
                    info!("Got an error reading from stdout");
                    break
                }
            };
            tx2.send((bytes, read));
        }
    });
    thread::spawn(move || {
        unsafe { libc::waitpid(pid, &mut 0, 0) };
        tx3.send(());
    });
    return (rx1, rx2, rx3);
}

fn main() {
    let logger_config = fern::DispatchConfig {
        format: Box::new(|msg, _, _| {
            format!("{}", msg)
        }),
        output: vec![fern::OutputConfig::file("output.log")],
        level: log::LogLevelFilter::Trace
    };
    fern::init_global_logger(logger_config, log::LogLevelFilter::Trace).unwrap();

    let ipc = match I3Connection::connect() {
        Ok(ipc) => ipc,
        Err(err) => {
            write!(std::io::stderr(), "Error connecting to i3 IPC! {}\n", err).unwrap();
            let args : Vec<CString> = env::args_os().skip(1).map(|e| CString::new(e.into_vec()).unwrap()).collect();
            let mut args_ptrs : Vec<_> = args.iter().map(|e| e.as_ptr()).collect();
            args_ptrs.push(ptr::null());
            let args_ptr = args_ptrs.as_ptr();
            let cmd = args_ptrs[0];
            unsafe { libc::execvp(cmd, args_ptr) };
            unreachable!();
        }
    };

    let mut fork = Fork::from_ptmx().unwrap();
    if let Fork::Parent(pid, ref mut master) = fork {
        let mut raw_stdout = std::io::stdout().into_raw_mode().unwrap();
        let (input, output, close) = readers(master.clone(), pid);
        let mut input_mode = InputModes::LookingForTmuxDec(ipc);
        loop {
            chan_select! {
                input.recv() -> val => {
                    let (bytes, read) = match val {
                        Some((bytes, read)) => (bytes, read),
                        None => break
                    };
                    input_mode = input_mode.handle_input(&bytes[..read]);
                },
                output.recv() -> val => {
                    let (bytes, read) = match val {
                        Some((bytes, read)) => (bytes, read),
                        None => break
                    };
                    if let InputModes::LookingForTmuxDec(_) = input_mode {
                        master.write(&bytes[..read]).unwrap();
                        master.flush().unwrap();
                    } else { if bytes.iter().find(|e| **e == 27).is_some() {
                        info!("Detaching");
                        master.write("detach\r\n".as_ref()).unwrap();
                        master.flush().unwrap();
                    } else {
                        info!("{:?}", &bytes[..read]);
                    }}
                },
                close.recv() -> _ => {
                    break
                }
            }
        }
    } else {
        let cmd = CString::new(env::args_os().nth(1).unwrap().into_vec()).unwrap();
        let args = [cmd.as_ptr(), ptr::null()].as_mut_ptr();
        unsafe { libc::execvp(cmd.as_ptr(), args) };
    }
}
