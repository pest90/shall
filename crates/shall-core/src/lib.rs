//! Core of `shall`: data model, parsers, link graph and tracer.
//!
//! This crate has no dependency on the command line or the report, so it
//! can be used as a library to build custom tools.

pub mod check;
pub mod diagnostic;
pub mod id;
pub mod item;
pub mod markdown;

pub use diagnostic::{Diagnostic, Severity};
pub use id::{ItemId, ItemType, MAX_REVISION, ParseIdError};
pub use item::{Item, Location, LocationError, ParseStatusError, Reference, Status};
