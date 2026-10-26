//! Session records: a lossless, direction-aware packet log, and a replayer that can seek.
//!
//! One record is one plaintext packet (starting with its 2-byte little-endian opcode), the direction it travelled and when.
//! Files are JSON lines (`.nsr`), one record per line, so they can be appended to while a capture is running:
//!
//! ```text
//! {"t":12345,"d":"in","hex":"2d02..."}      t = milliseconds since the session started
//! ```
//!
//! The older stamped text log (`<unix s> op <hex4> len <n>: <hex bytes>`, incoming only) is read too, so every capture made
//! so far replays. Outgoing records come from the opt-in gdb tool and may contain login tokens: keep them local.

use neon_protocol::Dir;

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub t_ms: u64,
    pub dir: Dir,
    pub plain: Vec<u8>,
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn bytes_of(h: &str) -> Option<Vec<u8>> {
    let h: String = h.chars().filter(|c| !c.is_whitespace()).collect();
    if h.len() % 2 != 0 {
        return None;
    }
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()).collect()
}

impl Record {
    pub fn to_line(&self) -> String {
        let d = match self.dir {
            Dir::In => "in",
            Dir::Out => "out",
        };
        format!("{{\"t\":{},\"d\":\"{d}\",\"hex\":\"{}\"}}", self.t_ms, hex_of(&self.plain))
    }

    /// Parse either file format. `t_ms` of a legacy line is its unix time in ms; [`Session::from_lines`] makes times relative.
    pub fn parse_line(line: &str) -> Option<Record> {
        let line = line.trim();
        if line.starts_with('{') {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            let dir = match v.get("d")?.as_str()? {
                "in" => Dir::In,
                "out" => Dir::Out,
                _ => return None,
            };
            return Some(Record { t_ms: v.get("t")?.as_u64()?, dir, plain: bytes_of(v.get("hex")?.as_str()?)? });
        }
        let (secs, plain) = crate::parse_log_line(line)?;
        Some(Record { t_ms: (secs * 1000.0) as u64, dir: Dir::In, plain })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Session {
    pub records: Vec<Record>,
    pub bad_lines: usize,
    /// Unix ms of time zero when the records carried absolute times (so wall-clock marks can be placed on the timeline).
    pub origin_unix_ms: Option<u64>,
}

impl Session {
    /// Parse lines, drop what is not a record, and make times relative to the first record, in order.
    pub fn from_lines<I: IntoIterator<Item = String>>(lines: I) -> Session {
        let mut s = Session::default();
        for l in lines {
            if l.trim().is_empty() {
                continue;
            }
            match Record::parse_line(&l) {
                Some(r) => s.records.push(r),
                None => s.bad_lines += 1,
            }
        }
        // Several files may be merged, so time zero is the earliest record, not the first line read.
        if let Some(t0) = s.records.iter().map(|r| r.t_ms).min() {
            // legacy logs carry unix ms; relative sessions already start near 0: subtract only when it is clearly absolute
            if t0 > 1_000_000_000_000 {
                s.origin_unix_ms = Some(t0);
                for r in &mut s.records {
                    r.t_ms -= t0;
                }
            }
        }
        s.records.sort_by_key(|r| r.t_ms); // stable: equal times keep their order
        s
    }

    pub fn duration_ms(&self) -> u64 {
        self.records.last().map_or(0, |r| r.t_ms)
    }

    pub fn to_text(&self) -> String {
        self.records.iter().map(|r| r.to_line() + "\n").collect()
    }

    /// Index of the first record after `t_ms` (everything before it has happened by then).
    pub fn upto(&self, t_ms: u64) -> usize {
        self.records.partition_point(|r| r.t_ms <= t_ms)
    }
}

/// Anything a replay can drive: the view's shared state, a test double, a recorder.
pub trait Sink {
    fn reset(&mut self);
    fn feed(&mut self, rec: &Record);
}

/// Seekable, speed-controlled playback of a [`Session`] into a [`Sink`].
#[derive(Debug)]
pub struct Replayer {
    pub session: Session,
    cursor: usize,
    pos_ms: f64,
    pub playing: bool,
    pub speed: f64,
}

impl Replayer {
    pub fn new(session: Session) -> Self {
        Replayer { session, cursor: 0, pos_ms: 0.0, playing: false, speed: 1.0 }
    }

    pub fn pos_ms(&self) -> u64 {
        self.pos_ms as u64
    }

    pub fn at_end(&self) -> bool {
        self.cursor >= self.session.records.len()
    }

    /// Jump to `t_ms`: rebuild the sink from the start (the model is a pure fold, so this is exact).
    pub fn seek<S: Sink>(&mut self, sink: &mut S, t_ms: u64) {
        let t_ms = t_ms.min(self.session.duration_ms());
        sink.reset();
        let end = self.session.upto(t_ms);
        for r in &self.session.records[..end] {
            sink.feed(r);
        }
        self.cursor = end;
        self.pos_ms = t_ms as f64;
    }

    /// Advance by `dt_ms` of wall time (scaled by `speed`) while playing. Returns how many records were fed.
    pub fn tick<S: Sink>(&mut self, sink: &mut S, dt_ms: u64) -> usize {
        if !self.playing || self.at_end() {
            if self.at_end() {
                self.playing = false;
            }
            return 0;
        }
        self.pos_ms += dt_ms as f64 * self.speed.max(0.0);
        let end = self.session.upto(self.pos_ms as u64);
        let n = end.saturating_sub(self.cursor);
        for r in &self.session.records[self.cursor..end] {
            sink.feed(r);
        }
        self.cursor = end.max(self.cursor);
        n
    }
}
