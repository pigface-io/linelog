# The line formats and the level policy

Every record is one line, in one of two formats. A program picks the format;
libraries only log.

## JSON Lines

One JSON object per line, written with one `write` call so that lines from
different threads never interleave.

| key | value |
|---|---|
| `ts` | the time in UTC, RFC 3339 to the nanosecond: `2026-10-26T10:00:00.123456789Z` |
| `level` | `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE` |
| `target` | where the record came from: the module path, such as `app::net`, unless the call names one |
| `msg` | a short message, lowercase, with no trailing period |
| *fields* | the record's fields, in the order the call gives them |

Field values:

| Rust value | JSON |
|---|---|
| integers | number |
| `f32`, `f64` | number; `"NaN"`, `"inf"` and `"-inf"` as strings |
| `bool` | `true` or `false` |
| `str`, `String`, `Cow<str>` | string |
| `None` | `null` |
| `Duration` | number of milliseconds, with the fraction |
| `display(&x)`, `debug(&x)`, `char`, 128-bit integers | string |

A field named `ts`, `level`, `target` or `msg` is written with a `_` appended
(`msg_`), so no key appears twice.

DuckDB reads a log as a table:

```sql
select ts, level, msg, code
from read_json('app.log', format = 'newline_delimited')
where level in ('WARN', 'ERROR');
```

### OpenTelemetry

The keys map onto the OpenTelemetry log data model, so a collector can take
the lines as they are:

| key | OpenTelemetry |
|---|---|
| `ts` | `Timestamp` |
| `level` | `SeverityText`; `SeverityNumber` is 17, 13, 9, 5 and 1 from `ERROR` to `TRACE` |
| `target` | `InstrumentationScope.Name` |
| `msg` | `Body` |
| fields | `Attributes` |

## Text

For people, in the style of logfmt:

```text
2026-10-26T10:00:00.123456789Z WARN  app::net retrying url="https://x/a b" attempt=2
```

- The time, the level padded to five characters, the target, the message,
  then `key=value` for each field.
- The message is written as is, except that control characters are escaped
  (`\n`, `\t`, `\u0001`), so a record stays on one line.
- A text value is bare unless it is empty or holds a space, `=`, `"` or a
  control character; then it is quoted, with `"` and `\` escaped as in JSON.
- `None` writes nothing after the `=`; an empty string writes `""`.
- On a terminal, the time is shortened to the time of day (`10:00:00.123Z`),
  the level is coloured and keys are dimmed. `NO_COLOR` turns the colour off.

## Levels

| level | who logs it | what it means |
|---|---|---|
| `ERROR` | the program, once, at the top | the command failed; carries a `code` |
| `WARN` | anyone, always with a `code` | something degraded and the program carried on: a retry given up, a file set aside, a row rejected |
| `INFO` | the program; libraries only for milestones | a run, a file or a round began or ended, with its counts |
| `DEBUG` | libraries | a decision and why: what was picked, skipped, retried or waited for |
| `TRACE` | libraries | fine detail per block or entry; never per row in a hot loop |

Programs show `WARN` and above by default. `-v` raises that to `INFO`,
`-vv` to `DEBUG` and `-vvv` to `TRACE`; `-q` lowers it to `ERROR`. A
variable such as `APP_LOG=info,app::net=debug` sets a level per target.

## Filter directives

`level` sets the default, and `target=level` sets the level for a target and
every target under it (`::` or `.` continues a target). The longest matching
target wins. `off` passes nothing. A word that is not a level is an error,
never silently a target.
