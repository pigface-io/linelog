//! Where finished lines go.

use std::fs;
use std::io::{self, IsTerminal as _, Write as _};
use std::path::Path;
use std::sync::atomic::{
    AtomicBool, AtomicU64, AtomicUsize,
    Ordering::{AcqRel, Acquire, Relaxed, Release},
};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

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

    /// Returns once every line written before the call has reached its
    /// destination. Only a sink that holds lines back needs it.
    fn flush(&self) {}
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

/// A file opened for appending, one `write_all` a line.
///
/// No lock: the file is opened with `O_APPEND`, so the kernel places each
/// write at the end, and threads write at once without waiting on each
/// other.
#[derive(Debug)]
pub struct File {
    file: fs::File,
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
        Ok(Self { file })
    }
}

impl Sink for File {
    fn write(&self, line: &[u8]) {
        let _ = (&self.file).write_all(line);
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

/// Another sink, written to from a thread of its own.
///
/// A record only copies its line into a buffer; the thread hands what has
/// gathered to the inner sink, many lines in one call. No thread that logs
/// waits on a write to a file or a terminal: what an async runtime's worker
/// threads need. Each thread that logs copies into one of eight buffers,
/// so threads rarely wait on each other either. Every line takes a number
/// in one sequence as it is copied, and the thread writes lines in that
/// order: a task that moves between threads keeps its lines in order.
///
/// Lines wait in memory until the thread writes them, so call
/// [`flush`](crate::flush) before the program exits; the panic hook does.
/// When more than `limit` bytes are waiting, a record waits for room, so a
/// sink that cannot keep up slows the program rather than losing lines.
pub struct Background {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

const SHARDS: usize = 8;

/// A batch this big means lines come fast: the thread drains again at
/// once instead of pausing to gather more.
const BIG: usize = 512 << 10;

struct Shared {
    shards: [Shard; SHARDS],
    /// Set by the thread before it sleeps until woken; a record that finds
    /// it set clears it and wakes the thread.
    asleep: AtomicBool,
    closed: AtomicBool,
    /// Flushes and full buffers wait here for the thread's next batch.
    lock: Mutex<()>,
    done: Condvar,
    /// The most bytes one buffer holds before its records wait.
    limit: usize,
}

/// One buffer, on a cache line of its own so threads copying into
/// different buffers never touch the same line.
#[repr(align(128))]
struct Shard {
    lines: Mutex<Lines>,
    /// Bytes of this buffer the thread has written, ever.
    written: AtomicU64,
}

/// A buffer's lines, each as an 8-byte sequence number, a 4-byte length
/// and the line, all little-endian; numbers rise through the buffer.
#[derive(Default)]
struct Lines {
    buf: Vec<u8>,
    /// Bytes ever copied into this buffer.
    queued: u64,
}

/// The sequence every line takes a number in, as it is copied.
static SEQ: AtomicU64 = AtomicU64::new(0);

const HEAD: usize = 12;

/// The sequence number and line at the start of `buf`.
fn entry(buf: &[u8]) -> (u64, &[u8]) {
    let mut seq = [0; 8];
    seq.copy_from_slice(&buf[..8]);
    let mut len = [0; 4];
    len.copy_from_slice(&buf[8..HEAD]);
    let len = u32::from_le_bytes(len) as usize;
    (u64::from_le_bytes(seq), &buf[HEAD..HEAD + len])
}

impl Background {
    /// Writes to `inner` from a new thread, holding at most 8 MiB.
    pub fn new(inner: impl Sink + 'static) -> Self {
        Self::with_limit(inner, 8 << 20)
    }

    /// Writes to `inner` from a new thread, holding at most about `limit`
    /// bytes.
    pub fn with_limit(inner: impl Sink + 'static, limit: usize) -> Self {
        let shared = Arc::new(Shared {
            shards: std::array::from_fn(|_| Shard {
                lines: Mutex::new(Lines::default()),
                written: AtomicU64::new(0),
            }),
            asleep: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            lock: Mutex::new(()),
            done: Condvar::new(),
            limit: (limit / SHARDS).max(1),
        });
        let theirs = Arc::clone(&shared);
        let thread = thread::spawn(move || theirs.run(&inner));
        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// Wakes the thread if it sleeps until woken. A busy thread wakes
    /// itself each millisecond, so a stream of records costs a load each.
    fn wake(&self) {
        let asleep = &self.shared.asleep;
        if asleep.load(Acquire) && asleep.swap(false, AcqRel) {
            self.unpark();
        }
    }

    fn unpark(&self) {
        if let Some(t) = &self.thread {
            t.thread().unpark();
        }
    }

    /// Waits until `reached` holds, nudging the thread to write meanwhile.
    fn wait(&self, reached: impl Fn() -> bool) {
        let shared = &self.shared;
        let mut guard = shared.lock.lock().unwrap_or_else(PoisonError::into_inner);
        while !reached() && !shared.closed.load(Acquire) {
            self.unpark();
            guard = shared
                .done
                .wait_timeout(guard, std::time::Duration::from_millis(10))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// The buffer this thread copies into: threads take them in turn.
fn shard() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static MINE: usize = NEXT.fetch_add(1, Relaxed) % SHARDS;
    }
    MINE.try_with(|m| *m).unwrap_or(0)
}

impl Shared {
    fn run(&self, inner: &dyn Sink) {
        let mut batch = Vec::new();
        let mut taken: [Vec<u8>; SHARDS] = Default::default();
        let mut ends = [0usize; SHARDS];
        loop {
            self.drain(&mut batch, &mut taken, &mut ends);
            if batch.is_empty() {
                if self.closed.load(Acquire) {
                    return;
                }
                // Sleep until woken, unless a record came in between the
                // drain and the flag. A flag left set while busy costs one
                // spare wake-up at most.
                self.asleep.store(true, Release);
                self.drain(&mut batch, &mut taken, &mut ends);
                if batch.is_empty() {
                    thread::park();
                }
                // Any lines the second drain found stay in the batch, and
                // the next drain adds to them.
                continue;
            }
            inner.write(&batch);
            inner.flush();
            let big = batch.len() >= BIG;
            batch.clear();
            for (shard, end) in self.shards.iter().zip(&mut ends) {
                shard.written.fetch_add(*end as u64, Release);
                *end = 0;
            }
            {
                let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
                self.done.notify_all();
            }
            // Let lines gather into the next batch, unless they come fast.
            if !big {
                thread::park_timeout(std::time::Duration::from_millis(1));
            }
        }
    }

    /// Moves every line numbered below the sequence's present value into
    /// `batch`, in sequence order, adding the bytes taken from each buffer
    /// to `ends`. Each buffer's numbers rise, so its share
    /// is a prefix; and a line numbered below the cutoff was copied under
    /// its buffer's lock before the drain takes that lock, so none is
    /// missed and later ones wait for the next batch.
    fn drain(
        &self,
        batch: &mut Vec<u8>,
        taken: &mut [Vec<u8>; SHARDS],
        ends: &mut [usize; SHARDS],
    ) {
        let cutoff = SEQ.load(Acquire);
        for ((shard, mine), taken_bytes) in self
            .shards
            .iter()
            .zip(taken.iter_mut())
            .zip(ends.iter_mut())
        {
            let mut lines = shard.lines.lock().unwrap_or_else(PoisonError::into_inner);
            let mut end = 0;
            while end < lines.buf.len() {
                let (seq, line) = entry(&lines.buf[end..]);
                if seq >= cutoff {
                    break;
                }
                end += HEAD + line.len();
            }
            if end == lines.buf.len() {
                std::mem::swap(&mut lines.buf, mine);
            } else {
                mine.extend_from_slice(&lines.buf[..end]);
                lines.buf.drain(..end);
            }
            drop(lines);
            *taken_bytes += end;
        }
        // Merge the buffers' runs by sequence number.
        let mut at = [0usize; SHARDS];
        loop {
            let mut next: Option<(u64, usize)> = None;
            for (i, mine) in taken.iter().enumerate() {
                if at[i] < mine.len() {
                    let (seq, _) = entry(&mine[at[i]..]);
                    if next.is_none_or(|(best, _)| seq < best) {
                        next = Some((seq, i));
                    }
                }
            }
            let Some((_, i)) = next else { break };
            let (_, line) = entry(&taken[i][at[i]..]);
            batch.extend_from_slice(line);
            at[i] += HEAD + line.len();
        }
        for mine in taken.iter_mut() {
            mine.clear();
        }
    }
}

impl Sink for Background {
    fn write(&self, line: &[u8]) {
        let shard = &self.shared.shards[shard()];
        let mut lines = shard.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.buf.len() >= self.shared.limit {
            drop(lines);
            self.wait(|| {
                shard
                    .lines
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .buf
                    .len()
                    < self.shared.limit
            });
            lines = shard.lines.lock().unwrap_or_else(PoisonError::into_inner);
        }
        let seq = SEQ.fetch_add(1, AcqRel);
        let len = u32::try_from(line.len()).unwrap_or(u32::MAX);
        let line = &line[..len as usize];
        lines.buf.extend_from_slice(&seq.to_le_bytes());
        lines.buf.extend_from_slice(&len.to_le_bytes());
        lines.buf.extend_from_slice(line);
        lines.queued += (HEAD + line.len()) as u64;
        drop(lines);
        self.wake();
    }

    fn flush(&self) {
        let targets: [u64; SHARDS] = std::array::from_fn(|i| {
            self.shared.shards[i]
                .lines
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .queued
        });
        self.wait(|| {
            self.shared
                .shards
                .iter()
                .zip(targets)
                .all(|(shard, target)| shard.written.load(Acquire) >= target)
        });
    }
}

impl Drop for Background {
    /// Writes what is waiting, then ends the thread.
    fn drop(&mut self) {
        self.shared.closed.store(true, Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
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
    fn background_writes_every_line_in_order_per_thread() {
        let c = Capture::new();
        let bg = Background::new(c.clone());
        thread::scope(|scope| {
            for t in 0..12 {
                let bg = &bg;
                scope.spawn(move || {
                    for i in 0..2000 {
                        bg.write(format!("{t} {i}\n").as_bytes());
                    }
                });
            }
        });
        bg.flush();
        let lines = c.lines();
        assert_eq!(lines.len(), 24_000);
        for t in 0..12 {
            let mine: Vec<u32> = lines
                .iter()
                .filter_map(|l| l.strip_prefix(&format!("{t} ")))
                .map(|i| i.parse().unwrap())
                .collect();
            assert_eq!(mine, (0..2000).collect::<Vec<_>>());
        }
    }

    #[test]
    fn background_wakes_from_sleep_and_drains_on_drop() {
        let c = Capture::new();
        let bg = Background::new(c.clone());
        bg.write(b"one\n");
        bg.flush();
        assert_eq!(c.contents(), "one\n");
        // Long enough for the thread to go to sleep until woken.
        thread::sleep(std::time::Duration::from_millis(20));
        bg.write(b"two\n");
        bg.flush();
        assert_eq!(c.contents(), "one\ntwo\n");
        bg.write(b"three\n");
        drop(bg);
        assert_eq!(c.lines(), ["one", "two", "three"]);
    }

    #[test]
    fn a_full_background_makes_records_wait() {
        /// A slow sink, so lines pile up past the limit.
        struct Slow(Capture);
        impl Sink for Slow {
            fn write(&self, line: &[u8]) {
                thread::sleep(std::time::Duration::from_millis(2));
                self.0.write(line);
            }
        }
        let c = Capture::new();
        let bg = Background::with_limit(Slow(c.clone()), 64);
        for i in 0..50 {
            bg.write(format!("line {i:02}\n").as_bytes());
        }
        bg.flush();
        assert_eq!(c.lines().len(), 50);
        assert_eq!(c.lines()[49], "line 49");
    }

    #[test]
    fn flush_after_close_returns() {
        let bg = Background::new(Capture::new());
        bg.shared.closed.store(true, Release);
        bg.shared.shards[0].lines.lock().unwrap().queued = 10;
        bg.flush();
    }

    #[test]
    fn stderr_takes_a_line() {
        Stderr.write(b"");
        let _ = Stderr.is_terminal();
    }
}
