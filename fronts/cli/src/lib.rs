//! The `nova` command's socket commands: each one talks to a running novad
//! and nothing else. The `nova` binary (`app/src/bin/nova`) parses the
//! command line and hands these their arguments; the commands that start,
//! install or inspect Nova from outside the socket (`nova` itself, `restart`,
//! `quit`, `setup`, `doctor`, `claude-statusline`, `daemon`) stay there.
//!
//! A front uses only the light framework crates and feature api crates
//! (rule W2 in scripts/check-layout.sh), never the app or a feature's core.

pub mod client;
pub mod ipc;
pub mod logs;
pub mod status;
pub mod toggle;
pub mod wallpaper;
