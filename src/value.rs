//! Field values: what a record's `key = value` pairs may hold.

use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

/// What a value is, for the formatter to write.
#[derive(Clone, Copy)]
pub enum Kind<'a> {
    /// Text.
    Str(&'a str),
    /// A signed integer.
    I64(i64),
    /// An unsigned integer.
    U64(u64),
    /// A float. NaN and the infinities are written as text.
    F64(f64),
    /// A boolean.
    Bool(bool),
    /// No value: an empty `Option`.
    Null,
    /// Anything else, written through its `Display` as text.
    Display(&'a dyn fmt::Display),
}

/// A value a field can hold.
///
/// Implemented for the integers, floats, `bool`, `char`, `str`, `String`,
/// `Cow<str>`, `Option` and `Duration` (written in milliseconds). Wrap any
/// other type in [`display`] or [`debug`], or implement this trait for it.
pub trait Value {
    /// What the value is.
    fn kind(&self) -> Kind<'_>;
}

impl<T: Value + ?Sized> Value for &T {
    fn kind(&self) -> Kind<'_> {
        (**self).kind()
    }
}

macro_rules! signed {
    ($($t:ty),*) => {$(
        impl Value for $t {
            fn kind(&self) -> Kind<'_> {
                Kind::I64(i64::from(*self))
            }
        }
    )*};
}
signed!(i8, i16, i32, i64);

macro_rules! unsigned {
    ($($t:ty),*) => {$(
        impl Value for $t {
            fn kind(&self) -> Kind<'_> {
                Kind::U64(u64::from(*self))
            }
        }
    )*};
}
unsigned!(u8, u16, u32, u64);

impl Value for isize {
    fn kind(&self) -> Kind<'_> {
        i64::try_from(*self).map_or(Kind::Display(self), Kind::I64)
    }
}

impl Value for usize {
    fn kind(&self) -> Kind<'_> {
        u64::try_from(*self).map_or(Kind::Display(self), Kind::U64)
    }
}

macro_rules! displayed {
    ($($t:ty),*) => {$(
        impl Value for $t {
            fn kind(&self) -> Kind<'_> {
                Kind::Display(self)
            }
        }
    )*};
}
displayed!(i128, u128, char);

impl Value for f32 {
    fn kind(&self) -> Kind<'_> {
        Kind::F64(f64::from(*self))
    }
}

impl Value for f64 {
    fn kind(&self) -> Kind<'_> {
        Kind::F64(*self)
    }
}

impl Value for bool {
    fn kind(&self) -> Kind<'_> {
        Kind::Bool(*self)
    }
}

impl Value for str {
    fn kind(&self) -> Kind<'_> {
        Kind::Str(self)
    }
}

impl Value for String {
    fn kind(&self) -> Kind<'_> {
        Kind::Str(self)
    }
}

impl Value for Cow<'_, str> {
    fn kind(&self) -> Kind<'_> {
        Kind::Str(self)
    }
}

impl<T: Value> Value for Option<T> {
    fn kind(&self) -> Kind<'_> {
        self.as_ref().map_or(Kind::Null, Value::kind)
    }
}

/// Milliseconds, with the fraction kept.
impl Value for Duration {
    fn kind(&self) -> Kind<'_> {
        Kind::F64(self.as_secs_f64() * 1000.0)
    }
}

/// A value written through its `Display`; made by [`display`].
pub struct DisplayValue<'a, T: ?Sized>(&'a T);

impl<T: fmt::Display + ?Sized> Value for DisplayValue<'_, T> {
    fn kind(&self) -> Kind<'_> {
        Kind::Display(&self.0)
    }
}

/// Writes `v` through its `Display`: `path = display(&p.display())`.
pub const fn display<T: fmt::Display + ?Sized>(v: &T) -> DisplayValue<'_, T> {
    DisplayValue(v)
}

/// A value written through its `Debug`; made by [`debug`].
pub struct DebugValue<'a, T: ?Sized>(&'a T);

impl<T: fmt::Debug + ?Sized> fmt::Display for DebugValue<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}

impl<T: fmt::Debug + ?Sized> Value for DebugValue<'_, T> {
    fn kind(&self) -> Kind<'_> {
        Kind::Display(self)
    }
}

/// Writes `v` through its `Debug`: `state = debug(&state)`.
pub const fn debug<T: fmt::Debug + ?Sized>(v: &T) -> DebugValue<'_, T> {
    DebugValue(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind as text, to compare in tests.
    fn show(v: &dyn Value) -> String {
        match v.kind() {
            Kind::Str(s) => format!("str {s}"),
            Kind::I64(n) => format!("i64 {n}"),
            Kind::U64(n) => format!("u64 {n}"),
            Kind::F64(x) => format!("f64 {x}"),
            Kind::Bool(b) => format!("bool {b}"),
            Kind::Null => "null".to_owned(),
            Kind::Display(d) => format!("display {d}"),
        }
    }

    #[test]
    fn each_type_has_its_kind() {
        assert_eq!(show(&-1i8), "i64 -1");
        assert_eq!(show(&-2i16), "i64 -2");
        assert_eq!(show(&-3i32), "i64 -3");
        assert_eq!(show(&-4i64), "i64 -4");
        assert_eq!(show(&-5isize), "i64 -5");
        assert_eq!(show(&1u8), "u64 1");
        assert_eq!(show(&2u16), "u64 2");
        assert_eq!(show(&3u32), "u64 3");
        assert_eq!(show(&4u64), "u64 4");
        assert_eq!(show(&5usize), "u64 5");
        assert_eq!(show(&6i128), "display 6");
        assert_eq!(show(&7u128), "display 7");
        assert_eq!(show(&'x'), "display x");
        assert_eq!(show(&1.5f32), "f64 1.5");
        assert_eq!(show(&2.5f64), "f64 2.5");
        assert_eq!(show(&true), "bool true");
        assert!(matches!(str::kind("s"), Kind::Str("s")));
        assert_eq!(show(&"r"), "str r");
        assert_eq!(show(&String::from("t")), "str t");
        assert_eq!(show(&Cow::Borrowed("c")), "str c");
        assert_eq!(show(&Some(1u8)), "u64 1");
        assert_eq!(show(&None::<u8>), "null");
        assert_eq!(show(&Duration::from_micros(1500)), "f64 1.5");
        assert_eq!(
            show(&display(&std::path::Path::new("/a").display())),
            "display /a"
        );
        assert_eq!(show(&debug(&"q")), r#"display "q""#);
    }
}
