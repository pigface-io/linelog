//! The zones a record passes through, with the feature `zones`. Ids
//! 1 to 99 are linelog's.

use zonetrace::Zone;

/// A whole record, from the callsite's yes to the sink's return.
pub(crate) static LOG: Zone = Zone::new(1, "log");
/// Reading the wall clock.
pub(crate) static CLOCK: Zone = Zone::new(2, "log.clock");
/// Formatting the line into the thread's buffer.
pub(crate) static RENDER: Zone = Zone::new(3, "log.render");
/// The timestamp.
pub(crate) static TIME: Zone = Zone::new(4, "log.render.time");
/// The level, target and message.
pub(crate) static HEAD: Zone = Zone::new(5, "log.render.head");
/// The fields.
pub(crate) static FIELDS: Zone = Zone::new(6, "log.render.fields");
/// Handing the line to the sink.
pub(crate) static SINK: Zone = Zone::new(7, "log.sink");
