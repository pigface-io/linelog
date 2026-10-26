# Changelog

All notable changes to linelog. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Records with a level, a target, a message and typed fields, written as
  one line of logfmt-style text or JSON Lines.
- Filters by level per target prefix, read from a variable the program
  names; a call site remembers its answer until the filter changes.
- Sinks for standard error, an appended file, a closure and an in-memory
  capture; a panic hook that logs panics as records.
