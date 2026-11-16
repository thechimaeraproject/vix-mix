//! The control from outside: how another program reads, changes and follows the console.
//!
//! A request is one line, `verb address [value]` ([`parse`]); an address names a value of the
//! setup ([`resolve`], [`leaves`]); [`plan`] says what the request asks of the mixer: values
//! to print, commands to apply, or addresses to follow. Every command a request can send is
//! one the window's switches and faders send, so nothing here changes what the mixer does,
//! only who may ask.
//!
//! **Data in, data out.** This crate depends on the model and on nothing else of the
//! workspace: it holds no host, no window and no audio. What applies a command is behind a
//! two-method seam the app implements.
//!
//! See `docs/reference/control.md`.

pub mod address;
mod request;
mod service;
mod tree;
mod value;

pub use address::resolve;
pub use request::{Plan, Request, TOP_DB, Verb, parse, plan};
pub use service::{
    Client, ClientId, Connector, Full, INBOX, MAX_CLIENTS, MAX_LINE, MAX_WATCHES, OUTBOX, Requests,
    SERVE_BUDGET, Served, Service, Target, service,
};
pub use tree::{Change, GONE, Kind, Leaf, diff, leaves};
pub use value::{SILENT, Value};

/// The protocol's number, which the mixer says first on every connection. It moves when an
/// address is renamed or a line changes its meaning; a new address or a new verb keeps it.
pub const PROTOCOL: u32 = 1;

/// The first line the mixer says on a connection: who answers, and the protocol's number.
pub fn greeting() -> String {
    format!("vix-mix control {PROTOCOL}")
}

/// The line that ends a request's answer when it was carried out.
pub const OK: &str = "ok";

/// What begins the line that ends a request's answer when it was not; the message follows,
/// and ends with what to send instead.
pub const ERROR: &str = "error: ";

/// What begins a line that tells a follower of a change; an answer's lines never begin so.
pub const EVENT: &str = "event ";
