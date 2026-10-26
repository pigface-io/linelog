//! Where a record's time goes, in zones of zonetrace, and what it
//! allocates: `cargo run --release --features zones --example profile`.
//!
//! Each shape of record runs on one thread, then the busiest shape on
//! several threads at once into one shared sink. The sink only counts
//! bytes, so the zones time linelog, not I/O.

// A measuring tool: a failure should stop it loudly.
#![allow(clippy::unwrap_used)]
// A counting allocator is the one unsafe thing in the repository.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::thread;
use std::time::{Duration, Instant};

use linelog::{Format, Level, Sink, debug, display, info};

/// Counts allocations, to show a record allocates nothing.
struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

struct Count(AtomicUsize);

impl Sink for Count {
    fn write(&self, line: &[u8]) {
        self.0.fetch_add(line.len(), Relaxed);
    }
}

const N: u32 = 1_000_000;

/// Runs `f` `N` times in each of five rounds; returns the median round's
/// ns a call, and allocations a call over every round.
fn run(f: impl Fn()) -> (f64, f64) {
    for _ in 0..1000 {
        f();
    }
    let before = ALLOCS.load(Relaxed);
    let mut rounds: Vec<f64> = (0..5)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..N {
                f();
            }
            start.elapsed().as_secs_f64() * 1e9 / f64::from(N)
        })
        .collect();
    rounds.sort_by(f64::total_cmp);
    let allocs = (ALLOCS.load(Relaxed) - before) as f64 / f64::from(5 * N);
    (rounds[2], allocs)
}

fn main() {
    let json = std::env::args().any(|a| a == "json");
    let format = if json { Format::Json } else { Format::Text };
    linelog::init()
        .level(Level::Info)
        .format(format)
        .sink(Count(AtomicUsize::new(0)))
        .install()
        .unwrap();
    let url = "https://www.nemweb.com.au/Reports/Current/DispatchIS_Reports/";
    let path = std::path::Path::new("/data/archive/PUBLIC_DISPATCHIS_202610261005.zip");
    let shapes: [(&str, &dyn Fn()); 6] = [
        ("off", &|| debug!("retrying"; url, attempt = black_box(2))),
        ("no fields", &|| info!("round done")),
        (
            "4 ints",
            &|| info!("file ingested"; rows = black_box(120_u64), bytes = 40_960_u64, files = 3_u32, ms = 4_u64),
        ),
        (
            "4 mixed",
            &|| info!("file ingested"; url, rows = black_box(120_u64), ok = true, ms = 4.5),
        ),
        (
            "display",
            &|| info!("file archived"; path = display(&path.display()), took = Duration::from_micros(1500)),
        ),
        (
            "quoted",
            &|| info!("row rejected"; sample = "a b=c \"d\"", code = "csv.field_count"),
        ),
    ];
    let _ = zonetrace::take();
    println!("format {format:?}, {N} calls each, one thread:");
    for (name, f) in shapes {
        let (ns, allocs) = run(f);
        println!("  {name:<10} {ns:>8.1} ns  {allocs:.3} allocs/record");
    }
    let overhead = zonetrace::overhead();
    println!("\nzones over all shapes (overhead {overhead:.1} ns a zone subtracted):");
    print!("{}", zonetrace::take().text(overhead));

    for threads in [1, 2, 4, 8] {
        let start = Instant::now();
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                thread::spawn(move || {
                    for i in 0..N / 4 {
                        info!("file ingested"; url, rows = black_box(u64::from(i)), ok = true, ms = 4.5);
                    }
                    zonetrace::flush();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let secs = start.elapsed().as_secs_f64();
        let total = f64::from(threads * (N / 4));
        println!(
            "\n{threads} threads: {:.2} M records/s in all, {:.1} ns a record a thread",
            total / secs / 1e6,
            secs * 1e9 * f64::from(threads) / total
        );
    }
    println!("\nzones over the threaded runs:");
    print!("{}", zonetrace::take_all().text(overhead));
}
