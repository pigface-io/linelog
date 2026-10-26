//! Severity levels, from `Error`, the most severe, to `Trace`.

use std::fmt;
use std::str::FromStr;

/// How severe a record is.
///
/// Ordered by verbosity: `Error < Warn < Info < Debug < Trace`, so a filter
/// at `Info` lets through every level less than or equal to `Info`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Level {
    /// The operation failed.
    Error = 1,
    /// Something degraded, and the program carried on.
    Warn = 2,
    /// A milestone: a run, a file or a round began or ended.
    Info = 3,
    /// A decision and why it was taken.
    Debug = 4,
    /// Fine detail, per block or entry.
    Trace = 5,
}

impl Level {
    /// Every level, most severe first.
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    /// The level's name in capitals, as both formats write it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }

    /// The ANSI colour the text format gives the level on a terminal.
    pub(crate) const fn color(self) -> &'static str {
        match self {
            Self::Error => "\x1b[31m",
            Self::Warn => "\x1b[33m",
            Self::Info => "\x1b[32m",
            Self::Debug => "\x1b[34m",
            Self::Trace => "\x1b[35m",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A name that is not a level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseLevelError(pub String);

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown level {:?}, expected error, warn, info, debug or trace",
            self.0
        )
    }
}

impl std::error::Error for ParseLevelError {}

impl FromStr for Level {
    type Err = ParseLevelError;

    /// Reads a level's name in any case: `warn`, `WARN` or `Warn`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|l| l.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| ParseLevelError(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_in_any_case() {
        for l in Level::ALL {
            assert_eq!(l.as_str().parse::<Level>(), Ok(l));
            assert_eq!(l.as_str().to_lowercase().parse::<Level>(), Ok(l));
            assert_eq!(l.to_string(), l.as_str());
            assert!(l.color().starts_with("\x1b["));
        }
    }

    #[test]
    fn ordered_by_verbosity() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Debug < Level::Trace);
    }

    #[test]
    fn a_bad_name_says_what_is_expected() {
        let e = "verbose".parse::<Level>().unwrap_err();
        assert_eq!(
            e.to_string(),
            r#"unknown level "verbose", expected error, warn, info, debug or trace"#
        );
    }
}
