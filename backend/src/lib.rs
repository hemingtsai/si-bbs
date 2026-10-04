//! SI BBS backend library crate.
//!
//! `main.rs` is a thin binary entrypoint; everything testable lives here so
//! integration tests can build the same router the binary serves.

pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod services;

pub use routes::create_router;