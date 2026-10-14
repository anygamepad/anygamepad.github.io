//! Reports as JSON text (feature `json`).
//!
//! The size limit, [`MAX_REPORT_BYTES`], applies to the compact form, which
//! is what a GitHub issue carries; the indented form, for files and
//! display, is about two and a half times as long.

use core::fmt;

use crate::{InvalidReport, MAX_REPORT_BYTES, Report};

/// Text [`Report::from_json`] reads at most, before parsing: room for an
/// indented report whose compact form fits.
const MAX_TEXT_BYTES: usize = 4 * MAX_REPORT_BYTES;

/// Error returned by [`Report::from_json`], [`Report::to_json`] and
/// [`Report::to_json_pretty`].
#[derive(Debug)]
pub enum ReportError {
    /// Larger than the limit: the compact form above [`MAX_REPORT_BYTES`]
    /// bytes, or a text four times as large.
    TooLarge(usize),
    /// Not JSON, or not the report format.
    Json(serde_json::Error),
    /// Well-formed but invalid (see [`Report::validate`]).
    Invalid(InvalidReport),
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(len) => write!(
                f,
                "device report of {len} bytes; at most {MAX_REPORT_BYTES} as compact JSON"
            ),
            Self::Json(error) => error.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ReportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TooLarge(_) => None,
            Self::Json(error) => Some(error),
            Self::Invalid(error) => Some(error),
        }
    }
}

impl Report {
    /// Parses and [validates](Self::validate) a JSON report, compact or
    /// indented, whose compact form has at most [`MAX_REPORT_BYTES`] bytes.
    /// Unknown fields are errors, so nothing the format does not know
    /// passes through.
    pub fn from_json(text: &str) -> Result<Self, ReportError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(ReportError::TooLarge(text.len()));
        }
        let report: Self = serde_json::from_str(text).map_err(ReportError::Json)?;
        report.validate().map_err(ReportError::Invalid)?;
        report.compact()?;
        Ok(report)
    }

    /// Validates the report and writes it as compact JSON, the form shared
    /// in GitHub issues; fails above [`MAX_REPORT_BYTES`].
    pub fn to_json(&self) -> Result<String, ReportError> {
        self.validate().map_err(ReportError::Invalid)?;
        self.compact()
    }

    /// Like [`to_json`](Self::to_json), indented, for files and display;
    /// ends with a newline.
    pub fn to_json_pretty(&self) -> Result<String, ReportError> {
        self.to_json()?;
        Ok(serde_json::to_string_pretty(self).map_err(ReportError::Json)? + "\n")
    }

    fn compact(&self) -> Result<String, ReportError> {
        let text = serde_json::to_string(self).map_err(ReportError::Json)?;
        if text.len() > MAX_REPORT_BYTES {
            return Err(ReportError::TooLarge(text.len()));
        }
        Ok(text)
    }
}
