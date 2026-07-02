//! Presentation layer for the `midds` CLI.
//!
//! Everything that makes the tool feel like a modern installer (bun / the
//! Claude Code installer) lives here: the banner, a tuned `dialoguer` theme,
//! section / step headers, status lines, the summary table and the
//! `indicatif`-backed spinners and progress bars.
//!
//! # stdout vs stderr
//!
//! All *chrome* (banner, prompts, headers, spinners, summaries) is written to
//! **stderr**. Only machine-consumable output — the SCALE hex and the JSON
//! payload produced by `create` — goes to **stdout**. That split is what lets
//! `midds create --type musical-work … > work.json` stay pipe-clean while the
//! human still sees the full styled walkthrough on the terminal.

use std::time::Duration;

use console::{Emoji, Style, style};
use dialoguer::theme::ColorfulTheme;
use indicatif::{ProgressBar, ProgressStyle};

/// Inner width (characters) of the banner / payload panels. Wide enough for a
/// 130-char SCALE hex line to wrap cleanly without dominating an 80-col term.
const PANEL_WIDTH: usize = 62;

static DIAMOND: Emoji<'static, 'static> = Emoji("◆ ", "* ");
static CHECK: Emoji<'static, 'static> = Emoji("✓ ", "+ ");
static CROSS: Emoji<'static, 'static> = Emoji("✗ ", "x ");
static INFO: Emoji<'static, 'static> = Emoji("• ", "- ");
static ARROW: Emoji<'static, 'static> = Emoji("› ", "> ");

/// Print the startup banner to stderr. Called once by the no-argument
/// interactive entry point and by every wizard so a piped sub-invocation
/// still identifies itself.
pub fn banner() {
    let title = style("midds").cyan().bold();
    let tagline = style("Allfeat MIDDS operator toolkit").dim();
    let version = style(format!("v{}", env!("CARGO_PKG_VERSION"))).dim();
    let bar = style("━".repeat(PANEL_WIDTH)).cyan();

    eprintln!();
    eprintln!("  {bar}");
    eprintln!("  {DIAMOND}{title}  {tagline}");
    eprintln!("  {version}  ·  create · seed · bench");
    eprintln!("  {bar}");
    eprintln!();
}

/// The shared `dialoguer` theme. A light touch over `ColorfulTheme::default()`
/// — colored prefixes that match the rest of the chrome — so every `Input` /
/// `Select` / `Confirm` across the CLI reads as one coherent UI.
pub fn theme() -> ColorfulTheme {
    ColorfulTheme {
        prompt_prefix: style("?".to_string()).cyan().bold(),
        success_prefix: style("✓".to_string()).green().bold(),
        error_prefix: style("✗".to_string()).red().bold(),
        active_item_prefix: style("❯".to_string()).cyan().bold(),
        prompt_suffix: style("›".to_string()).dim(),
        success_suffix: style("·".to_string()).dim(),
        values_style: Style::new().cyan(),
        hint_style: Style::new().dim(),
        ..ColorfulTheme::default()
    }
}

/// A top-level section header (`◆ Title` + rule). Use to open a logical block
/// of the wizard ("Identification", "Creators", …).
pub fn section(title: &str) {
    eprintln!();
    eprintln!("  {}{}", style(DIAMOND).cyan(), style(title).bold());
    eprintln!("  {}", style("─".repeat(PANEL_WIDTH)).dim());
}

/// A `Step n/total · Title` marker, shown before a wizard's question block so
/// the user always knows how far through the form they are.
pub fn step(idx: usize, total: usize, title: &str) {
    let counter = style(format!("Step {idx}/{total}")).cyan().bold();
    eprintln!();
    eprintln!("  {counter} {} {}", style("·").dim(), style(title).bold());
}

/// Green success line.
pub fn success(msg: &str) {
    eprintln!("  {}{}", style(CHECK).green(), style(msg).green());
}

/// Red failure line. (`error` is a `Result` idiom elsewhere; the `_line`
/// suffix keeps this unambiguous at call sites.)
pub fn error_line(msg: &str) {
    eprintln!("  {}{}", style(CROSS).red(), style(msg).red());
}

/// Dim informational line.
pub fn info(msg: &str) {
    eprintln!("  {}{}", style(INFO).blue(), style(msg).dim());
}

/// Cyan hint / next-action line.
pub fn hint(msg: &str) {
    eprintln!("  {}{}", style(ARROW).cyan(), style(msg).cyan());
}

/// Render an aligned `key … value` summary block to stderr — the recap of a
/// freshly built MIDDS before its payload is emitted.
pub fn summary(title: &str, rows: &[(String, String)]) {
    section(title);
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (k, v) in rows {
        let key = format!("{k:<width$}");
        eprintln!(
            "  {}  {}  {}",
            style(key).dim(),
            style("┊").dim(),
            style(v).white(),
        );
    }
}

/// Emit a generated payload. The styled framing (`title`, rule) is written to
/// stderr; `body` is written **raw to stdout** so the value survives a pipe or
/// `>` redirect untouched.
pub fn payload_panel(title: &str, body: &str) {
    eprintln!();
    eprintln!("  {} {}", style("▍").cyan(), style(title).cyan().bold());
    eprintln!("  {}", style("─".repeat(PANEL_WIDTH)).dim());
    println!("{body}");
}

/// An indeterminate spinner with a steady 80 ms tick, drawn on stderr.
/// `finish_with`/`abandon` it when the work it covers is done.
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .expect("static spinner template is valid")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// A determinate progress bar (used by the bench/seed loops). Auto-hides when
/// stderr is not a TTY, so CI logs stay clean.
pub fn bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.cyan} [{bar:28.cyan/blue}] {pos}/{len} ({percent}%) \
             {per_sec}, eta {eta} · {msg}",
        )
        .expect("static bar template is valid")
        .progress_chars("━━╾─")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]),
    );
    pb
}
