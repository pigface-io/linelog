//! The installed logger, through the macros: filtering, both formats'
//! fields, a filter changed at run time, threads, and the panic hook. One
//! test, because a process has one logger.

use std::thread;

use linelog::{Capture, Error, Filter, Level, Value, enabled, error, info, log, trace, warn};

#[test]
fn the_installed_logger() {
    assert_eq!(linelog::set_filter(Filter::off()), Err(Error::NotInstalled));
    // Before install nothing passes.
    assert!(!enabled!(Level::Error));

    let lines = Capture::new();
    linelog::init()
        .env("LINELOG_TEST_UNSET")
        .format_env("LINELOG_TEST_UNSET_FORMAT")
        .default_level(Level::Info)
        .sink(lines.clone())
        .install()
        .unwrap();
    assert_eq!(
        linelog::init().sink(Capture::new()).install(),
        Err(Error::AlreadyInstalled)
    );

    let rows = 120_u64;
    let r#type = "csv";
    info!("file ingested"; rows, r#type, ms = 4.5,);
    linelog::debug!("not shown"; rows);
    warn!(target: "app.net", "retrying"; attempt = 2);
    error!(target: "app.net", "gave up");
    log!(Level::Info, "no fields");
    let got = lines.lines();
    assert_eq!(got.len(), 4, "{got:?}");
    assert!(
        got[0].ends_with(" INFO  global file ingested rows=120 type=csv ms=4.5"),
        "{got:?}"
    );
    assert!(got[1].ends_with(" WARN  app.net retrying attempt=2"));
    assert!(got[2].ends_with(" ERROR app.net gave up"));
    assert!(got[3].ends_with(" INFO  global no fields"));
    assert!(enabled!(Level::Info));
    assert!(!enabled!(target: "app.net", Level::Debug));

    // A changed filter reaches call sites that already answered.
    let site = |lines: &Capture| {
        lines.clear();
        trace!(target: "app.net.fetch", "detail");
        lines.lines().len()
    };
    assert_eq!(site(&lines), 0);
    linelog::set_filter(Filter::new(Level::Warn).with("app.net", Level::Trace)).unwrap();
    assert_eq!(site(&lines), 1);
    linelog::set_filter(Filter::new(Level::Warn)).unwrap();
    assert_eq!(site(&lines), 0);
    linelog::set_filter(Filter::new(Level::Info)).unwrap();

    // Lines from many threads stay whole.
    lines.clear();
    let threads: Vec<_> = (0..8_u64)
        .map(|t| {
            thread::spawn(move || {
                for i in 0..500_u64 {
                    info!("tick"; t, i, pad = "x".repeat(64));
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let got = lines.lines();
    assert_eq!(got.len(), 4000);
    assert!(
        got.iter()
            .all(|l| l.contains(" INFO  global tick t=") && l.ends_with(&"x".repeat(64)))
    );

    // A panic is logged before the usual hook prints it.
    lines.clear();
    linelog::install_panic_hook();
    let _ = std::panic::catch_unwind(|| panic!("boom {}", 1));
    let _ = std::panic::catch_unwind(|| std::panic::panic_any("static"));
    let _ = std::panic::catch_unwind(|| std::panic::panic_any(7_u8));
    let got = lines.lines();
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(got[0].contains(" ERROR panic boom 1 code=internal.panic location=tests/global.rs:"));
    assert!(got[1].contains(" ERROR panic static code="));
    assert!(got[2].contains(" ERROR panic panic code="));

    // A value is any Value, borrowed.
    let v: &dyn Value = &3_u8;
    info!("dyn"; v);
}
