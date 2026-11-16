//! Core library for rupa: SQLite models, keys, peer config and commands.
//!
//! The binary is a thin CLI over these modules. A future web control plane can
//! depend on this crate without pulling in the CLI.

pub mod cli;
pub mod commands;
pub mod control;
pub mod db;
pub mod ip;
pub mod keys;
pub mod model;
pub mod peer_config;
pub mod util;
