//! Some utilities shared by both tmux-integration-window and
//! i3-tmux-integration

extern crate libc;
#[macro_use]
extern crate nix;
mod termsize;
pub use termsize::*;
