//! A terminal sink gets short, coloured text.

use linelog::{Capture, Format, Sink, info};

#[derive(Clone, Default)]
struct Tty(Capture);

impl Sink for Tty {
    fn write(&self, line: &[u8]) {
        self.0.write(line);
    }
    fn is_terminal(&self) -> bool {
        true
    }
}

#[test]
fn short_coloured_text() {
    assert_eq!("text".parse::<Format>(), Ok(Format::Text));
    let tty = Tty::default();
    linelog::init()
        .env("LINELOG_TEST_UNSET")
        .sink(tty.clone())
        .install()
        .unwrap();
    info!(target: "app", "done");
    let line = tty.0.contents();
    assert_eq!(
        line.len(),
        "10:00:00.123Z ".len() + "\x1b[32mINFO\x1b[0m  \x1b[2mapp\x1b[0m done\n".len()
    );
    assert!(
        line.ends_with("Z \x1b[32mINFO\x1b[0m  \x1b[2mapp\x1b[0m done\n"),
        "{line:?}"
    );
}
