//! Diagnostics: problems found while reading and tracing a project.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::item::Location;

/// A problem at a position in a source file, displayed as
/// `path:line: message` so that editors and terminals can link to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Where the problem is.
    pub location: Location,
    /// How severe the problem is.
    pub severity: Severity,
    /// Description of the problem, without location and severity.
    pub message: String,
}

impl Diagnostic {
    /// Creates an error.
    pub fn error(location: Location, message: impl Into<String>) -> Self {
        Self {
            location,
            severity: Severity::Error,
            message: message.into(),
        }
    }

    /// Creates a warning.
    pub fn warning(location: Location, message: impl Into<String>) -> Self {
        Self {
            location,
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Severity of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The input is wrong; results that depend on it are not reliable.
    Error,
    /// The input is suspicious but can be processed.
    Warning,
}
