//! Library code for the sshx command-line client application.
//!
//! This crate does not forbid use of unsafe code because it needs to interact
//! with operating-system APIs to access pseudoterminal (PTY) devices.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod agent;
pub mod analyze;
pub mod claude_hooks;
pub mod controller;
pub mod encrypt;
pub mod ide;
pub mod openvscode;
pub mod platform;
pub mod runner;
pub mod terminal;
pub mod tunnel;
pub mod update;
pub mod workspace;
