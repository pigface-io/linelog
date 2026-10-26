//! Structured logging, one line a record.
//!
//! A record has a level, a target (the module path unless given), a short
//! message and `key = value` fields. It is written as one line, in one of
//! two formats:
//!
//! - **text**, for people, in the style of logfmt:
//!   `10:00:00.123Z WARN  app::net retrying url=https://x/a attempt=2`;
//! - **JSON Lines**, for machines, one object a line:
//!   `{"ts":"2026-10-26T10:00:00.123456789Z","level":"WARN","target":"app::net","msg":"retrying","url":"https://x/a","attempt":2}`.
//!
//! The program sets the logger up once; libraries only log:
//!
//! ```
//! use linelog::{Level, debug, info, warn};
//!
//! linelog::init()
//!     .env("APP_LOG") // APP_LOG=info,app::net=debug
//!     .format_env("APP_LOG_FORMAT") // text or json
//!     .default_level(Level::Warn)
//!     .install()
//!     .unwrap();
//!
//! let (url, attempt, rows) = ("https://x/a", 2, 120_u64);
//! debug!("retrying"; url, attempt);
//! info!(target: "app.ingest", "file ingested"; rows, ms = 4.5);
//! warn!("row rejected"; code = "csv.field_count", line = 17);
//! ```
//!
//! A field written `rows` alone is `rows = rows`. Fields hold integers,
//! floats, `bool`, text, `Option` and `Duration`; wrap anything else in
//! [`display`] or [`debug`](fn@debug).
//!
//! A call that the filter turns away costs one atomic load and a compare,
//! and never evaluates its fields. The answer for each call site is kept
//! until the filter changes.
//!
//! There are no spans and no implicit context: pass context as fields.

mod error;
mod filter;
mod format;
mod level;
mod logger;
mod sink;
mod value;

pub use error::Error;
pub use filter::Filter;
pub use format::Format;
pub use level::{Level, ParseLevelError};
pub use logger::{Builder, init, install_panic_hook, set_filter};
pub use sink::{Capture, File, FnSink, Sink, Stderr, sink_fn};
pub use value::{DebugValue, DisplayValue, Kind, Value, debug, display};

#[doc(hidden)]
pub mod __private {
    pub use crate::logger::{Callsite, emit, key};
}

/// Logs a record at a level given at run time; the level macros are
/// shorter.
///
/// `log!(level, "msg"; key = value, key)` or
/// `log!(target: "app.net", level, "msg"; key = value)`.
#[macro_export]
macro_rules! log {
    (target: $target:expr, $level:expr, $msg:expr $(; $($key:ident $(= $value:expr)?),* $(,)?)?) => {{
        static CALLSITE: $crate::__private::Callsite = $crate::__private::Callsite::new();
        let level: $crate::Level = $level;
        if CALLSITE.enabled(level, $target) {
            $crate::__private::emit(level, $target, $msg, &[$($((
                $crate::__private::key(stringify!($key)),
                &$crate::__value!($key $(= $value)?) as &dyn $crate::Value,
            )),*)?]);
        }
    }};
    ($level:expr, $msg:expr $(; $($rest:tt)*)?) => {
        $crate::log!(target: module_path!(), $level, $msg $(; $($rest)*)?)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __value {
    ($key:ident) => {
        $key
    };
    ($key:ident = $value:expr) => {
        $value
    };
}

/// Whether a record at this level would be written: to skip work that only
/// a log line needs.
///
/// `enabled!(Level::Debug)` or `enabled!(target: "app.net", Level::Debug)`.
#[macro_export]
macro_rules! enabled {
    (target: $target:expr, $level:expr) => {{
        static CALLSITE: $crate::__private::Callsite = $crate::__private::Callsite::new();
        CALLSITE.enabled($level, $target)
    }};
    ($level:expr) => {
        $crate::enabled!(target: module_path!(), $level)
    };
}

/// Logs at [`Level::Error`]: the operation failed.
#[macro_export]
macro_rules! error {
    (target: $target:expr, $($rest:tt)+) => { $crate::log!(target: $target, $crate::Level::Error, $($rest)+) };
    ($($rest:tt)+) => { $crate::log!($crate::Level::Error, $($rest)+) };
}

/// Logs at [`Level::Warn`]: something degraded, and the program carried on.
#[macro_export]
macro_rules! warn {
    (target: $target:expr, $($rest:tt)+) => { $crate::log!(target: $target, $crate::Level::Warn, $($rest)+) };
    ($($rest:tt)+) => { $crate::log!($crate::Level::Warn, $($rest)+) };
}

/// Logs at [`Level::Info`]: a milestone.
#[macro_export]
macro_rules! info {
    (target: $target:expr, $($rest:tt)+) => { $crate::log!(target: $target, $crate::Level::Info, $($rest)+) };
    ($($rest:tt)+) => { $crate::log!($crate::Level::Info, $($rest)+) };
}

/// Logs at [`Level::Debug`]: a decision and why.
#[macro_export]
macro_rules! debug {
    (target: $target:expr, $($rest:tt)+) => { $crate::log!(target: $target, $crate::Level::Debug, $($rest)+) };
    ($($rest:tt)+) => { $crate::log!($crate::Level::Debug, $($rest)+) };
}

/// Logs at [`Level::Trace`]: fine detail.
#[macro_export]
macro_rules! trace {
    (target: $target:expr, $($rest:tt)+) => { $crate::log!(target: $target, $crate::Level::Trace, $($rest)+) };
    ($($rest:tt)+) => { $crate::log!($crate::Level::Trace, $($rest)+) };
}
