//! Where finished lines go.

use std::fs;
use std::io::{self, IsTerminal as _, Write as _};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// Where finished lines go.
///
/// `write` gets one whole line, newline included, once per record. A sink
/// that writes it with one call keeps lines from different threads whole.
/// Logging never fails the program: a sink drops a line it cannot write.
pub trait Sink: Send + Sync {
    /// Writes one line.
    fn write(&self, line: &[u8]);

    /// Whether the sink is a terminal, so text may be coloured and short.
    fn is_terminal(&self) -> bool {
        false
    }
}

/// Standard error, one `write_all` a line under its lock. The default sink.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stderr;

impl Sink for Stderr {
    fn write(&self, line: &[u8]) {
        let _ = io::stderr().lock().write_all(line);
    }

    fn is_terminal(&self) -> bool {
        io::stderr().is_terminal()
    }
}

/// A file opened for appending, one `write_all` a line under a lock.
#[derive(Debug)]
pub struct File {
    file: Mutex<fs::File>,
}

impl File {
    /// Opens `path` for appending, creating it when missing.
    ///
    /// # Errors
    ///
    /// The I/O error from opening the file.
    pub fn append(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl Sink for File {
    fn write(&self, line: &[u8]) {
        let _ = self
            .file
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_all(line);
    }
}

/// Lines kept in memory, for tests. Clones share the same lines.
#[derive(Clone, Debug, Default)]
pub struct Capture {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl Capture {
    /// An empty capture.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every line written so far, newlines included.
    pub fn contents(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned()
    }

    /// Every line written so far, without newlines.
    pub fn lines(&self) -> Vec<String> {
        self.contents().lines().map(str::to_owned).collect()
    }

    /// Forgets every line written so far.
    pub fn clear(&self) {
        self.buf
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

impl Sink for Capture {
    fn write(&self, line: &[u8]) {
        self.buf
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(line);
    }
}

/// A closure as a sink; made by [`sink_fn`].
pub struct FnSink<F>(F);

impl<F: Fn(&[u8]) + Send + Sync> Sink for FnSink<F> {
    fn write(&self, line: &[u8]) {
        (self.0)(line);
    }
}

/// A closure as a sink: to print through a progress bar's own `println`,
/// so lines never tear it.
pub const fn sink_fn<F: Fn(&[u8]) + Send + Sync>(f: F) -> FnSink<F> {
    FnSink(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_keeps_lines_until_cleared() {
        let c = Capture::new();
        let shared = c.clone();
        shared.write(b"a\n");
        shared.write(b"b\n");
        assert_eq!(c.lines(), ["a", "b"]);
        assert!(!c.is_terminal());
        c.clear();
        assert_eq!(c.contents(), "");
    }

    #[test]
    fn a_file_is_appended_to() {
        let path = std::env::temp_dir().join(format!("linelog-{}.log", std::process::id()));
        let _ = fs::remove_file(&path);
        File::append(&path).unwrap().write(b"one\n");
        let f = File::append(&path).unwrap();
        f.write(b"two\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        fs::remove_file(&path).unwrap();
        assert!(File::append(path.join("not-a-dir")).is_err());
    }

    #[test]
    fn a_closure_gets_each_line() {
        let c = Capture::new();
        let inner = c.clone();
        sink_fn(move |line: &[u8]| inner.write(line)).write(b"x\n");
        assert_eq!(c.contents(), "x\n");
    }

    #[test]
    fn stderr_takes_a_line() {
        Stderr.write(b"");
        let _ = Stderr.is_terminal();
    }
}
