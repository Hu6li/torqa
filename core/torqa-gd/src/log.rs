//! The core's log (`tracing`) on standard error, so that problems show in the terminal Torqa
//! was started from (or Godot's output when run from the editor) — without it, warnings such
//! as a video that cannot be decoded would go nowhere.

use std::fmt::Write as _;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// Writes events at info level and above as `Torqa WARN target: message key=value`.
pub struct StderrLog;

impl Subscriber for StderrLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= Level::INFO && metadata.target().starts_with("torqa")
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        // Spans are not shown; every one gets the same id.
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let mut line = format!("Torqa {} {}:", metadata.level(), metadata.target());
        event.record(&mut Line(&mut line));
        eprintln!("{line}");
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, " {value:?}");
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            let _ = write!(self.0, " {value}");
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }
}
