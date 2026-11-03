//! JSONL run event log (`event_log.schema.json`): ground truth for scoring and replay.

use arcadia_protocol::{LogEntry, LogRecord, SimTime, wire};
use std::io::Write;

pub struct EventLog<W: Write> {
    out: W,
}

impl<W: Write> EventLog<W> {
    pub fn new(out: W) -> Self {
        Self { out }
    }

    pub fn record(&mut self, t: SimTime, entry: LogEntry) -> Result<(), wire::WireError> {
        wire::send(&mut self.out, &LogRecord { t, entry })
    }

    pub fn into_inner(self) -> W {
        self.out
    }
}
