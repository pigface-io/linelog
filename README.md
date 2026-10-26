# linelog

Structured logging, one line a record: logfmt-style text for people, JSON
Lines for machines. No dependencies.

```rust
use linelog::{Level, debug, info, warn};

fn main() {
    linelog::init()
        .env("APP_LOG")               // APP_LOG=info,app::net=debug
        .format_env("APP_LOG_FORMAT") // text or json
        .default_level(Level::Warn)
        .install()
        .unwrap();

    let (url, attempt) = ("https://example.com/a.zip", 2);
    debug!("retrying"; url, attempt);
    info!(target: "app.ingest", "file ingested"; rows = 120_u64, ms = 4.5);
    warn!("row rejected"; code = "csv.field_count", line = 17);
}
```

```text
10:00:00.123Z WARN  app row rejected code=csv.field_count line=17
```

```json
{"ts":"2026-10-26T10:00:00.123456789Z","level":"WARN","target":"app","msg":"row rejected","code":"csv.field_count","line":17}
```

- **One line a record, one write a line.** Lines from many threads stay
  whole, and a log file is a table: DuckDB reads it with
  `read_json('app.log', format = 'newline_delimited')`.
- **Cheap when off.** A call the filter turns away costs one atomic load and
  a compare (0.4 ns on Apple silicon) and never evaluates its fields. Each
  call site remembers its answer until the filter changes.
- **Fields, not formatted strings.** `key = value`, or `key` alone for
  `key = key`. Integers, floats, `bool`, text, `Option` and `Duration` are
  written as themselves; `display(&x)` and `debug(&x)` wrap anything else.
- **Filters like `RUST_LOG`**, from a variable the program names, with a
  level per target. A misspelt level is an error, not a target.
- **Sinks:** standard error (default), an appended file, a closure (to print
  through a progress bar), an in-memory capture for tests, and
  `Background`, which writes another sink from a thread of its own.
- **Async-ready.** With `Background`, a record only copies its line into one
  of eight per-thread buffers; no worker thread waits on a file or a
  terminal. Lines keep the order they were logged in, even for a tokio task
  that moves between threads. Call `linelog::flush()` before exit.
- **No allocation per record.** Lines are formatted into a reused
  per-thread buffer; numbers, times and escapes are written by hand.
- **Panics as records** with `install_panic_hook()`.
- No spans and no implicit context: pass context as fields. No bridge from
  the `log` or `tracing` facades.

[docs/format.md](docs/format.md) defines both formats, the mapping onto
OpenTelemetry's log model, and the level policy.

## Cost

`just bench` runs `benches/cost.rs`, which fails over its ceiling. On an
Apple M2:

| case | time |
|---|---|
| a call the filter turns away | 0.35 ns |
| a text record with four fields, formatted and handed to a sink | 140 ns |

`just profile` times each stage of a record in zones of
[zonetrace](https://github.com/pigface-io/zonetrace) (feature `zones`, off by
default) and counts allocations: none per record. `just contend` logs from
1 to 8 threads and from 64 tokio tasks on 4 workers through each sink
(records a second in all, M2 with 4 performance cores; the machine was busy,
so take the spread as noise):

| sink | 1 thread | 4 threads | tokio, 4 workers |
|---|---|---|---|
| a sink that drops the line (linelog alone) | 6.2 M/s | 17.7 M/s | 17.7 M/s |
| `Stderr`, direct | 1.6 M/s | 0.8 M/s | 0.7 M/s |
| `File`, direct (`/dev/null`) | 1.4 M/s | 1.3 M/s | 1.6 M/s |
| `Background(Stderr)` | 4.8 M/s | 10 to 18 M/s | 15 to 17 M/s |
| `Background(File)` | 4.9 to 5.6 M/s | 7 to 14 M/s | 9 to 15 M/s |

A direct sink makes each record a write system call, and threads queue on
its lock; `Background` batches them on its own thread.

A call site adds about 70 bytes of code, plus about 15 a field; the library
itself about 40 KB (stripped, arm64).

## Licence

MIT or Apache-2.0, at your option.
