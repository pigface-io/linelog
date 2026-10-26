//! The JSON format from the installed logger.

use linelog::{Capture, Format, Level, info};

#[test]
fn one_object_a_line() {
    let lines = Capture::new();
    linelog::init()
        .format(Format::Json)
        .level(Level::Debug)
        .sink(lines.clone())
        .install()
        .unwrap();
    info!(target: "app", "done"; rows = 3_u64, ok = true, path = linelog::display(&"a\"b"));
    let line = &lines.lines()[0];
    assert!(line.starts_with(r#"{"ts":""#), "{line}");
    assert!(
        line.ends_with(
            r#"Z","level":"INFO","target":"app","msg":"done","rows":3,"ok":true,"path":"a\"b"}"#
        ),
        "{line}"
    );
}
