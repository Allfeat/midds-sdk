//! `indicatif`-backed progress for the `bench` / `seed` deposit loops.
//!
//! Long-running deposit loops (one finalisation per record, or hundreds of
//! batches per signer) take many minutes against a real chain. Workers feed
//! completion events into [`ProgressPrinter`], which drives a single
//! `indicatif` bar (rate / ETA / counts handled by the bar's own template
//! and draw throttling). Out-of-band notices go through
//! [`ProgressPrinter::log`] so they print *above* the bar instead of
//! corrupting it.

use indicatif::ProgressBar;

use crate::ui;

pub struct ProgressPrinter {
    /// `None` when `total == 0` (nothing to render). `indicatif` itself hides
    /// the bar when stderr is not a TTY, so callers never special-case
    /// non-interactive runs.
    bar: Option<ProgressBar>,
}

impl ProgressPrinter {
    pub fn new(total: u32) -> Self {
        let bar = (total > 0).then(|| {
            let pb = ui::bar(total as u64);
            pb.set_message("ok=0 failed=0");
            pb
        });
        Self { bar }
    }

    /// Update the bar to `processed`/total with the running ok/failed split.
    /// Rate and ETA are derived by `indicatif` from its own position history,
    /// so no run-start [`std::time::Instant`] is threaded through here.
    pub fn tick(&mut self, processed: u32, ok: u32, failed: u32) {
        if let Some(pb) = &self.bar {
            pb.set_position(processed as u64);
            pb.set_message(format!("ok={ok} failed={failed}"));
        }
    }

    /// Emit an out-of-band line. With a live bar `indicatif` prints it above
    /// the bar and redraws cleanly; without one it falls back to stderr.
    /// Replaces the old `clear(); eprintln!` pair at the call sites.
    pub fn log(&self, msg: &str) {
        match &self.bar {
            Some(pb) => pb.println(format!("  {msg}")),
            None => eprintln!("  {msg}"),
        }
    }

    /// Terminate the bar (clearing its line) so the regular `done: …`
    /// summary the caller prints next lands on a fresh row.
    pub fn finish(&mut self) {
        if let Some(pb) = self.bar.take() {
            pb.finish_and_clear();
        }
    }
}
