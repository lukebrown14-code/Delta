//! Shared domain core for the Delta Rust rewrite.
//!
//! This crate will hold config, domain models, IDs, SQL persistence and the
//! event system (R1a+). For now it only declares the module skeleton.

pub mod config;
pub mod db;
pub mod events;
pub mod ids;
pub mod json;
pub mod models;
pub mod state;
pub mod time;
