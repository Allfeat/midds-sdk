//! Throttled in-place progress line shared by `bench fees` and `seed`.
//!
//! Long-running deposit loops (one finalisation per record, or hundreds of
//! batches per signer) can take many minutes against a real chain. Without
//! periodic feedback the run looks frozen, so workers feed completion
//! events into [`ProgressPrinter`] which renders an updating single-line
//! summary on stderr. Refresh is gated on a minimum interval so a fast
//! loop doesn't flood the terminal; the final state is always emitted via
//! [`ProgressPrinter::finish`] regardless of timing.

use std::{
    io::{self, Write as _},
    time::{Duration, Instant},
};

pub struct ProgressPrinter {
    total: u32,
    last_refresh: Option<Instant>,
    refresh_interval: Duration,
    last_line_len: usize,
    enabled: bool,
}

impl ProgressPrinter {
    pub fn new(total: u32) -> Self {
        Self {
            total,
            last_refresh: None,
            refresh_interval: Duration::from_millis(500),
            last_line_len: 0,
            enabled: total > 0,
        }
    }

    /// Render a refresh if the throttle interval has elapsed *or* the run
    /// has reached `total`. Cheap to call from a hot loop.
    pub fn tick(&mut self, processed: u32, ok: u32, failed: u32, started: Instant) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let due = match self.last_refresh {
            None => true,
            Some(t) => now.duration_since(t) >= self.refresh_interval,
        };
        let last = processed == self.total;
        if !(due || last) {
            return;
        }
        self.last_refresh = Some(now);
        self.render(processed, ok, failed, started.elapsed());
    }

    fn render(&mut self, processed: u32, ok: u32, failed: u32, elapsed: Duration) {
        let total = self.total;
        let pct = (processed as f64 / total as f64) * 100.0;
        let rate = if elapsed.as_secs_f64() > 0.0 {
            processed as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };
        let remaining = total.saturating_sub(processed);
        let eta = if rate > 0.0 {
            format_duration(Duration::from_secs_f64(remaining as f64 / rate))
        } else {
            "?".to_string()
        };
        let line = format!(
            "  progress: {processed}/{total} ({pct:.1}%) ok={ok} failed={failed} \
             rate={rate:.2}/s elapsed={} eta={eta}",
            format_duration(elapsed),
        );
        let pad = self.last_line_len.saturating_sub(line.len());
        let mut stderr = io::stderr().lock();
        let _ = write!(stderr, "\r{line}{:pad$}", "", pad = pad);
        let _ = stderr.flush();
        self.last_line_len = line.len();
    }

    /// Wipe the current progress line so the next `eprintln!` (e.g. an
    /// error log) doesn't get appended to a partial line.
    pub fn clear(&mut self) {
        if !self.enabled || self.last_line_len == 0 {
            return;
        }
        let mut stderr = io::stderr().lock();
        let _ = write!(stderr, "\r{:width$}\r", "", width = self.last_line_len);
        let _ = stderr.flush();
        self.last_line_len = 0;
    }

    /// Terminate the in-place progress line with a newline so the regular
    /// `done: ...` summary appears on a fresh row.
    pub fn finish(&mut self) {
        if !self.enabled || self.last_line_len == 0 {
            return;
        }
        let mut stderr = io::stderr().lock();
        let _ = writeln!(stderr);
        let _ = stderr.flush();
        self.last_line_len = 0;
    }
}

pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let h = secs / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}
