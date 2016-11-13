extern crate pty;
extern crate libc;
extern crate termion;
extern crate tempdir;
extern crate twoway;
#[macro_use(chan_select)]
extern crate chan;
#[macro_use]
extern crate log;
extern crate fern;
extern crate i3ipc;
extern crate byteorder;
#[macro_use]
extern crate nom;
extern crate nix;

mod layout;
mod fd;

use std::os::unix::net::UnixDatagram;
use std::os::unix::io::AsRawFd;
use std::collections::HashMap;
use i3ipc::I3Connection;
use std::env;
use std::path::{PathBuf, Path};
use std::ptr;
use std::sync::Arc;
use std::sync::Mutex;
use std::io::{self, Read, Write, Cursor};
use std::ffi::{CString, CStr};
use std::thread;
use termion::raw::IntoRawMode;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::io::RawFd;
use pty::fork::*;
use nix::sys::socket::{recvmsg, MsgFlags, CmsgSpace, ControlMessage};
use nix::sys::uio::{IoVec};
use byteorder::{ReadBytesExt, NativeEndian};

const TMUX_DEC : [u8; 7]= [0o33, 'P' as u8, '1' as u8, '0' as u8, '0' as u8, '0' as u8, 'p' as u8];

// TODO: Figure out how to create an Arc<Path>
enum InputModes {
    LookingForTmuxDec(I3Connection, Arc<PathBuf>, Arc<Mutex<HashMap<u64, Pane>>>),
    TmuxWaiting(I3Connection, String, Arc<PathBuf>, Arc<Mutex<HashMap<u64, Pane>>>),
    TmuxCommandBlock(I3Connection, String, Arc<PathBuf>, Arc<Mutex<HashMap<u64, Pane>>>)
}

struct Pane {
    id: u64,
    x11win: Option<u64>,
    signal_fd: Option<fd::Fd>,
    output: chan::Sender<String>,
    // Need to store the rx side until the thread is created when we
    // receive stuff on the unix socket...
    _read: Option<chan::Receiver<String>>,
    thread_read: Option<std::thread::JoinHandle<()>>,
    thread_write: Option<std::thread::JoinHandle<()>>
}

impl Pane {
    pub fn new(id: u64, path: &PathBuf, ipc: &mut I3Connection) -> Pane {
        let (tx, rx) = chan::async();
        // Start the terminal on creation.
        let running = format!("workspace tmp; exec RUST_BACKTRACE=1 i3-sensible-terminal --hold -e /home/roblabla/i3-tmux-integration/target/debug/tmux-integration-window {} {}", path.display(), id);
        println!("{}", running);
        ipc.command(&running).unwrap();
        Pane {
            id: id,
            x11win: None,
            signal_fd: None,
            output: tx,
            _read: Some(rx),
            thread_read: None,
            thread_write: None
        }
    }
}

impl InputModes {
    fn handle_input(mut self, bytes: &[u8]) -> Self {
        match self {
            InputModes::LookingForTmuxDec(mut ipc, path, map) => match twoway::find_bytes(bytes, &TMUX_DEC) {
                Some(x) => {
                    std::io::stdout().write(&bytes[..x]).unwrap();
                    print_tmux_msg();
                    // TODO: Make sure it creates a new workspace
                    ipc.command("workspace tmux").unwrap();
                    self = InputModes::TmuxWaiting(ipc, "tmux".into(), path, map);
                    self.handle_input(&bytes[x + TMUX_DEC.len()..])
                },
                None => {
                    std::io::stdout().write(bytes).unwrap();
                    std::io::stdout().flush().unwrap();
                    InputModes::LookingForTmuxDec(ipc, path, map)
                }
            },
            InputModes::TmuxWaiting(mut ipc, workspace, path, panes) => {
                let mut size = 0usize;
                for mut line in bytes.split(|&e| e == '\n' as u8) {
                    size += line.len() + 1;
                    // TODO: figure out what happens in case it's not utf8
                    if line.len() > 0 && line[0] == b'\r' {
                        line = &line[1..]
                    }
                    let mut iter = std::str::from_utf8(line).unwrap().split(' ');
                    let cmd = match iter.next() {
                        Some(cmd) => cmd.trim(),
                        None => continue
                    };
                    let args : Vec<&str> = iter.collect();
                    info!("Command : {} {:?}", cmd, args);
                    match cmd {
                        "%begin" => (),
                        "%exit" => {
                            self = InputModes::LookingForTmuxDec(ipc, path, panes);
                            return self.handle_input(&bytes[size..]);
                        },
                        "%layout-change" => {
                            let windowid = args[0][1..].parse::<u64>();
                            let layout_str = &args[1][5..];
                            let layout = layout::Layout::parse(layout_str).unwrap();
                            info!("{:?}", layout);
                            // This is where the fun starts.
                        },
                        "%output" => {
                            let paneid = args[0][1..].parse::<u64>().unwrap();
                            let mut lock = panes.lock().unwrap();
                            let pane = lock.entry(paneid).or_insert_with(|| Pane::new(paneid, &*path, &mut ipc));
                            pane.output.send(args[1..].iter().fold("".to_string(), |mut acc, &x| { acc.push_str(x); acc }));
                        },
                        "%session-changed" => (),
                        "%session-renamed" => (),
                        "%sessions-changed" => (),
                        "%unlinked-window-add" => (),
                        "%window-add" => (),/*{
                            let windowid = args[0];
                            let mut path = tempdir.join("socket");

                            //ipc.command(format!("workspace tmux; exec urxvt -e 'tmux-integration-window {}; workspace back_and_forth'", path.display())).unwrap();
                        },*/
                        "%window-close" => (),
                        "%window-renamed" => (),
                        cmd => {
                            info!("Unknown command \"{:?}\"", line);
                        }
                    }
                }
                InputModes::TmuxWaiting(ipc, workspace, path, panes)
            },
            InputModes::TmuxCommandBlock(..) => {
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
fn readers<'a, 'b>(mut pty_master : Master, pid: libc::pid_t, tempsock: Arc<PathBuf>, panes: Arc<Mutex<HashMap<u64, Pane>>>) -> (chan::Receiver<([u8;4096], usize)>, chan::Receiver<([u8;4096], usize)>, chan::Receiver<()>) {
    let (tx1, rx1) = chan::sync(0); // TODO: might want to make this a sync channel instead of rdv
    let (tx2, rx2) = chan::sync(0);
    let (tx3, rx3) = chan::sync(0);
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        loop {
            let read = match pty_master.read(&mut bytes) {
                Ok(read) => read,
                Err(_) => {
                    info!("Got an error reading from stdin");
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
        let mut buf = [0u8; 8];
        let datagram = UnixDatagram::bind(&*tempsock).unwrap();
        loop {
            let iovec = [IoVec::from_mut_slice(&mut buf)];
            let mut space = CmsgSpace::<[RawFd; 2]>::new();
            let (paneid, fds) = match recvmsg(datagram.as_raw_fd(), &iovec, Some(&mut space), MsgFlags::empty()) {
                Ok(msg) => {
                    if let Some(ControlMessage::ScmRights(fds)) = msg.cmsgs().next() {
                        println!("{:?}", fds);
                        (Cursor::new(iovec[0].as_slice()).read_u64::<NativeEndian>().unwrap(), fds.to_vec())
                    } else {
                        info!("Got an error reading from unix socket");
                        break
                    }
                },
                Err(_) => {
                    info!("Got an error reading from unix socket");
                    break
                }
            };
            let mut lock = panes.lock().unwrap();
            if let Some(pane) = lock.get_mut(&paneid) {
                //TODO: put those f-ds into raw mode
                let mut write_fd = fd::Fd(fds[1]);
                let mut read_fd = fd::Fd(fds[0]);
                pane.thread_read = Some(std::thread::spawn(move || {
                    let mut bytes = [0u8; 4096];
                    loop {
                        match read_fd.read(&mut bytes) {
                            Ok(siz) => {
                                // This is where things goes to shit !
                                write!(pty_master, "send-keys {}", to_hex(&bytes[..siz])).unwrap();
                            },
                            Err(_) => {
                                // TODO: ?
                            }
                        };
                    }
                }));
                /*let rx = pane._read.take().unwrap();
                pane.thread_write = Some(std::thread::spawn(move || {
                    loop {
                        match rx.recv() {
                            Some(string) => write_fd.write_all(string.as_bytes()).unwrap(),
                            None => break
                        }
                    }
                }));*/
            }
        }
    });
    thread::spawn(move || {
        unsafe { libc::waitpid(pid, &mut 0, 0) };
        tx3.send(());
    });
    return (rx1, rx2, rx3);
}

fn to_hex(bytes: &[u8]) -> String {
    let mut mystr = String::with_capacity(bytes.len() * 3);
    for byte in bytes {
        if (byte >> 4) < 10u8 {
            mystr.push(((byte >> 4) + b'0') as char)
        } else {
            mystr.push(((byte >> 4) + b'a') as char)
        }
        if (byte & 0xf) < 10u8 {
            mystr.push(((byte & 0xf) + b'0') as char)
        } else {
            mystr.push(((byte & 0xf) + b'a') as char)
        }
        mystr.push(' ');
    }
    mystr
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

    let tempdir = tempdir::TempDir::new("i3-tmux").unwrap();
    let tempsock = Arc::new(tempdir.path().join("server.sock"));
    println!("{}", tempsock.display());
    let map = Arc::new(Mutex::new(HashMap::new()));

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
        let (input, output, close) = readers(master.clone(), pid, tempsock.clone(), map.clone());
        let mut input_mode = InputModes::LookingForTmuxDec(ipc, tempsock.clone(), map.clone());
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
                    if let InputModes::LookingForTmuxDec(..) = input_mode {
                        master.write(&bytes[..read]).unwrap();
                        master.flush().unwrap();
                    } else { if bytes.iter().find(|e| **e == 27).is_some() {
                        info!("Detaching");
                        master.write("detach\n".as_ref()).unwrap();
                        master.flush().unwrap();
                    } else {
                        info!("{:?}", &bytes[..read]);
                    }}
                },
                close.recv() -> _ => {
                    break
                },
            }
        }
    } else {
        let hold_cstring : CString;
        let cmd = unsafe {
            if let Some(str) = env::args_os().nth(1) {
                hold_cstring = CString::new(str.into_vec()).unwrap();
                &hold_cstring
            } else if let Some(passwd) = libc::getpwuid(libc::getuid()).as_ref() {
                CStr::from_ptr(passwd.pw_shell)
            } else {
                // TODO: exit cleanly, saying something's WRONG
                unreachable!()
            }
        };
        let args = [cmd.as_ptr(), ptr::null()].as_mut_ptr();
        unsafe { libc::execvp(cmd.as_ptr(), args) };
    }
}
