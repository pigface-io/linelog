//! What a log call costs: one the filter turns away, and one written as
//! text with four fields to a sink that only counts bytes. No harness: each
//! case is the median of seven rounds, timed by `Instant` around the round,
//! and a case over its ceiling fails the run.

use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use linelog::{Level, Sink, debug, info};

/// Counts bytes, so the bench measures formatting, not I/O.
struct Count(AtomicUsize);

impl Sink for Count {
    fn write(&self, line: &[u8]) {
        self.0.fetch_add(line.len(), Relaxed);
    }
}

const ROUNDS: usize = 7;

/// Nanoseconds a call of `f`, the median round of `calls` calls.
fn time(calls: u32, mut f: impl FnMut()) -> f64 {
    for _ in 0..calls / 10 {
        f();
    }
    let mut rounds: Vec<f64> = (0..ROUNDS)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..calls {
                f();
            }
            start.elapsed().as_secs_f64() * 1e9 / f64::from(calls)
        })
        .collect();
    rounds.sort_by(f64::total_cmp);
    rounds[ROUNDS / 2]
}

#[allow(clippy::expect_used, reason = "a bench fails loudly")]
fn main() {
    // About twice the cost, to catch a logger gone wrong, not a noisy
    // machine. A turned-away call is one load and a compare, about 0.4 ns
    // on Apple silicon; a written one reads the clock and formats about 150
    // bytes, about 220 ns there.
    let apple = cfg!(all(target_arch = "aarch64", target_os = "macos"));
    let (off_ceiling, on_ceiling) = if apple { (1.0, 500.0) } else { (5.0, 2000.0) };

    linelog::init()
        .level(Level::Info)
        .sink(Count(AtomicUsize::new(0)))
        .install()
        .expect("the bench installs the only logger");
    let url = "https://www.nemweb.com.au/Reports/Current/DispatchIS_Reports/";
    let results = [
        (
            "off",
            off_ceiling,
            time(
                100_000_000,
                || debug!("retrying"; url, attempt = black_box(2)),
            ),
        ),
        (
            "text_4_fields",
            on_ceiling,
            time(1_000_000, || {
                info!("file ingested"; url, rows = black_box(120_u64), bytes = 40_960_u64, ms = 4.5);
            }),
        ),
    ];
    for (name, _, ns) in &results {
        println!("log/{name:<16} {ns:>8.2} ns");
    }
    let over: Vec<_> = results.iter().filter(|(_, c, ns)| ns > c).collect();
    assert!(over.is_empty(), "over the ceiling: {over:?}");
}
