//! The two line formats: logfmt-style text and JSON Lines.

use std::fmt;
use std::io::Write as _;
use std::str::FromStr;
use std::time::Duration;

use crate::value::Kind;
use crate::{Error, Level, Value};

/// How a record is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// For people: `10:00:00.123Z WARN  net retrying url=https://… attempt=2`.
    Text,
    /// For machines: one JSON object a line, keys `ts`, `level`, `target`,
    /// `msg`, then the fields in the order given.
    Json,
}

impl FromStr for Format {
    type Err = Error;

    /// Reads `text` or `json`, in any case.
    fn from_str(s: &str) -> Result<Self, Error> {
        if s.eq_ignore_ascii_case("text") {
            Ok(Self::Text)
        } else if s.eq_ignore_ascii_case("json") {
            Ok(Self::Json)
        } else {
            Err(Error::Format(s.to_owned()))
        }
    }
}

/// How text lines look: colour and the length of the time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Style {
    /// ANSI colour on the level and dimmed keys.
    pub color: bool,
    /// The time of day to the millisecond, not the full date and time.
    pub short_time: bool,
}

/// One record, before it is written.
pub(crate) struct Record<'a> {
    pub level: Level,
    pub target: &'a str,
    pub msg: &'a str,
    pub fields: &'a [(&'a str, &'a dyn Value)],
}

/// Keys the JSON format writes itself; a field with one of these names
/// gets a `_` appended so the object never holds a key twice.
const RESERVED: [&str; 4] = ["ts", "level", "target", "msg"];

const DIM: &[u8] = b"\x1b[2m";
const RESET: &[u8] = b"\x1b[0m";
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Appends `rec` to `out` as one line, newline included. `now` is the time
/// since the Unix epoch.
pub(crate) fn render(
    out: &mut Vec<u8>,
    format: Format,
    style: Style,
    now: Duration,
    rec: &Record<'_>,
) {
    match format {
        Format::Text => text(out, style, now, rec),
        Format::Json => json(out, now, rec),
    }
    out.push(b'\n');
}

fn text(out: &mut Vec<u8>, style: Style, now: Duration, rec: &Record<'_>) {
    if style.short_time {
        time_short(out, now);
    } else {
        time_full(out, now);
    }
    out.push(b' ');
    let name = rec.level.as_str();
    if style.color {
        out.extend_from_slice(rec.level.color().as_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(RESET);
    } else {
        out.extend_from_slice(name.as_bytes());
    }
    out.resize(out.len() + 6 - name.len(), b' ');
    if style.color {
        out.extend_from_slice(DIM);
        out.extend_from_slice(rec.target.as_bytes());
        out.extend_from_slice(RESET);
    } else {
        out.extend_from_slice(rec.target.as_bytes());
    }
    out.push(b' ');
    escape(out, rec.msg, false);
    for (key, value) in rec.fields {
        out.push(b' ');
        if style.color {
            out.extend_from_slice(DIM);
            out.extend_from_slice(key.as_bytes());
            out.push(b'=');
            out.extend_from_slice(RESET);
        } else {
            out.extend_from_slice(key.as_bytes());
            out.push(b'=');
        }
        text_value(out, value.kind());
    }
}

fn text_value(out: &mut Vec<u8>, kind: Kind<'_>) {
    match kind {
        Kind::Str(s) => text_str(out, s),
        Kind::I64(n) => {
            let _ = write!(out, "{n}");
        }
        Kind::U64(n) => {
            let _ = write!(out, "{n}");
        }
        Kind::F64(x) => float(out, x),
        Kind::Bool(b) => out.extend_from_slice(if b { b"true" } else { b"false" }),
        Kind::Null => {}
        Kind::Display(d) => {
            let mark = out.len();
            let _ = write!(out, "{d}");
            if needs_quotes(&out[mark..]) {
                let raw = out.split_off(mark);
                text_str(out, &String::from_utf8_lossy(&raw));
            }
        }
    }
}

/// Text as a logfmt value: bare when it can be, else quoted with escapes.
fn text_str(out: &mut Vec<u8>, s: &str) {
    if needs_quotes(s.as_bytes()) {
        out.push(b'"');
        escape(out, s, true);
        out.push(b'"');
    } else {
        out.extend_from_slice(s.as_bytes());
    }
}

/// Whether a logfmt value must be quoted: empty, or holding a space, `=`,
/// `"` or a control character.
fn needs_quotes(s: &[u8]) -> bool {
    s.is_empty()
        || s.iter()
            .any(|&b| matches!(b, b' ' | b'=' | b'"') || b.is_ascii_control())
}

fn json(out: &mut Vec<u8>, now: Duration, rec: &Record<'_>) {
    out.extend_from_slice(b"{\"ts\":\"");
    time_full(out, now);
    out.extend_from_slice(b"\",\"level\":\"");
    out.extend_from_slice(rec.level.as_str().as_bytes());
    out.extend_from_slice(b"\",\"target\":");
    json_str(out, rec.target);
    out.extend_from_slice(b",\"msg\":");
    json_str(out, rec.msg);
    for (key, value) in rec.fields {
        out.extend_from_slice(b",\"");
        escape(out, key, true);
        if RESERVED.contains(key) {
            out.push(b'_');
        }
        out.extend_from_slice(b"\":");
        json_value(out, value.kind());
    }
    out.push(b'}');
}

fn json_value(out: &mut Vec<u8>, kind: Kind<'_>) {
    match kind {
        Kind::Str(s) => json_str(out, s),
        Kind::I64(n) => {
            let _ = write!(out, "{n}");
        }
        Kind::U64(n) => {
            let _ = write!(out, "{n}");
        }
        Kind::F64(x) if x.is_finite() => float(out, x),
        Kind::F64(x) => {
            out.push(b'"');
            float(out, x);
            out.push(b'"');
        }
        Kind::Bool(b) => out.extend_from_slice(if b { b"true" } else { b"false" }),
        Kind::Null => out.extend_from_slice(b"null"),
        Kind::Display(d) => {
            out.push(b'"');
            let _ = fmt::write(&mut Escaper(out), format_args!("{d}"));
            out.push(b'"');
        }
    }
}

fn json_str(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    escape(out, s, true);
    out.push(b'"');
}

/// The shortest text that reads back as the same float; `NaN`, `inf` and
/// `-inf` for the values JSON has no number for.
fn float(out: &mut Vec<u8>, x: f64) {
    if x.is_nan() {
        out.extend_from_slice(b"NaN");
    } else if x.is_infinite() {
        out.extend_from_slice(if x > 0.0 { b"inf" } else { b"-inf" });
    } else {
        let _ = write!(out, "{x:?}");
    }
}

/// Writes a `Display` through [`escape`], straight into the line.
struct Escaper<'a>(&'a mut Vec<u8>);

impl fmt::Write for Escaper<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        escape(self.0, s, true);
        Ok(())
    }
}

/// Appends `s` with every control character escaped as JSON does, so a
/// record stays on one line; with `quotes`, also `"` and `\`.
fn escape(out: &mut Vec<u8>, s: &str, quotes: bool) {
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let short: Option<&[u8]> = match b {
            b'"' if quotes => Some(b"\\\""),
            b'\\' if quotes => Some(b"\\\\"),
            b'\n' => Some(b"\\n"),
            b'\r' => Some(b"\\r"),
            b'\t' => Some(b"\\t"),
            _ => None,
        };
        if short.is_none() && !b.is_ascii_control() {
            continue;
        }
        out.extend_from_slice(&bytes[start..i]);
        let code = [
            b'\\',
            b'u',
            b'0',
            b'0',
            HEX[usize::from(b >> 4)],
            HEX[usize::from(b & 15)],
        ];
        out.extend_from_slice(short.unwrap_or(&code));
        start = i + 1;
    }
    out.extend_from_slice(&bytes[start..]);
}

/// RFC 3339 in UTC to the nanosecond: `2026-10-26T10:00:00.123456789Z`.
pub(crate) fn time_full(out: &mut Vec<u8>, now: Duration) {
    let secs = now.as_secs();
    let (y, m, d) = civil_from_days(i64::try_from(secs / 86_400).unwrap_or(i64::MAX));
    match u32::try_from(y) {
        Ok(y) if y < 10_000 => digits(out, y, 4),
        _ => {
            let _ = write!(out, "{y}");
        }
    }
    out.push(b'-');
    digits(out, m, 2);
    out.push(b'-');
    digits(out, d, 2);
    out.push(b'T');
    clock(out, secs);
    out.push(b'.');
    digits(out, now.subsec_nanos(), 9);
    out.push(b'Z');
}

/// The time of day in UTC to the millisecond: `10:00:00.123Z`.
fn time_short(out: &mut Vec<u8>, now: Duration) {
    clock(out, now.as_secs());
    out.push(b'.');
    digits(out, now.subsec_millis(), 3);
    out.push(b'Z');
}

/// `HH:MM:SS` of the day `secs` falls in.
fn clock(out: &mut Vec<u8>, secs: u64) {
    // Under 86 400, so each part fits a u32.
    let of_day = u32::try_from(secs % 86_400).unwrap_or(0);
    digits(out, of_day / 3600, 2);
    out.push(b':');
    digits(out, of_day / 60 % 60, 2);
    out.push(b':');
    digits(out, of_day % 60, 2);
}

/// `n` in `width` decimal digits, zero-padded; higher digits are dropped.
fn digits(out: &mut Vec<u8>, mut n: u32, width: usize) {
    let start = out.len();
    out.resize(start + width, b'0');
    for slot in out[start..].iter_mut().rev() {
        // n % 10 is a single digit.
        *slot = b'0' + (n % 10) as u8;
        n /= 10;
    }
}

/// The proleptic Gregorian date of a day counted from 1970-01-01, by
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    // m is 1..=12 and d is 1..=31.
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{debug, display};

    /// 2026-10-26T10:00:00.123456789Z.
    const NOW: Duration = Duration::new(1_793_008_800, 123_456_789);

    fn line(format: Format, style: Style, rec: &Record<'_>) -> String {
        let mut out = Vec::new();
        render(&mut out, format, style, NOW, rec);
        String::from_utf8(out).unwrap()
    }

    fn rec<'a>(fields: &'a [(&'a str, &'a dyn Value)]) -> Record<'a> {
        Record {
            level: Level::Warn,
            target: "net::fetch",
            msg: "retrying\r\nnow",
            fields,
        }
    }

    #[test]
    fn text_lines() {
        let fields: [(&str, &dyn Value); 11] = [
            ("url", &"https://x/a b"),
            ("attempt", &2u32),
            ("delta", &-3i64),
            ("ratio", &0.5f64),
            ("ok", &true),
            ("missing", &None::<u8>),
            ("empty", &""),
            ("path", &display(&"/p")),
            ("state", &debug(&"s")),
            ("no", &false),
            ("kind", &"csv"),
        ];
        assert_eq!(
            line(Format::Text, Style::default(), &rec(&fields)),
            "2026-10-26T10:00:00.123456789Z WARN  net::fetch retrying\\r\\nnow \
             url=\"https://x/a b\" attempt=2 delta=-3 ratio=0.5 ok=true missing= \
             empty=\"\" path=/p state=\"\\\"s\\\"\" no=false kind=csv\n"
        );
    }

    #[test]
    fn text_on_a_terminal() {
        let fields: [(&str, &dyn Value); 1] = [("n", &1u8)];
        let style = Style {
            color: true,
            short_time: true,
        };
        assert_eq!(
            line(Format::Text, style, &rec(&fields)),
            "10:00:00.123Z \x1b[33mWARN\x1b[0m  \x1b[2mnet::fetch\x1b[0m retrying\\r\\nnow \
             \x1b[2mn=\x1b[0m1\n"
        );
    }

    #[test]
    fn json_lines() {
        let fields: [(&str, &dyn Value); 12] = [
            ("url", &"a\"b\\c\u{1}"),
            ("attempt", &2u32),
            ("delta", &-3i64),
            ("ratio", &1e300f64),
            ("nan", &f64::NAN),
            ("ok", &false),
            ("yes", &true),
            ("missing", &None::<u8>),
            ("path", &display(&"/p\t")),
            ("msg", &"dup"),
            ("up", &f64::INFINITY),
            ("down", &f64::NEG_INFINITY),
        ];
        assert_eq!(
            line(Format::Json, Style::default(), &rec(&fields)),
            r#"{"ts":"2026-10-26T10:00:00.123456789Z","level":"WARN","target":"net::fetch","msg":"retrying\r\nnow","url":"a\"b\\c\u0001","attempt":2,"delta":-3,"ratio":1e300,"nan":"NaN","ok":false,"yes":true,"missing":null,"path":"/p\t","msg_":"dup","up":"inf","down":"-inf"}"#
                .to_owned()
                + "\n"
        );
    }

    #[test]
    fn formats_by_name() {
        assert_eq!("TEXT".parse::<Format>(), Ok(Format::Text));
        assert_eq!("json".parse::<Format>(), Ok(Format::Json));
        assert_eq!("xml".parse::<Format>(), Err(Error::Format("xml".into())));
    }

    #[test]
    fn dates_across_the_calendar() {
        let at = |secs| {
            let mut out = Vec::new();
            time_full(&mut out, Duration::from_secs(secs));
            String::from_utf8(out).unwrap()
        };
        assert_eq!(at(0), "1970-01-01T00:00:00.000000000Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00.000000000Z");
        assert_eq!(at(4_107_542_399), "2100-02-28T23:59:59.000000000Z");
        assert_eq!(at(253_402_300_800), "10000-01-01T00:00:00.000000000Z");
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}
