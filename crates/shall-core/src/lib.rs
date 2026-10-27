//! Core of `shall`: data model, parsers, link graph and tracer.
//!
//! This crate has no dependency on the command line or the report, so it
//! can be used as a library to build custom tools.

pub mod check;
pub mod config;
pub mod create;
pub mod diagnostic;
pub mod export;
pub mod id;
pub mod item;
pub mod junit;
pub mod markdown;
pub mod results;
pub mod tags;
pub mod trace;
pub mod verify;

pub use diagnostic::{Diagnostic, Severity};
pub use id::{ItemId, ItemType, MAX_REVISION, ParseIdError};
pub use item::{
    Item, Location, LocationError, Origin, ParseStatusError, Reference, Status, TestKind, TestName,
};
