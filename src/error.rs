//! What can go wrong setting up logging.

use std::fmt;

/// What can go wrong setting up logging. Logging itself never fails.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A filter directive does not read.
    Directive {
        /// The directive as written.
        directive: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A format name is neither `text` nor `json`.
    Format(String),
    /// An environment variable holds a value that does not read.
    Env {
        /// The variable's name.
        var: String,
        /// What is wrong with its value.
        error: Box<Error>,
    },
    /// An environment variable is not valid UTF-8.
    NotUnicode(String),
    /// A logger is already installed in this process.
    AlreadyInstalled,
    /// No logger is installed yet.
    NotInstalled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directive { directive, reason } => {
                write!(f, "filter directive {directive:?}: {reason}")
            }
            Self::Format(name) => write!(f, "unknown log format {name:?}, expected text or json"),
            Self::Env { var, error } => write!(f, "{var}: {error}"),
            Self::NotUnicode(var) => write!(f, "{var} is not valid UTF-8"),
            Self::AlreadyInstalled => f.write_str("a logger is already installed"),
            Self::NotInstalled => f.write_str("no logger is installed"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_lowercase_without_a_period() {
        let cases = [
            (
                Error::Directive {
                    directive: "a=loud".into(),
                    reason: "unknown level",
                },
                r#"filter directive "a=loud": unknown level"#,
            ),
            (
                Error::Format("xml".into()),
                r#"unknown log format "xml", expected text or json"#,
            ),
            (
                Error::Env {
                    var: "APP_LOG_FORMAT".into(),
                    error: Box::new(Error::Format("xml".into())),
                },
                r#"APP_LOG_FORMAT: unknown log format "xml", expected text or json"#,
            ),
            (
                Error::NotUnicode("APP_LOG".into()),
                "APP_LOG is not valid UTF-8",
            ),
            (Error::AlreadyInstalled, "a logger is already installed"),
            (Error::NotInstalled, "no logger is installed"),
        ];
        for (e, text) in cases {
            assert_eq!(e.to_string(), text);
        }
    }
}
