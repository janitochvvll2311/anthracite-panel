//! The reverse proxy service: admin document, data plane and TLS.
//!
//! The service is a normal socket-class app. The engine knows its listeners and
//! egress from the manifest; the proxy knows hosts and routes from its own
//! document and never asks the engine for traffic policy.

pub mod admin;
pub mod config;
pub mod logs;
pub mod plane;
pub mod service;
pub mod tls;
