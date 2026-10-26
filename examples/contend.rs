//! Records from many threads at once, and from tokio's worker threads:
//! `cargo run --release --example contend 2>/dev/null`.
//!
//! For each sink, the same total of records is split over 1, 2, 4 and 8
//! threads; the table shows records a second in all and the time a record
//! takes on its thread. A sink that only drops the line shows what
//! linelog itself costs; the others add the lock each sink takes.

// A measuring tool: a failure should stop it loudly.
#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;
use std::time::Instant;

use linelog::{Background, Capture, File, Filter, Level, Sink, Stderr, info};

/// The sinks under test, and which one the logger writes to now: read
/// with one atomic load, so choosing costs the records nothing shared.
static SINKS: OnceLock<Vec<(&str, Box<dyn Sink>)>> = OnceLock::new();
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Drops each line.
struct Drop;

impl Sink for Drop {
    fn write(&self, line: &[u8]) {
        black_box(line);
    }
}

/// Counts bytes in one shared atomic.
struct Count(AtomicUsize);

impl Sink for Count {
    fn write(&self, line: &[u8]) {
        self.0.fetch_add(line.len(), Relaxed);
    }
}

const TOTAL: u32 = 2_000_000;

fn record(i: u32) {
    let url = "https://www.nemweb.com.au/Reports/Current/DispatchIS_Reports/";
    info!("file ingested"; url, rows = u64::from(i), ok = true, ms = 4.5);
}

fn threads(n: u32) -> f64 {
    let start = Instant::now();
    let handles: Vec<_> = (0..n)
        .map(|_| {
            thread::spawn(move || {
                for i in 0..TOTAL / n {
                    record(i);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    start.elapsed().as_secs_f64()
}

fn tokio(workers: usize, tasks: u32) -> f64 {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .build()
        .unwrap();
    let start = Instant::now();
    rt.block_on(async {
        let handles: Vec<_> = (0..tasks)
            .map(|_| {
                tokio::spawn(async move {
                    for i in 0..TOTAL / tasks {
                        record(i);
                        if i % 64 == 0 {
                            tokio::task::yield_now().await;
                        }
                    }
                })
            })
            .collect();
        for h in handles {
            h.await.unwrap();
        }
    });
    start.elapsed().as_secs_f64()
}

fn row(name: &str, secs: f64, per_thread: u32) {
    println!(
        "  {name:<22} {:>6.2} M/s  {:>7.1} ns a record a thread",
        f64::from(TOTAL) / secs / 1e6,
        secs * 1e9 * f64::from(per_thread) / f64::from(TOTAL)
    );
}

fn main() {
    let dev_null = || File::append("/dev/null").unwrap();
    let sinks: Vec<(&str, Box<dyn Sink>)> = vec![
        ("drop", Box::new(Drop)),
        ("one atomic", Box::new(Count(AtomicUsize::new(0)))),
        ("capture", Box::new(Capture::new())),
        ("file /dev/null", Box::new(dev_null())),
        ("stderr", Box::new(Stderr)),
        ("background(file)", Box::new(Background::new(dev_null()))),
        ("background(stderr)", Box::new(Background::new(Stderr))),
    ];
    let count = sinks.len();
    let _ = SINKS.set(sinks);
    linelog::init()
        .level(Level::Info)
        .color(false)
        .sink(linelog::sink_fn(|line: &[u8]| {
            if let Some((_, s)) = SINKS.get().and_then(|s| s.get(CURRENT.load(Relaxed))) {
                s.write(line);
            }
        }))
        .install()
        .unwrap();
    for i in 0..count {
        CURRENT.store(i, Relaxed);
        let (name, sink) = &SINKS.get().unwrap()[i];
        println!("{name}:");
        for n in [1, 2, 4, 8] {
            let secs = threads(n);
            sink.flush();
            row(&format!("{n} threads"), secs, n);
        }
        let secs = tokio(4, 64);
        sink.flush();
        row("tokio 4 workers", secs, 4);
    }
    // The filter changing under load: every call site asks again, and no
    // record is torn or lost while it does.
    CURRENT.store(0, Relaxed);
    let flip = thread::spawn(|| {
        for i in 0..1000 {
            let level = if i % 2 == 0 { Level::Info } else { Level::Warn };
            linelog::set_filter(Filter::new(level)).unwrap();
        }
        linelog::set_filter(Filter::new(Level::Info)).unwrap();
    });
    threads(4);
    flip.join().unwrap();
    println!("filter changed 1000 times under 4 threads: ok");
}
