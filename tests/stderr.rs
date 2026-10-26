//! The default sink, standard error, with no variables set.

use linelog::{Level, info};

#[test]
fn writes_to_stderr_by_default() {
    linelog::init().env("LINELOG_TEST_UNSET").install().unwrap();
    assert!(linelog::enabled!(Level::Info));
    info!("to stderr"; n = 1);
}
