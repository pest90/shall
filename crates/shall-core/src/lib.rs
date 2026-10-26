//! Core of `shall`: data model, parsers, link graph and tracer.
//!
//! This crate has no dependency on the command line or the report, so it
//! can be used as a library to build custom tools.

pub mod id;
pub mod item;

pub use id::{ItemId, ItemType, MAX_REVISION, ParseIdError};
pub use item::{Item, Location, LocationError, ParseStatusError, Reference, Status};
