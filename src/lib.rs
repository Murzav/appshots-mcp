pub mod error;
pub mod io;
pub mod model;
pub mod service;
// Tools not yet wired to server handler — suppress dead_code until server.rs integration.
#[allow(dead_code)]
pub mod tools;
