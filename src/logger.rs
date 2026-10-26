//! The process's one logger: setting it up, the per-callsite check, and
//! writing a record.

use std::cell::RefCell;
use std::ffi::OsString;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering::Relaxed};
use std::sync::{OnceLock, PoisonError, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::format::{self, Record, Style};
use crate::{Error, Filter, Format, Level, Sink, Stderr, Value};

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// The most verbose level any target passes; 0, off, until a logger is
/// installed. The first check every record makes.
static MAX: AtomicU8 = AtomicU8::new(0);

/// Bumped each time the filter changes, so callsites ask again. It starts
/// at 1 so a callsite that has never asked (epoch 0) is always stale.
static EPOCH: AtomicU32 = AtomicU32::new(1);

pub(crate) struct Logger {
    filter: RwLock<Filter>,
    format: Format,
    style: Style,
    sink: Box<dyn Sink>,
}

impl Logger {
    fn enabled(&self, level: Level, target: &str) -> bool {
        self.filter
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .enabled(level, target)
    }

    fn log(&self, rec: &Record<'_>) {
        thread_local! {
            static BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let write = |buf: &mut Vec<u8>| {
            buf.clear();
            format::render(buf, self.format, self.style, now, rec);
            self.sink.write(buf);
        };
        // A value whose Display logs finds the buffer borrowed, and a
        // record logged while the thread exits finds it gone: the first
        // gets a buffer of its own, the second is dropped.
        let _ = BUF.try_with(|cell| match cell.try_borrow_mut() {
            Ok(mut buf) => write(&mut buf),
            Err(_) => write(&mut Vec::new()),
        });
    }
}

/// Whether one macro call site logs, remembered until the filter changes.
#[doc(hidden)]
pub struct Callsite {
    /// The epoch it last asked in, shifted left once, with the answer in
    /// the low bit.
    state: AtomicU32,
}

impl Callsite {
    #[expect(clippy::new_without_default, reason = "only ever a static in a macro")]
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(0),
        }
    }

    /// One load and a compare when `level` is above every target's level;
    /// two loads and a compare when the answer is remembered.
    #[inline]
    pub fn enabled(&self, level: Level, target: &'static str) -> bool {
        if level as u8 > MAX.load(Relaxed) {
            return false;
        }
        let state = self.state.load(Relaxed);
        let epoch = EPOCH.load(Relaxed);
        if state >> 1 == epoch {
            return state & 1 == 1;
        }
        self.ask(level, target, epoch)
    }

    #[cold]
    fn ask(&self, level: Level, target: &str, epoch: u32) -> bool {
        let on = LOGGER.get().is_some_and(|l| l.enabled(level, target));
        self.state.store(epoch << 1 | u32::from(on), Relaxed);
        on
    }
}

/// Writes one record through the installed logger. The macros call it
/// once their callsite says the record passes.
#[doc(hidden)]
pub fn emit(level: Level, target: &str, msg: &str, fields: &[(&str, &dyn Value)]) {
    if let Some(logger) = LOGGER.get() {
        logger.log(&Record {
            level,
            target,
            msg,
            fields,
        });
    }
}

/// A field's key as written: `r#type` becomes `type`.
#[doc(hidden)]
pub fn key(name: &'static str) -> &'static str {
    name.strip_prefix("r#").unwrap_or(name)
}

/// Replaces the installed logger's filter.
///
/// # Errors
///
/// [`Error::NotInstalled`] before [`Builder::install`].
pub fn set_filter(filter: Filter) -> Result<(), Error> {
    let logger = LOGGER.get().ok_or(Error::NotInstalled)?;
    let max = filter.max();
    *logger
        .filter
        .write()
        .unwrap_or_else(PoisonError::into_inner) = filter;
    EPOCH.fetch_add(1, Relaxed);
    MAX.store(max, Relaxed);
    Ok(())
}

/// Logs a panic as one `ERROR` record, target `panic`, with fields `code`
/// (`internal.panic`) and `location`, then runs the hook that was there
/// before, which prints it as usual.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("panic");
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()));
        crate::error!(target: "panic", msg; code = "internal.panic", location);
        previous(info);
    }));
}

/// Starts setting up the process's logger.
pub fn init() -> Builder {
    Builder::default()
}

/// Sets up the process's logger; made by [`init`].
///
/// The filter comes from the first of these that is set: [`level`]
/// (from the program's `-v` or `-q`), the variable named by [`env`], then
/// [`default_level`] (`Info` unless changed). Pass `level` only when the
/// user asked for a level, or the variable is never read. The format comes
/// from [`format`], then the variable named by [`format_env`], then text.
///
/// [`level`]: Builder::level
/// [`env`]: Builder::env
/// [`default_level`]: Builder::default_level
/// [`format`]: Builder::format
/// [`format_env`]: Builder::format_env
#[must_use = "a builder does nothing until `install`"]
pub struct Builder {
    env: Option<&'static str>,
    format_env: Option<&'static str>,
    level: Option<Level>,
    default_level: Level,
    format: Option<Format>,
    color: Option<bool>,
    sink: Option<Box<dyn Sink>>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            env: None,
            format_env: None,
            level: None,
            default_level: Level::Info,
            format: None,
            color: None,
            sink: None,
        }
    }
}

impl Builder {
    /// Reads filter directives from this variable: `APP_LOG=info,net=debug`.
    pub const fn env(mut self, var: &'static str) -> Self {
        self.env = Some(var);
        self
    }

    /// Reads the format, `text` or `json`, from this variable.
    pub const fn format_env(mut self, var: &'static str) -> Self {
        self.format_env = Some(var);
        self
    }

    /// Passes `level` and above for every target, over any variable.
    pub const fn level(mut self, level: Level) -> Self {
        self.level = Some(level);
        self
    }

    /// The level when neither [`Builder::level`] nor the variable is set.
    pub const fn default_level(mut self, level: Level) -> Self {
        self.default_level = level;
        self
    }

    /// Writes this format, over any variable.
    pub const fn format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }

    /// Colours text, or not. By default text is coloured when the sink is
    /// a terminal and `NO_COLOR` is unset or empty.
    pub const fn color(mut self, on: bool) -> Self {
        self.color = Some(on);
        self
    }

    /// Writes lines here; [`Stderr`] by default.
    pub fn sink(mut self, sink: impl Sink + 'static) -> Self {
        self.sink = Some(Box::new(sink));
        self
    }

    /// Installs the logger for the rest of the process.
    ///
    /// # Errors
    ///
    /// A variable that does not read, or [`Error::AlreadyInstalled`].
    pub fn install(self) -> Result<(), Error> {
        let logger = self.build(&|var| std::env::var_os(var))?;
        let max = logger
            .filter
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .max();
        LOGGER.set(logger).map_err(|_| Error::AlreadyInstalled)?;
        EPOCH.fetch_add(1, Relaxed);
        MAX.store(max, Relaxed);
        Ok(())
    }

    fn build(self, get: &dyn Fn(&str) -> Option<OsString>) -> Result<Logger, Error> {
        let read = |var: &'static str| -> Result<Option<String>, Error> {
            get(var)
                .map(|v| {
                    v.into_string()
                        .map_err(|_| Error::NotUnicode(var.to_owned()))
                })
                .transpose()
        };
        let in_env = |var: &'static str, e: Error| Error::Env {
            var: var.to_owned(),
            error: Box::new(e),
        };
        let filter = match (self.level, self.env) {
            (Some(level), _) => Filter::new(level),
            (None, Some(var)) => match read(var)? {
                Some(spec) => Filter::parse(&spec).map_err(|e| in_env(var, e))?,
                None => Filter::new(self.default_level),
            },
            (None, None) => Filter::new(self.default_level),
        };
        let format = match (self.format, self.format_env) {
            (Some(format), _) => format,
            (None, Some(var)) => match read(var)? {
                Some(name) => name.parse().map_err(|e| in_env(var, e))?,
                None => Format::Text,
            },
            (None, None) => Format::Text,
        };
        let sink = self.sink.unwrap_or_else(|| Box::new(Stderr));
        let terminal = format == Format::Text && sink.is_terminal();
        let no_color = get("NO_COLOR").is_some_and(|v| !v.is_empty());
        Ok(Logger {
            filter: RwLock::new(filter),
            format,
            style: Style {
                color: self.color.unwrap_or(terminal && !no_color),
                short_time: terminal,
            },
            sink,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Capture;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(*v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    /// A terminal that counts its writes.
    #[derive(Clone, Default)]
    struct Tty(Capture, std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl Sink for Tty {
        fn write(&self, line: &[u8]) {
            self.1.fetch_add(1, Relaxed);
            self.0.write(line);
        }
        fn is_terminal(&self) -> bool {
            true
        }
    }

    #[test]
    fn the_filter_comes_from_level_then_the_variable_then_the_default() {
        let get = env(&[("APP_LOG", "warn,net=trace")]);
        let l = init()
            .env("APP_LOG")
            .level(Level::Error)
            .build(&get)
            .unwrap();
        assert!(!l.enabled(Level::Warn, "net"));
        let l = init().env("APP_LOG").build(&get).unwrap();
        assert!(l.enabled(Level::Trace, "net"));
        assert!(!l.enabled(Level::Info, "x"));
        let l = init()
            .env("UNSET")
            .default_level(Level::Debug)
            .build(&get)
            .unwrap();
        assert!(l.enabled(Level::Debug, "x"));
        let l = init().default_level(Level::Error).build(&get).unwrap();
        assert!(!l.enabled(Level::Warn, "x"));
    }

    #[test]
    fn the_format_comes_from_format_then_the_variable_then_text() {
        let get = env(&[("APP_FMT", "json")]);
        let f = |b: Builder| b.build(&get).unwrap().format;
        assert_eq!(
            f(init().format_env("APP_FMT").format(Format::Text)),
            Format::Text
        );
        assert_eq!(f(init().format_env("APP_FMT")), Format::Json);
        assert_eq!(f(init().format_env("UNSET")), Format::Text);
        assert_eq!(f(init()), Format::Text);
    }

    #[test]
    fn bad_variables_are_errors() {
        let get = env(&[("APP_LOG", "infoo"), ("APP_FMT", "xml")]);
        let e = init().env("APP_LOG").build(&get).err().unwrap();
        assert_eq!(
            e.to_string(),
            r#"APP_LOG: filter directive "infoo": unknown level"#
        );
        let e = init().format_env("APP_FMT").build(&get).err().unwrap();
        assert_eq!(
            e.to_string(),
            r#"APP_FMT: unknown log format "xml", expected text or json"#
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_variable_that_is_not_utf8_is_an_error() {
        use std::os::unix::ffi::OsStringExt as _;
        let bad = |_: &str| Some(OsString::from_vec(vec![0xff]));
        let e = init().env("APP_LOG").build(&bad).err().unwrap();
        assert_eq!(e, Error::NotUnicode("APP_LOG".into()));
    }

    #[test]
    fn a_terminal_gets_short_coloured_text_unless_no_color() {
        let style = |pairs: &[(&str, &str)], b: Builder| {
            b.sink(Tty::default()).build(&env(pairs)).unwrap().style
        };
        assert_eq!(
            style(&[], init()),
            Style {
                color: true,
                short_time: true
            }
        );
        assert!(!style(&[("NO_COLOR", "1")], init()).color);
        assert!(style(&[("NO_COLOR", "")], init()).color);
        assert!(!style(&[], init().color(false)).color);
        assert_eq!(style(&[], init().format(Format::Json)), Style::default());
    }

    #[test]
    fn one_record_is_one_write() {
        let tty = Tty::default();
        let l = init()
            .sink(tty.clone())
            .color(false)
            .build(&env(&[]))
            .unwrap();
        let fields: [(&str, &dyn Value); 2] = [("a", &1u8), ("b", &"two words")];
        l.log(&Record {
            level: Level::Info,
            target: "t",
            msg: "m",
            fields: &fields,
        });
        assert_eq!(tty.1.load(Relaxed), 1);
        assert!(
            tty.0
                .contents()
                .ends_with(" INFO  t m a=1 b=\"two words\"\n")
        );
    }

    #[test]
    fn a_value_that_logs_gets_its_own_buffer() {
        struct Loud<'a>(&'a Logger);
        impl std::fmt::Display for Loud<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.log(&Record {
                    level: Level::Info,
                    target: "inner",
                    msg: "nested",
                    fields: &[],
                });
                f.write_str("outer")
            }
        }
        let c = Capture::new();
        let l = init().sink(c.clone()).build(&env(&[])).unwrap();
        let loud = Loud(&l);
        let fields: [(&str, &dyn Value); 1] = [("v", &crate::display(&loud))];
        l.log(&Record {
            level: Level::Info,
            target: "outer",
            msg: "m",
            fields: &fields,
        });
        let lines = c.lines();
        assert!(lines[0].ends_with("inner nested"), "{lines:?}");
        assert!(lines[1].ends_with("outer m v=outer"), "{lines:?}");
    }

    #[test]
    fn a_new_callsite_has_never_asked() {
        assert_eq!(Callsite::new().state.load(Relaxed), 0);
    }

    #[test]
    fn raw_keys_lose_their_prefix() {
        assert_eq!(key("r#type"), "type");
        assert_eq!(key("rows"), "rows");
    }
}
