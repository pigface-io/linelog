//! The two line formats: logfmt-style text and JSON Lines.

use std::cell::Cell;
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
    /// The fields' keys, as the call wrote them (`r#type` included).
    pub keys: &'a [&'a str],
    /// The fields' values, one for each key.
    pub values: &'a [&'a dyn Value],
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
    {
        zone!(TIME);
        if style.short_time {
            time_short(out, now);
        } else {
            time_full(out, now);
        }
    }
    zone!(HEAD);
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
    zone!(FIELDS);
    for (key, value) in rec.keys.iter().zip(rec.values) {
        let key = bare(key);
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

/// A key as written: `r#type` becomes `type`.
fn bare(key: &str) -> &str {
    key.strip_prefix("r#").unwrap_or(key)
}

fn text_value(out: &mut Vec<u8>, kind: Kind<'_>) {
    match kind {
        Kind::Str(s) => text_str(out, s),
        Kind::I64(n) => signed(out, n),
        Kind::U64(n) => unsigned(out, n),
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
            .any(|&b| ESCAPE[usize::from(b)] != 0 || b == b' ' || b == b'=')
}

fn json(out: &mut Vec<u8>, now: Duration, rec: &Record<'_>) {
    out.extend_from_slice(b"{\"ts\":\"");
    {
        zone!(TIME);
        time_full(out, now);
    }
    zone!(HEAD);
    out.extend_from_slice(b"\",\"level\":\"");
    out.extend_from_slice(rec.level.as_str().as_bytes());
    out.extend_from_slice(b"\",\"target\":");
    json_str(out, rec.target);
    out.extend_from_slice(b",\"msg\":");
    json_str(out, rec.msg);
    zone!(FIELDS);
    for (key, value) in rec.keys.iter().zip(rec.values) {
        let key = bare(key);
        out.extend_from_slice(b",\"");
        escape(out, key, true);
        if RESERVED.contains(&key) {
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
        Kind::I64(n) => signed(out, n),
        Kind::U64(n) => unsigned(out, n),
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
///
/// A float with at most three decimals, such as a duration in
/// milliseconds, is written from its integer digits: `x * 10^k` is an
/// integer `y`, and `y / 10^k == x` exactly, so the decimal `y / 10^k`
/// reads back as `x`. Anything else goes through `Debug`.
fn float(out: &mut Vec<u8>, x: f64) {
    const SCALE: [f64; 4] = [1.0, 10.0, 100.0, 1000.0];
    if x.is_nan() {
        out.extend_from_slice(b"NaN");
        return;
    }
    if x.is_infinite() {
        out.extend_from_slice(if x > 0.0 { b"inf" } else { b"-inf" });
        return;
    }
    if x.abs() < 1e15 && !(x == 0.0 && x.is_sign_negative()) {
        for (k, scale) in SCALE.into_iter().enumerate() {
            let y = x * scale;
            if y.fract() == 0.0 && y / scale == x {
                // |y| < 1e18, so it fits an i64 exactly.
                #[expect(clippy::cast_possible_truncation, reason = "an integer under 1e18")]
                let y = y as i64;
                decimal(out, y, k);
                return;
            }
        }
    }
    let _ = write!(out, "{x:?}");
}

/// `y / 10^k` with `k` decimals, at least one: `decimal(45, 1)` is `4.5`
/// and `decimal(3, 0)` is `3.0`.
fn decimal(out: &mut Vec<u8>, y: i64, k: usize) {
    if y < 0 {
        out.push(b'-');
    }
    let mut buf = [0u8; 20];
    let digits = itoa(&mut buf, y.unsigned_abs());
    if k == 0 {
        out.extend_from_slice(digits);
        out.extend_from_slice(b".0");
    } else if digits.len() > k {
        let (int, frac) = digits.split_at(digits.len() - k);
        out.extend_from_slice(int);
        out.push(b'.');
        out.extend_from_slice(frac);
    } else {
        out.extend_from_slice(b"0.");
        out.resize(out.len() + k - digits.len(), b'0');
        out.extend_from_slice(digits);
    }
}

fn signed(out: &mut Vec<u8>, n: i64) {
    if n < 0 {
        out.push(b'-');
    }
    unsigned(out, n.unsigned_abs());
}

fn unsigned(out: &mut Vec<u8>, n: u64) {
    let mut buf = [0u8; 20];
    out.extend_from_slice(itoa(&mut buf, n));
}

/// Two decimal digits for each of 0 to 99.
const PAIRS: &[u8; 200] = b"\
0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";

/// `n` in decimal, written into the end of `buf`; returns the digits.
fn itoa(buf: &mut [u8; 20], mut n: u64) -> &[u8] {
    let mut i = buf.len();
    while n >= 100 {
        let pair = usize::try_from(n % 100).unwrap_or(0) * 2;
        n /= 100;
        i -= 2;
        buf[i..i + 2].copy_from_slice(&PAIRS[pair..pair + 2]);
    }
    if n >= 10 {
        let pair = usize::try_from(n).unwrap_or(0) * 2;
        i -= 2;
        buf[i..i + 2].copy_from_slice(&PAIRS[pair..pair + 2]);
    } else {
        i -= 1;
        // n is a single digit here.
        buf[i] = b'0' + n as u8;
    }
    &buf[i..]
}

/// Writes a `Display` through [`escape`], straight into the line.
struct Escaper<'a>(&'a mut Vec<u8>);

impl fmt::Write for Escaper<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        escape(self.0, s, true);
        Ok(())
    }
}

/// Bytes [`escape`] must rewrite: 1 for a control character, 2 for `"`
/// and `\\`, which only need it inside quotes.
const ESCAPE: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut b = 0;
    while b < 0x20 {
        t[b] = 1;
        b += 1;
    }
    t[0x7f] = 1;
    t[b'"' as usize] = 2;
    t[b'\\' as usize] = 2;
    t
};

/// Appends `s` with every control character escaped as JSON does, so a
/// record stays on one line; with `quotes`, also `"` and `\\`.
fn escape(out: &mut Vec<u8>, s: &str, quotes: bool) {
    let mask = if quotes { 3 } else { 1 };
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if ESCAPE[usize::from(b)] & mask == 0 {
            continue;
        }
        out.extend_from_slice(&bytes[start..i]);
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            _ => out.extend_from_slice(&[
                b'\\',
                b'u',
                b'0',
                b'0',
                HEX[usize::from(b >> 4)],
                HEX[usize::from(b & 15)],
            ]),
        }
        start = i + 1;
    }
    out.extend_from_slice(&bytes[start..]);
}

/// RFC 3339 in UTC to the nanosecond: `2026-10-26T10:00:00.123456789Z`.
pub(crate) fn time_full(out: &mut Vec<u8>, now: Duration) {
    out.extend_from_slice(&second(now.as_secs()));
    out.push(b'.');
    digits(out, now.subsec_nanos(), 9);
    out.push(b'Z');
}

/// The time of day in UTC to the millisecond: `10:00:00.123Z`.
fn time_short(out: &mut Vec<u8>, now: Duration) {
    let second = second(now.as_secs());
    out.extend_from_slice(&second[second.len() - 8..]);
    out.push(b'.');
    digits(out, now.subsec_millis(), 3);
    out.push(b'Z');
}

/// `YYYY-MM-DDTHH:MM:SS` for a second since the epoch. Records come many a
/// second, so each thread keeps the last second it wrote and only
/// formats a new one.
fn second(secs: u64) -> Vec19 {
    thread_local! {
        static LAST: Cell<(u64, Vec19)> = const { Cell::new((u64::MAX, Vec19::new())) };
    }
    // While the thread exits the cache is gone, and every second is new.
    let (at, text) = LAST.try_with(Cell::get).unwrap_or((u64::MAX, Vec19::new()));
    if at == secs {
        return text;
    }
    let text = format_second(secs);
    let _ = LAST.try_with(|last| last.set((secs, text)));
    text
}

/// The date and time to the second, in a fixed buffer: up to 9999 it is 19
/// bytes, and later years take more.
#[derive(Clone, Copy)]
struct Vec19 {
    len: u8,
    bytes: [u8; 24],
}

impl Vec19 {
    const fn new() -> Self {
        Self {
            len: 0,
            bytes: [0; 24],
        }
    }
}

impl std::ops::Deref for Vec19 {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

fn format_second(secs: u64) -> Vec19 {
    let mut text = Vec19::new();
    let mut put = |bytes: &[u8]| {
        let at = usize::from(text.len);
        let end = (at + bytes.len()).min(text.bytes.len());
        text.bytes[at..end].copy_from_slice(&bytes[..end - at]);
        // At most 24.
        text.len = end as u8;
    };
    let (y, m, d) = civil_from_days(i64::try_from(secs / 86_400).unwrap_or(i64::MAX));
    let mut buf = [0u8; 20];
    // Seconds since 1970 never fall before it.
    let year = itoa(&mut buf, y.unsigned_abs());
    put(&b"000"[..4usize.saturating_sub(year.len())]);
    put(year);
    let of_day = u32::try_from(secs % 86_400).unwrap_or(0);
    let two = |n: u32| {
        let i = usize::try_from(n).unwrap_or(0) * 2;
        [PAIRS[i], PAIRS[i + 1]]
    };
    let [m0, m1] = two(m);
    let [d0, d1] = two(d);
    let [h0, h1] = two(of_day / 3600);
    let [i0, i1] = two(of_day / 60 % 60);
    let [s0, s1] = two(of_day % 60);
    put(&[
        b'-', m0, m1, b'-', d0, d1, b'T', h0, h1, b':', i0, i1, b':', s0, s1,
    ]);
    text
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

    /// Renders a warning with these fields.
    fn rec(format: Format, style: Style, fields: &[(&str, &dyn Value)]) -> String {
        let keys: Vec<&str> = fields.iter().map(|(k, _)| *k).collect();
        let values: Vec<&dyn Value> = fields.iter().map(|(_, v)| *v).collect();
        line(
            format,
            style,
            &Record {
                level: Level::Warn,
                target: "net::fetch",
                msg: "retrying\r\nnow",
                keys: &keys,
                values: &values,
            },
        )
    }

    #[test]
    fn text_lines() {
        let fields: [(&str, &dyn Value); 12] = [
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
            ("r#type", &1u8),
        ];
        assert_eq!(
            rec(Format::Text, Style::default(), &fields),
            "2026-10-26T10:00:00.123456789Z WARN  net::fetch retrying\\r\\nnow \
             url=\"https://x/a b\" attempt=2 delta=-3 ratio=0.5 ok=true missing= \
             empty=\"\" path=/p state=\"\\\"s\\\"\" no=false kind=csv type=1\n"
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
            rec(Format::Text, style, &fields),
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
            rec(Format::Json, Style::default(), &fields),
            r#"{"ts":"2026-10-26T10:00:00.123456789Z","level":"WARN","target":"net::fetch","msg":"retrying\r\nnow","url":"a\"b\\c\u0001","attempt":2,"delta":-3,"ratio":1e300,"nan":"NaN","ok":false,"yes":true,"missing":null,"path":"/p\t","msg_":"dup","up":"inf","down":"-inf"}"#
                .to_owned()
                + "\n"
        );
    }

    #[test]
    fn floats_read_back_as_themselves() {
        let text = |x: f64| {
            let mut out = Vec::new();
            float(&mut out, x);
            String::from_utf8(out).unwrap()
        };
        for (x, want) in [
            (3.0, "3.0"),
            (-4.5, "-4.5"),
            (0.005, "0.005"),
            (-0.25, "-0.25"),
            (12.125, "12.125"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-0.0, "-0.0"),
            (1e15, "1000000000000000.0"),
            (1e-7, "1e-7"),
        ] {
            assert_eq!(text(x), want);
            assert_eq!(text(x).parse::<f64>().unwrap().to_bits(), x.to_bits());
        }
        let mut out = Vec::new();
        signed(&mut out, i64::MIN);
        unsigned(&mut out, 7);
        unsigned(&mut out, u64::MAX);
        assert_eq!(out, b"-9223372036854775808718446744073709551615");
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
