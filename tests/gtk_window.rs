//! GTK checks for the main window built through the public API: the chat input enabling Send,
//! the mode toggle, the selection chip and (once added) the editor placeholder. GTK must run on
//! the thread that initialised it and only one `gtk::Application` may run per process, so this
//! test builds the real window inside `connect_activate`, drives it with
//! `glib::idle_add_local_once` once it is realized, and quits the application afterwards.
//! It needs a display; `dev/headless.sh cargo test` provides a private one and sets
//! `COUNTERPOINT_REQUIRE_DISPLAY` so a missing display fails loudly instead of skipping silently.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use counterpoint::ui::window::MainWindow;

const NO_SELECTION: &str = "No selection — whole document";

#[derive(Default)]
struct Checks {
    failures: usize,
    passed: usize,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str) {
        if ok {
            self.passed += 1;
        } else {
            self.failures += 1;
            println!("FAIL: {what}");
        }
    }
}

fn main() -> ExitCode {
    if gtk::init().is_err() {
        if std::env::var_os("COUNTERPOINT_REQUIRE_DISPLAY").is_some() {
            println!("FAIL: no display although COUNTERPOINT_REQUIRE_DISPLAY is set");
            return ExitCode::FAILURE;
        }
        println!("SKIPPED: no display");
        return ExitCode::SUCCESS;
    }

    let app = adw::Application::builder()
        .application_id("de.marcusleg.Counterpoint.Test")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| sourceview5::init());

    let checks = Rc::new(RefCell::new(Checks::default()));
    app.connect_activate(glib::clone!(
        #[strong]
        checks,
        move |app| {
            let window = MainWindow::new(app);
            window.present();
            let checks = Rc::clone(&checks);
            let app = app.clone();
            glib::idle_add_local_once(move || {
                run_checks(&app, &mut checks.borrow_mut());
                app.quit();
            });
        }
    ));
    app.run_with_args::<&str>(&[]);

    let checks = checks.borrow();
    println!("{} passed, {} failed", checks.passed, checks.failures);
    if checks.failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Appends `widget` and every descendant, in tree order, to `out`.
fn collect_widgets(widget: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        collect_widgets(&c, out);
        child = c.next_sibling();
    }
}

fn run_checks(app: &adw::Application, checks: &mut Checks) {
    let root = app
        .active_window()
        .expect("the window is presented")
        .upcast::<gtk::Widget>();
    let mut widgets = Vec::new();
    collect_widgets(&root, &mut widgets);

    // The chat input is a plain `gtk::TextView`; an exact type check excludes the
    // `sourceview5::View` editor, which is also a `gtk::TextView` subclass.
    let input = widgets
        .iter()
        .find(|w| w.type_() == gtk::TextView::static_type())
        .expect("chat input")
        .clone()
        .downcast::<gtk::TextView>()
        .expect("chat input is a gtk::TextView");
    let send = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::Button>())
        .find(|b| b.label().as_deref() == Some("Send"))
        .expect("Send button")
        .clone();
    let placeholder = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::Label>())
        .find(|l| l.text().starts_with("Ask "))
        .expect("chat input placeholder")
        .clone();
    let chip = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::Label>())
        .find(|l| l.text() == NO_SELECTION)
        .expect("selection chip")
        .clone();
    let editor = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<sourceview5::View>())
        .expect("editor view")
        .clone();
    let mode = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<adw::ToggleGroup>())
        .expect("mode toggle")
        .clone();

    checks.check(!send.is_sensitive(), "Send starts insensitive");

    input.buffer().set_text("Hello");
    checks.check(
        send.is_sensitive(),
        "Send becomes sensitive once the chat input has text",
    );
    checks.check(
        !placeholder.is_visible(),
        "the chat placeholder hides once the chat input has text",
    );

    mode.set_active_name(Some("ghostwriting"));
    checks.check(
        placeholder.text().starts_with("Ask for a change"),
        "the placeholder switches with the mode",
    );

    let buffer = editor.buffer();
    buffer.set_text("Some example text.");
    let start = buffer.iter_at_offset(0);
    let end = buffer.iter_at_offset(4);
    buffer.select_range(&start, &end);
    checks.check(
        chip.text() == "Selection: “Some”",
        "the selection chip shows the trimmed selection",
    );
}
