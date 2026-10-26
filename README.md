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
  through a progress bar), and an in-memory capture for tests.
- **Panics as records** with `install_panic_hook()`.
- No spans and no implicit context: pass context as fields. No bridge from
  the `log` or `tracing` facades.

[docs/format.md](docs/format.md) defines both formats, the mapping onto
OpenTelemetry's log model, and the level policy.

## Cost

`cargo bench` runs `benches/cost.rs`, which fails over its ceiling:

| case | Apple M-series |
|---|---|
| a call the filter turns away | 0.4 ns |
| a text record with four fields, formatted and handed to a sink | 220 ns |

## Licence

MIT or Apache-2.0, at your option.
