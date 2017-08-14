extern crate pty;
extern crate libc;
extern crate termios as raw_termios;
extern crate tempdir;
extern crate twoway;
#[macro_use]
extern crate log;
extern crate fern;
extern crate i3ipc;
extern crate byteorder;
#[macro_use]
extern crate nom;
extern crate nix;
extern crate unescape;
extern crate i3_tmux_integration;
extern crate mio;
extern crate mio_uds;
#[macro_use]
extern crate error_chain;

mod layout;
mod termios;
mod error;

use mio::*;
use mio::unix::EventedFd;
use i3_tmux_integration::{get_terminal_size, set_terminal_size};
use unescape::unescape;
use termios::Termios;
use i3_tmux_integration::Fd;
use mio_uds::UnixDatagram;
use std::os::unix::io::AsRawFd;
use std::collections::HashMap;
use i3ipc::I3Connection;
use std::env;
use std::path::Path;
use std::ptr;
use std::io::{self, Read, Write, Cursor};
use std::ffi::{CString, CStr};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::io::RawFd;
use pty::fork::*;
use libc::{winsize, STDIN_FILENO, STDOUT_FILENO};
use nix::sys::socket::{recvmsg, MsgFlags, CmsgSpace, ControlMessage};
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::SignalFd;
use nix::sys::uio::{IoVec};
use nix::unistd::Pid;
use byteorder::{ReadBytesExt, NativeEndian};

const TMUX_DEC : [u8; 7]= [0o33, 'P' as u8, '1' as u8, '0' as u8, '0' as u8, '0' as u8, 'p' as u8];

enum InputModes<'a> {
    LookingForTmuxDec(I3Connection, &'a Path),
    TmuxWaiting(I3Connection, String, &'a Path),
    TmuxCommandBlock(I3Connection, String, &'a Path)
}

struct Pane<'a> {
    id: u64,
    x11win: Option<u64>,
    // Need to store the rx side until the thread is created when we
    // receive stuff on the unix socket...
    _to_write: Vec<Vec<u8>>,
    read_fd: Option<Fd>,
    size_fd: Option<Fd>,
    write_fd: Option<termios::Termios<Fd>>,
    poll: Option<&'a Poll>
}

// TODO: Impl Read for Pane

impl<'a> Write for Pane<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(ref mut fd) = self.write_fd {
            fd.write(buf)
        } else {
            self._to_write.push(buf.into());
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(ref mut fd) = self.write_fd {
            fd.flush()
        } else {
            Ok(())
        }
    }
}

impl<'a> Pane<'a> {
    pub fn new(id: u64, path: &Path, ipc: &mut I3Connection) -> Pane<'a> {
        // Start the terminal on creation.
        let mut exepath = std::env::current_exe().unwrap();
        exepath.set_file_name("tmux-integration-window");
        let running = format!("workspace tmp; exec RUST_BACKTRACE=1 i3-sensible-terminal --hold -e {} {} {}", exepath.display(), path.display(), id);
        ipc.command(&running).unwrap();
        Pane {
            id: id,
            x11win: None,
            _to_write: vec![],
            read_fd: None,
            size_fd: None,
            write_fd: None,
            poll: None
        }
    }
}

impl<'a> Drop for Pane<'a> {
    fn drop(&mut self) {
        if let (Some(poll), Some(read_fd), Some(size_fd)) = (self.poll.take(), self.read_fd.take(), self.size_fd.take())  {
            poll.deregister(&EventedFd(&read_fd.as_raw_fd())).unwrap();
            poll.deregister(&EventedFd(&size_fd.as_raw_fd())).unwrap();
        }
    }
}

impl<'a> InputModes<'a> {
    fn handle_input(mut self, bytes: &[u8], panes: &mut HashMap<u64, Pane>) -> Self {
        match self {
            InputModes::LookingForTmuxDec(mut ipc, path) => match twoway::find_bytes(bytes, &TMUX_DEC) {
                Some(x) => {
                    std::io::stdout().write(&bytes[..x]).unwrap();
                    print_tmux_msg();
                    // TODO: Make sure it creates a new workspace
                    ipc.command("workspace tmux").unwrap();
                    // TODO: Set the client size
                    // TODO: Put stdout back into normal mode
                    self = InputModes::TmuxWaiting(ipc, "tmux".into(), path);
                    self.handle_input(&bytes[x + TMUX_DEC.len()..], panes)
                },
                None => {
                    std::io::stdout().write(bytes).unwrap();
                    std::io::stdout().flush().unwrap();
                    InputModes::LookingForTmuxDec(ipc, path)
                }
            },
            InputModes::TmuxWaiting(mut ipc, workspace, path) => {
                let mut size = 0usize;
                for mut line in bytes.split(|&e| e == '\n' as u8) {
                    size += line.len() + 1;
                    // TODO: figure out what happens in case it's not utf8
                    if line.len() > 0 && line[line.len() - 1] == b'\r' {
                        line = &line[..line.len() - 1];
                    }
                    let mut iter = std::str::from_utf8(line).unwrap().split(' ');
                    let cmd = match iter.next() {
                        Some(cmd) => cmd.trim(),
                        None => continue
                    };
                    let args : Vec<&str> = iter.collect();
                    info!("Command : '{}' {:?}", cmd, args);
                    match cmd {
                        "%begin" => {
                            ()//self = InputModes::TmuxCommandBlock(ipc, workspace, path);
                        },
                        "%exit" => {
                            panes.clear();
                            self = InputModes::LookingForTmuxDec(ipc, path);
                            return self.handle_input(&bytes[size..], panes);
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
                            let pane = panes.entry(paneid).or_insert_with(|| Pane::new(paneid, path, &mut ipc));
                            pane.write(&args[2..].iter().fold(unescape(args[1]).unwrap(), |mut acc, &x| {
                                acc.push_str(" ");
                                acc.push_str(&unescape(x).unwrap());
                                acc
                            }).into_bytes()[..]).unwrap();
                        },
                        "%session-changed" => (),
                        "%session-renamed" => (),
                        "%sessions-changed" => (),
                        "%unlinked-window-add" => (),
                        "%window-add" => {
                            let windowid = args[0];
                        },
                        "%window-close" => (),
                        "%window-renamed" => (),
                        cmd => {
                            info!("Unknown command");
                        }
                    }
                }
                InputModes::TmuxWaiting(ipc, workspace, path)
            },
            InputModes::TmuxCommandBlock(..) => {
                self
                /*let mut size = 0usize;
                for mut lines in bytes.split(|&e| e == '\n' as u8) {
                    size += line.len() + 1;
                    if line.len() > 0 && line[line.len() - 1] == b'\r' {
                        line = &line[..line.len() - 1];
                    }
                }*/
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

macro_rules! tmux_cmd {
    ($dst: expr, $($arg: tt)*) => {{
        info!("Sending cmd: {}", format!($($arg)*));
        writeln!($dst, $($arg)*)
    }}
}

fn to_hex(bytes: &[u8]) -> String {
    let mut mystr = String::with_capacity(bytes.len() * 5);
    for byte in bytes {
        mystr.push_str("0x");
        if (byte >> 4) < 10u8 {
            mystr.push(((byte >> 4) + b'0') as char)
        } else {
            mystr.push(((byte >> 4) - 10u8 + b'a') as char)
        }
        if (byte & 0xf) < 10u8 {
            mystr.push(((byte & 0xf) + b'0') as char)
        } else {
            mystr.push(((byte & 0xf) - 10u8 + b'a') as char)
        }
        mystr.push(' ');
    }
    mystr
}

// Macro that calls `continue` on nonblock error
macro_rules! try_cont {
    ($e:expr) => (match $e {
        Ok(v) => v,
        Err(ref e) if e.kind() == ::std::io::ErrorKind::WouldBlock => continue,
        Err(e) => return Err(From::from(e))
    })
}

const SUBWINDOW_INPUT_PANEID_MOD : usize = 0;
const SUBWINDOW_SIZE_PANEID_MOD : usize = 1;
const FD_PER_PANEID : usize = 2;
const MAIN_STDIN : Token = Token(0);
const MAIN_STDOUT : Token = Token(1);
const MAIN_SIGFD : Token = Token(2);
const SUBWINDOW_SOCKET : Token = Token(3);
const FIRST_PANEID : usize = 4;

fn token_to_pane(t: usize) -> usize {
    assert!(t >= FIRST_PANEID);
    (t - FIRST_PANEID) / FD_PER_PANEID
}

fn token_is_subwindow_input(t: usize) -> bool {
    t >= FIRST_PANEID && t % FD_PER_PANEID == SUBWINDOW_INPUT_PANEID_MOD
}

fn token_is_subwindow_size(t: usize) -> bool {
    t >= FIRST_PANEID && t % FD_PER_PANEID == SUBWINDOW_SIZE_PANEID_MOD
}

fn main_window_loop(shell_pid: Pid, master: &mut pty::fork::Master) -> error::Result<libc::c_void> {
    // Create the Unix Datagram socket that will allow the tmux subwindow to
    // send the stdin/stdout/termsize FD
    let tempdir = tempdir::TempDir::new("i3-tmux")?;
    let subwindow_creation_sockpath = tempdir.path().join("server.sock");
    let subwindow_creation_socket = UnixDatagram::bind(&*subwindow_creation_sockpath)?;

    // Connect to the I3 IPC
    let ipc = I3Connection::connect()?;

    // Create our Mio Event loop
    let poll = Poll::new()?;

    // Create the map of Pane
    let mut panes : HashMap<u64, Pane> = HashMap::new();

    // put stdin in Raw Mode so we can act as a proxy between the terminal and
    // the shell.
    let mut raw_termios = Termios::new(Fd::new(STDIN_FILENO))?;
    raw_termios.set_raw_mode()?;

    // Initialize the tmux parser state
    let mut input_mode = InputModes::LookingForTmuxDec(ipc, &subwindow_creation_sockpath);

    // Resize the shell
    let mut main_window_signals = SigSet::empty();
    main_window_signals.add(Signal::SIGWINCH);
    main_window_signals.thread_block()?;
    let mut signalfd = SignalFd::with_flags(&main_window_signals, nix::sys::signalfd::SFD_NONBLOCK).unwrap();

    // Initialize various variables
    let mut buf = [0u8; 4096];
    let mut events = Events::with_capacity(1024);

    // Register the "main" things.
    poll.register(&EventedFd(&master.as_raw_fd()), MAIN_STDIN, Ready::readable(), PollOpt::level())?;
    poll.register(&EventedFd(&STDIN_FILENO), MAIN_STDOUT, Ready::readable(), PollOpt::level())?;
    poll.register(&EventedFd(&signalfd.as_raw_fd()), MAIN_SIGFD, Ready::readable(), PollOpt::level())?;
    poll.register(&subwindow_creation_socket, SUBWINDOW_SOCKET, Ready::readable(), PollOpt::level())?;

    // Set the initial size of the subshell. We don't go fast enough to get the
    // first sigwinch
    {
        let size = get_terminal_size(STDIN_FILENO)?;
        set_terminal_size(master.as_raw_fd(), size)?;
        nix::sys::signal::kill(shell_pid, Signal::SIGWINCH)?;
    }

    loop {
        // Check for new events
        poll.poll(&mut events, None)?;
        for event in events.iter() {
            match event.token() {
                // User typed data on the main window
                MAIN_STDIN => {
                    let read = match try_cont!(master.read(&mut buf)) {
                        0 => /* TODO: bash was closed */(),
                        read => input_mode = input_mode.handle_input(&buf[..read], &mut panes),
                    };
                },
                // The main window's shell printed data
                MAIN_STDOUT => {
                    let read = try_cont!(io::stdin().read(&mut buf));
                    if let InputModes::LookingForTmuxDec(..) = input_mode {
                        master.write(&buf[..read])?;
                        master.flush()?;
                    } else { if buf.iter().find(|e| **e == 27).is_some() {
                        info!("Detaching");
                        master.write("detach\n".as_ref())?;
                        master.flush()?;
                    } else {
                        info!("{:?}", &buf[..read]);
                    }}
                },
                // The main window's size has changed
                MAIN_SIGFD => {
                    if let Some(_) = signalfd.read_signal()? {
                        let size = get_terminal_size(STDIN_FILENO)?;
                        set_terminal_size(master.as_raw_fd(), size)?;
                        nix::sys::signal::kill(shell_pid, Signal::SIGWINCH)?;
                    }
                },
                // A new subwindow was created, it sent its fds to our socket
                SUBWINDOW_SOCKET => {
                    // Get the pane id and file descriptors
                    let iovec = [IoVec::from_mut_slice(&mut buf)];
                    let mut space = CmsgSpace::<[RawFd; 3]>::new();
                    let msg = match recvmsg(subwindow_creation_socket.as_raw_fd(), &iovec, Some(&mut space), MsgFlags::empty()) {
                        Ok(v) => v,
                        Err(nix::Error::Sys(nix::Errno::EAGAIN)) => continue,
                        Err(err) => return Err(From::from(err))
                    };
                    let (paneid, fds) = if let Some(ControlMessage::ScmRights(fds)) = msg.cmsgs().next() {
                        (Cursor::new(iovec[0].as_slice()).read_u64::<NativeEndian>()?, fds.to_vec())
                    } else {
                        // TODO: Error it
                        panic!("Got an error reading from unix socket");
                        //break
                    };

                    // Try to find which Pane is this paneid
                    if let Some(pane) = panes.get_mut(&paneid) {
                        // Write this pane's pending data, set the read/write
                        // FDs, and register the new panel to the event loop
                        let mut raw_fd = Termios::new(Fd::new(fds[1])).unwrap();
                        raw_fd.set_raw_mode().unwrap();
                        pane.write_fd = Some(raw_fd);
                        let bytesvec = std::mem::replace(&mut pane._to_write, vec![]);
                        for bytes in bytesvec {
                            pane.write_all(&bytes).unwrap();
                        }
                        pane.poll = Some(&poll);
                        {
                            let fd = Fd::new(fds[0]);
                            // Set nonblock ?
                            poll.register(&fd, Token(((paneid as usize * FD_PER_PANEID) + 0) + FIRST_PANEID), Ready::readable(), PollOpt::level())?;
                            pane.read_fd = Some(fd);
                        }
                        {
                            let fd = Fd::new(fds[2]);
                            poll.register(&fd, Token(((paneid as usize * FD_PER_PANEID) + 1) + FIRST_PANEID), Ready::readable(), PollOpt::level())?;
                            pane.size_fd = Some(fd);
                        }
                    } else {
                        panic!("WTF?");
                    }
                },
                // A subwindow's shell printed data
                Token(i) if token_is_subwindow_input(i) => {
                    let paneid = token_to_pane(i);
                    // TODO: How to handle panics ?
                    if let Some(ref mut pane) = panes.get_mut(&(paneid as u64)) {
                        let read_fd = pane.read_fd.as_mut().unwrap();
                        let read = try_cont!(read_fd.read(&mut buf));
                        if read == 0 {
                            // No more input updates. The shell was probably closed.
                            // TODO: There *has* to be a clean way to wait for pid
                            // death, but I can't find one. The best I could find
                            // was to dedicate a thread by pid to loop waitpid. WAT
                            tmux_cmd!(master, "kill-pane -t {}", paneid)?;
                            continue;
                        }
                        tmux_cmd!(master, "send-keys -t {} {}", paneid, to_hex(&buf[..read]))?;
                    } else {
                        // TODO: The pane got closed ?
                    }
                },
                // A subwindow's size changed
                Token(i) if token_is_subwindow_size(i) => {
                    let paneid = token_to_pane(i);
                    let size_fd = panes.get_mut(&(paneid as u64)).unwrap().size_fd.as_mut().unwrap();
                    let mut thisbuf = vec![0u8; std::mem::size_of::<winsize>()];
                    let read = try_cont!(size_fd.read(&mut thisbuf));
                    if read == 0 {
                        // No more window updates. I should probably deregister ?
                        continue;
                    } else if read != std::mem::size_of::<winsize>() {
                        panic!("Got a partial read for subwindow_size. Not too
                        sure how to handle that yet");
                    }
                    let size = thisbuf.as_ptr() as *const u8 as *const winsize;
                    // TODO: We're supposed to send the size of the whole client
                    // area, AKA the sum of every window.
                    unsafe { tmux_cmd!(master, "refresh-client -C {},{}", (*size).ws_col, (*size).ws_row)? };
                    unsafe { tmux_cmd!(master, "resize-pane -x {} -y {} -t %{}", (*size).ws_col, (*size).ws_row, paneid)? };
                    unsafe { info!("Pixel density : {},{}", (*size).ws_xpixel / (*size).ws_col, (*size).ws_ypixel / (*size).ws_row) };
                },
                _ => panic!("WTF?")
            }
        }
    }
}

fn main() {
    // Setup the logger
    fern::Dispatch::new()
        .level(log::LogLevelFilter::Trace)
        .chain(fern::log_file("output.log").unwrap())
        .apply().unwrap();

    let mut fork = Fork::from_ptmx().unwrap();
    if let Fork::Parent(pid, ref mut master) = fork {
        main_window_loop(Pid::from_raw(pid), master).unwrap();
    } else {
        // Start the user's shell.
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
