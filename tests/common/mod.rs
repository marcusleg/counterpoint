//! Shared plumbing for the `harness = false` GTK test binaries: the pass/fail tally, the
//! display check, a watchdog, and widget-tree helpers.

#![allow(dead_code)]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;

/// How long a whole test binary may run before it is killed with a `FAIL:` line, so a hung
/// main loop never stalls `cargo test` silently.
const WATCHDOG: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct Checks {
    failures: usize,
    passed: usize,
}

impl Checks {
    pub fn check(&mut self, ok: bool, what: &str) {
        if ok {
            self.passed += 1;
        } else {
            self.failures += 1;
            println!("FAIL: {what}");
        }
    }

    pub fn fail(&mut self, what: &str) {
        self.check(false, what);
    }

    /// Prints the tally and turns it into the process exit code.
    pub fn finish(&self) -> ExitCode {
        println!("{} passed, {} failed", self.passed, self.failures);
        if self.failures == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

/// Initialises GTK, or explains why the binary is skipped (or, under
/// `COUNTERPOINT_REQUIRE_DISPLAY`, failed) without a display. Also arms the watchdog and turns
/// off animations so dialogs appear at once.
pub fn init_or_skip() -> Result<(), ExitCode> {
    if gtk::init().is_err() {
        if std::env::var_os("COUNTERPOINT_REQUIRE_DISPLAY").is_some() {
            println!("FAIL: no display although COUNTERPOINT_REQUIRE_DISPLAY is set");
            return Err(ExitCode::FAILURE);
        }
        println!("SKIPPED: no display");
        return Err(ExitCode::SUCCESS);
    }
    sourceview5::init();
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }
    glib::timeout_add_once(WATCHDOG, || {
        println!(
            "FAIL: the test did not finish within {}s",
            WATCHDOG.as_secs()
        );
        println!("0 passed, 1 failed");
        std::process::exit(1);
    });
    Ok(())
}

/// Appends `widget` and every descendant, in tree order, to `out`.
pub fn collect_widgets(widget: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        collect_widgets(&c, out);
        child = c.next_sibling();
    }
}

/// Every widget under `root`, in tree order.
pub fn widgets_under(root: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    collect_widgets(root.upcast_ref(), &mut out);
    out
}

/// The first button under `root` with exactly this label (including a mnemonic underscore).
pub fn find_button(root: &impl IsA<gtk::Widget>, label: &str) -> Option<gtk::Button> {
    widgets_under(root)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Button>().ok())
        .find(|b| b.label().as_deref() == Some(label))
}

/// The first label under `root` whose text satisfies `matches`.
pub fn find_label(
    root: &impl IsA<gtk::Widget>,
    matches: impl Fn(&str) -> bool,
) -> Option<gtk::Label> {
    widgets_under(root)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Label>().ok())
        .find(|l| matches(&l.text()))
}

/// The alert dialog currently shown under `root`, if any.
pub fn find_alert(root: &impl IsA<gtk::Widget>) -> Option<adw::AlertDialog> {
    widgets_under(root)
        .into_iter()
        .find_map(|w| w.downcast::<adw::AlertDialog>().ok())
}

/// Runs the main loop until `done` holds, or fails the check named `what` after `timeout`.
pub fn pump_until(
    checks: &mut Checks,
    what: &str,
    timeout: Duration,
    mut done: impl FnMut() -> bool,
) -> bool {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + timeout;
    loop {
        while context.iteration(false) {}
        if done() {
            return true;
        }
        if Instant::now() > deadline {
            checks.fail(&format!("{what} (timed out after {}s)", timeout.as_secs()));
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs every pending main-loop source once.
pub fn pump() {
    let context = glib::MainContext::default();
    while context.iteration(false) {}
}
