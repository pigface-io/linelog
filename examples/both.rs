//! Writes a few records in the format named by the first argument, `text`
//! (the default) or `json`, to standard error:
//!
//! ```text
//! cargo run --example both -- json 2> app.log
//! duckdb -c "select level, msg, rows from read_json('app.log', format = 'newline_delimited')"
//! ```

use linelog::{Format, Level, debug, info, warn};

fn main() -> Result<(), linelog::Error> {
    let format = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("text")
        .parse::<Format>()?;
    linelog::init()
        .env("EXAMPLE_LOG")
        .default_level(Level::Debug)
        .format(format)
        .install()?;
    let url = "https://example.com/reports/a b.zip";
    debug!(target: "app.net", "fetching"; url, attempt = 1);
    info!(target: "app.ingest", "file ingested"; rows = 120_u64, bytes = 40_960_u64, ms = 4.5);
    warn!(target: "app.ingest", "row rejected"; code = "csv.field_count", line = 17, expected = 12, observed = 11);
    Ok(())
}
