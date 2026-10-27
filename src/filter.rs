//! Which records pass: a default level and a level per target prefix.

use crate::{Error, Level};

/// The threshold that lets nothing through.
const OFF: u8 = 0;

/// Which records pass, by level and target.
///
/// A filter holds a default level and any number of directives, each a
/// target prefix with its own level. A record passes when its level is at
/// or below the level of the longest directive that matches its target, or
/// the default when none does. A directive matches a target equal to it or
/// one that continues with `::` or `.`: `phasor_net` matches
/// `phasor_net::fetch` and `phasor_net.retry`, not `phasor_netx`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    default: u8,
    /// Longest target first, so the first match is the most specific.
    directives: Vec<(String, u8)>,
}

impl Filter {
    /// A filter that passes `level` and everything more severe.
    pub const fn new(level: Level) -> Self {
        Self {
            default: level as u8,
            directives: Vec::new(),
        }
    }

    /// A filter that passes nothing.
    pub const fn off() -> Self {
        Self {
            default: OFF,
            directives: Vec::new(),
        }
    }

    /// The same filter with `target` and the targets under it at `level`.
    #[must_use]
    pub fn with(mut self, target: &str, level: Level) -> Self {
        self.set(target, level as u8);
        self
    }

    /// Reads directives separated by commas, as in `RUST_LOG`:
    /// `info,phasor_net=debug,phasor_pipeline::run=trace`.
    ///
    /// A bare level sets the default and `target=level` sets a directive;
    /// `off` is a level that passes nothing. Unlike `RUST_LOG`, a bare word
    /// must be a level: a misspelt level is an error, never a target.
    ///
    /// # Errors
    ///
    /// [`Error::Directive`] names the first directive that does not read.
    pub fn parse(spec: &str) -> Result<Self, Error> {
        let mut filter = Self::new(Level::Info);
        for directive in spec.split(',').map(str::trim).filter(|d| !d.is_empty()) {
            let bad = |reason| Error::Directive {
                directive: directive.to_owned(),
                reason,
            };
            match directive.split_once('=') {
                Some((target, level)) => {
                    let target = target.trim();
                    if target.is_empty() {
                        return Err(bad("empty target"));
                    }
                    let level = threshold(level.trim()).ok_or_else(|| bad("unknown level"))?;
                    filter.set(target, level);
                }
                None => {
                    filter.default = threshold(directive).ok_or_else(|| bad("unknown level"))?;
                }
            }
        }
        Ok(filter)
    }

    /// Whether a record at `level` from `target` passes.
    pub fn enabled(&self, level: Level, target: &str) -> bool {
        level as u8 <= self.threshold(target)
    }

    /// The most verbose level any target passes, as a number; 0 is off.
    pub(crate) fn max(&self) -> u8 {
        self.directives
            .iter()
            .map(|(_, l)| *l)
            .fold(self.default, u8::max)
    }

    fn threshold(&self, target: &str) -> u8 {
        self.directives
            .iter()
            .find(|(prefix, _)| matches(target, prefix))
            .map_or(self.default, |(_, l)| *l)
    }

    fn set(&mut self, target: &str, level: u8) {
        self.directives.retain(|(t, _)| t != target);
        self.directives.push((target.to_owned(), level));
        self.directives
            .sort_by_key(|(t, _)| std::cmp::Reverse(t.len()));
    }
}

/// A level's name, or `off`, as a threshold.
fn threshold(name: &str) -> Option<u8> {
    if name.eq_ignore_ascii_case("off") {
        return Some(OFF);
    }
    name.parse::<Level>().ok().map(|l| l as u8)
}

fn matches(target: &str, prefix: &str) -> bool {
    target
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::") || rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_matching_directive_wins() {
        let f = Filter::parse("warn, a=debug ,a::b=trace,").unwrap();
        assert!(f.enabled(Level::Warn, "x"));
        assert!(!f.enabled(Level::Info, "x"));
        assert!(f.enabled(Level::Debug, "a::c"));
        assert!(!f.enabled(Level::Trace, "a::c"));
        assert!(f.enabled(Level::Trace, "a::b::c"));
        assert!(f.enabled(Level::Debug, "a.dotted"));
        assert!(!f.enabled(Level::Debug, "ab"));
        assert_eq!(f.max(), Level::Trace as u8);
    }

    #[test]
    fn off_and_case() {
        let f = Filter::parse("OFF,net=Info").unwrap();
        assert!(!f.enabled(Level::Error, "x"));
        assert!(f.enabled(Level::Info, "net"));
        assert_eq!(Filter::parse("").unwrap(), Filter::new(Level::Info));
        assert!(!Filter::off().enabled(Level::Error, "x"));
        assert_eq!(Filter::off().max(), 0);
    }

    #[test]
    fn a_later_directive_replaces_an_earlier_one() {
        let f = Filter::new(Level::Warn)
            .with("a", Level::Trace)
            .with("a", Level::Error);
        assert!(!f.enabled(Level::Warn, "a"));
        assert_eq!(f, Filter::parse("warn,a=trace,a=error").unwrap());
    }

    #[test]
    fn a_bad_directive_is_an_error() {
        for (spec, directive, reason) in [
            ("infoo", "infoo", "unknown level"),
            ("info,a=loud", "a=loud", "unknown level"),
            ("=debug", "=debug", "empty target"),
        ] {
            assert_eq!(
                Filter::parse(spec),
                Err(Error::Directive {
                    directive: directive.to_owned(),
                    reason
                })
            );
        }
    }
}
