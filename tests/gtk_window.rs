//! GTK checks for the main window built through the public API: the chat pane, the primary menu
//! and zoom controls, the unsaved-changes and file dialogs, saving, Open Recent, a chat round
//! trip against a mock endpoint, and the chat history. GTK must run on the thread that initialised it and only one
//! `gtk::Application` may run per process, so this test builds the real window inside
//! `connect_activate`, drives it with `glib::idle_add_local_once` once it is realized, and quits
//! the application afterwards. It needs a display; `dev/headless.sh cargo test` provides a
//! private one and sets `COUNTERPOINT_REQUIRE_DISPLAY` so a missing display fails loudly instead
//! of skipping silently.

mod common;

use std::cell::RefCell;
use std::fs;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use adw::prelude::*;
use gtk::{gio, glib};

use common::{find_alert, find_button, find_label, pump, pump_until, widgets_under, Checks};
use counterpoint::chat_history::{self, ChatHistory};
use counterpoint::config::Config;
use counterpoint::state::{self, State};
use counterpoint::ui::window::MainWindow;

const NO_SELECTION: &str = "No selection — whole document";
const EMPTY_EDITOR_HINT: &str = "Open a Markdown file or start writing…";
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
const DIALOG_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    // Safe: the process is still single-threaded here, before `gtk::init()` below can start any
    // GTK-owned threads. A fresh temp directory keeps this test from ever touching the user's
    // real state, settings and chat history files; it stays alive for the whole test.
    let home = tempfile::tempdir().expect("temp dir for the XDG directories");
    unsafe {
        std::env::set_var("XDG_STATE_HOME", home.path());
        std::env::set_var("XDG_CONFIG_HOME", home.path());
        std::env::set_var("XDG_DATA_HOME", home.path());
    }

    if let Err(code) = common::init_or_skip() {
        return code;
    }

    let app = adw::Application::builder()
        .application_id("de.marcusleg.Counterpoint.Test")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let checks = Rc::new(RefCell::new(Checks::default()));
    let work_dir = home.path().join("work");
    fs::create_dir_all(&work_dir).unwrap();
    app.connect_activate(glib::clone!(
        #[strong]
        checks,
        #[strong]
        work_dir,
        move |app| {
            let window = MainWindow::new(app);
            window.present();
            let checks = Rc::clone(&checks);
            let app = app.clone();
            let work_dir = work_dir.clone();
            glib::idle_add_local_once(move || {
                let mut checks = checks.borrow_mut();
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    run_checks(&app, &window, &work_dir, &mut checks)
                }));
                if let Err(payload) = result {
                    let message = payload
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".to_string());
                    checks.fail(&format!("the checks panicked: {message}"));
                }
                app.quit();
            });
        }
    ));
    app.run_with_args::<&str>(&[]);

    let checks = checks.borrow();
    checks.finish()
}

/// The labels of every item in every "section" link of `model`, in order.
fn menu_labels(model: &gio::MenuModel) -> Vec<String> {
    let mut labels = Vec::new();
    for i in 0..model.n_items() {
        let Some(section) = model.item_link(i, "section") else {
            continue;
        };
        for j in 0..section.n_items() {
            if let Some(value) = section.item_attribute_value(j, "label", None) {
                labels.push(value.str().unwrap_or_default().to_string());
            }
        }
    }
    labels
}

/// The primary menu's model.
fn primary_menu_model(root: &gtk::Widget) -> gio::MenuModel {
    widgets_under(root)
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::MenuButton>())
        .find(|b| b.icon_name().as_deref() == Some("open-menu-symbolic"))
        .and_then(|b| b.popover())
        .and_downcast::<gtk::PopoverMenu>()
        .and_then(|popover| popover.menu_model())
        .expect("the primary menu has a model")
}

/// The submenu behind **Open Recent** in the primary menu.
fn recent_submenu(root: &gtk::Widget) -> gio::MenuModel {
    let model = primary_menu_model(root);
    (0..model.n_items())
        .filter_map(|i| model.item_link(i, "section"))
        .find_map(|section| {
            (0..section.n_items()).find_map(|j| {
                let label = section.item_attribute_value(j, "label", None)?;
                (label.str() == Some("Open _Recent"))
                    .then(|| section.item_link(j, "submenu"))
                    .flatten()
            })
        })
        .expect("the primary menu has an Open Recent submenu")
}

/// (label, action, target) of every entry in `menu`.
fn menu_entries(menu: &gio::MenuModel) -> Vec<(String, String, Option<glib::Variant>)> {
    (0..menu.n_items())
        .map(|i| {
            let string = |name| {
                menu.item_attribute_value(i, name, None)
                    .and_then(|v| v.str().map(str::to_string))
                    .unwrap_or_default()
            };
            (
                string("label"),
                string("action"),
                menu.item_attribute_value(i, "target", None),
            )
        })
        .collect()
}

/// Activates the **Open Recent** entry whose label starts with `name`.
fn activate_recent(window: &gtk::Window, root: &gtk::Widget, name: &str) {
    let (_, action, target) = menu_entries(&recent_submenu(root))
        .into_iter()
        .find(|(label, _, _)| label.starts_with(name))
        .unwrap_or_else(|| panic!("an Open Recent entry for {name}"));
    window
        .activate_action(&action, target.as_ref())
        .expect("the Open Recent action exists");
}

/// The rows of the chat list with the given CSS class.
fn chat_rows(root: &gtk::Widget, class: &str) -> Vec<gtk::ListBoxRow> {
    widgets_under(root)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::ListBoxRow>().ok())
        .filter(|row| row.has_css_class(class))
        .collect()
}

fn run_checks(app: &adw::Application, main: &Rc<MainWindow>, work_dir: &Path, checks: &mut Checks) {
    let window = app.active_window().expect("the window is presented");
    let app_window = window
        .clone()
        .downcast::<adw::ApplicationWindow>()
        .expect("the active window is an AdwApplicationWindow");
    let root = window.clone().upcast::<gtk::Widget>();
    let widgets = widgets_under(&root);

    // The chat input is a plain `gtk::TextView`; an exact type check excludes the
    // `sourceview5::View` editor, which is also a `gtk::TextView` subclass.
    let input = widgets
        .iter()
        .find(|w| w.type_() == gtk::TextView::static_type())
        .expect("chat input")
        .clone()
        .downcast::<gtk::TextView>()
        .expect("chat input is a gtk::TextView");
    let send = find_button(&root, "_Send").expect("Send button");
    let stop = find_button(&root, "_Stop").expect("Stop button");
    let placeholder = find_label(&root, |t| t.starts_with("Ask ")).expect("chat input placeholder");
    let chip = find_label(&root, |t| t == NO_SELECTION).expect("selection chip");
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
    let editor_placeholder =
        find_label(&root, |t| t == EMPTY_EDITOR_HINT).expect("editor placeholder");
    let banner = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<adw::Banner>())
        .expect("config banner")
        .clone();
    let empty_state = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<adw::StatusPage>())
        .expect("chat empty state")
        .clone();
    let paned = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<gtk::Paned>())
        .expect("paned beside the editor")
        .clone();

    checks.check(!send.is_sensitive(), "Send starts insensitive");
    checks.check(!stop.is_visible(), "Stop is hidden while idle");
    checks.check(
        editor_placeholder.is_visible(),
        "the editor placeholder is visible for the empty editor",
    );
    checks.check(
        banner.is_revealed() && banner.title() == "No model configured",
        "without settings, the chat pane asks for a model",
    );
    checks.check(
        empty_state.is_mapped() && empty_state.title() == "Sparring",
        "an empty conversation shows the Sparring empty state",
    );
    checks.check(
        empty_state
            .description()
            .is_some_and(|d| d.contains("Highlight")),
        "the empty state explains highlighting",
    );

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
    checks.check(
        empty_state.title() == "Ghostwriting",
        "the empty state switches with the mode",
    );
    checks.check(
        mode.ancestor(adw::HeaderBar::static_type()).is_none(),
        "the mode toggle lives in the chat pane, not the header bar",
    );
    mode.set_active_name(Some("sparring"));

    let buffer = editor.buffer();
    buffer.set_text("Some example text.");
    checks.check(
        !editor_placeholder.is_visible(),
        "the editor placeholder hides once the editor has text",
    );
    let start = buffer.iter_at_offset(0);
    let end = buffer.iter_at_offset(4);
    buffer.select_range(&start, &end);
    checks.check(
        chip.text() == "Selection: “Some”",
        "the selection chip shows the trimmed selection",
    );

    header_bar_and_menu(checks, &root, &widgets);
    zoom(checks, &window, &app_window, &root);
    find(checks, &window, &root, &editor);

    chat_pane(checks, &window, &paned, &input);

    new_document(checks, &window, &root, &buffer);
    undo_redo(checks, &window, &root, &buffer);
    unsaved_changes_dialog(checks, &window, &root, &buffer);
    files(checks, main, &window, &root, &buffer, work_dir);
    recent_files(checks, main, &window, &root, &buffer, work_dir);
    chat(
        checks, &window, &root, &input, &send, &stop, &mode, &buffer, &banner,
    );
    chat_history(checks, app, main, &window, &root, &buffer, work_dir);

    let second = MainWindow::new(app);
    let second_window = app
        .windows()
        .into_iter()
        .find(|w| *w != window)
        .expect("a second window was created");
    let second_zoom_label = find_button(&second_window, "110%");
    checks.check(
        second_zoom_label.is_some(),
        "a new window starts at the persisted zoom level",
    );
    drop(second);
    second_window.destroy();

    window_size(checks, app, &window);
}

fn header_bar_and_menu(checks: &mut Checks, root: &gtk::Widget, widgets: &[gtk::Widget]) {
    let menu_button = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::MenuButton>())
        .find(|b| b.icon_name().as_deref() == Some("open-menu-symbolic"))
        .expect("primary menu button")
        .clone();
    let popover = menu_button
        .popover()
        .and_downcast::<gtk::PopoverMenu>()
        .expect("the primary menu is a popover menu");
    let menu_model = popover.menu_model().expect("primary menu has a model");
    let expected_labels = [
        "_New".to_string(),
        "_Open…".to_string(),
        "Open _Recent".to_string(),
        "_Save".to_string(),
        "Save _As…".to_string(),
        "_Find…".to_string(),
        "_Preferences".to_string(),
        "_Keyboard Shortcuts".to_string(),
        "_About Counterpoint".to_string(),
    ];
    checks.check(
        menu_labels(&menu_model) == expected_labels,
        "the primary menu follows the GNOME HIG and has no Quit item",
    );
    checks.check(
        recent_submenu(root).n_items() == 0,
        "Open Recent is shown with an empty list before any file was opened",
    );

    let zoom_box = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::Box>())
        .find(|b| b.has_css_class("zoom-controls"))
        .expect("zoom controls box")
        .clone();
    checks.check(
        zoom_box.ancestor(gtk::PopoverMenu::static_type()).is_some(),
        "the zoom controls live inside the primary menu",
    );

    let icon_button = |icon: &str| {
        widgets
            .iter()
            .filter_map(|w| w.downcast_ref::<gtk::Button>())
            .find(|b| b.icon_name().as_deref() == Some(icon))
            .cloned()
    };
    let undo = icon_button("edit-undo-symbolic").expect("Undo button");
    let redo = icon_button("edit-redo-symbolic").expect("Redo button");
    checks.check(
        find_button(root, "_Open…").is_none(),
        "the header bar leaves Open to the primary menu",
    );
    let title_widget = widgets
        .iter()
        .find(|w| w.type_() == adw::WindowTitle::static_type())
        .expect("window title")
        .clone();
    let chat_toggle = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::ToggleButton>())
        .find(|b| b.icon_name().as_deref() == Some("sidebar-show-right-symbolic"))
        .expect("chat toggle button")
        .clone();
    let position = |widget: &gtk::Widget| widgets.iter().position(|w| w == widget);
    let undo_index = position(undo.upcast_ref());
    let redo_index = position(redo.upcast_ref());
    let title_index = position(&title_widget);
    let toggle_index = position(chat_toggle.upcast_ref());
    let menu_index = position(menu_button.upcast_ref());
    checks.check(
        undo.ancestor(adw::HeaderBar::static_type()).is_some()
            && undo_index < redo_index
            && redo_index < title_index
            && title_index < toggle_index
            && toggle_index < menu_index,
        "the header bar packs Undo and Redo at the start and the chat toggle before the primary menu",
    );
    checks.check(
        chat_toggle.is_active(),
        "the chat toggle reflects the shown chat pane",
    );
}

fn chat_pane(checks: &mut Checks, window: &gtk::Window, paned: &gtk::Paned, input: &gtk::TextView) {
    let chat_pane = paned
        .end_child()
        .expect("the chat is the paned's end child");
    checks.check(
        input.is_ancestor(&chat_pane),
        "the chat input is in the paned's end child",
    );
    checks.check(chat_pane.is_visible(), "the chat is shown at first");
    if !pump_until(
        checks,
        "the chat is laid out at its default width and may then be dragged narrower",
        DIALOG_TIMEOUT,
        || paned.is_position_set() && chat_pane.width_request() == 300,
    ) {
        return;
    }
    checks.check(
        chat_pane.width() == 360,
        &format!("the chat starts 360 pixels wide, not {}", chat_pane.width()),
    );

    paned.set_position(paned.position() - 100);
    pump_until(
        checks,
        "dragging the divider left widens the chat",
        DIALOG_TIMEOUT,
        || chat_pane.width() == 460,
    );
    paned.set_position(paned.max_position() + 100);
    pump_until(
        checks,
        "dragging the divider far right stops at the chat's minimum width",
        DIALOG_TIMEOUT,
        || chat_pane.width() == 300,
    );
    paned.set_position(paned.position() - 120);
    pump_until(
        checks,
        "dragging the divider back widens the chat again",
        DIALOG_TIMEOUT,
        || chat_pane.width() == 420,
    );

    window
        .activate_action("win.toggle-chat", None)
        .expect("win.toggle-chat exists");
    checks.check(!chat_pane.is_visible(), "toggle-chat hides the chat");
    let state_path = state::state_path().expect("state path resolves under XDG_STATE_HOME");
    checks.check(
        State::load_from(&state_path).chat_width == Some(420),
        "hiding the chat persists its width to the state file",
    );
    window
        .activate_action("win.toggle-chat", None)
        .expect("win.toggle-chat exists");
    checks.check(chat_pane.is_visible(), "toggle-chat shows the chat again");
    pump_until(
        checks,
        "the chat comes back at the width it was hidden at",
        DIALOG_TIMEOUT,
        || chat_pane.width() == 420,
    );
}

fn zoom(
    checks: &mut Checks,
    window: &gtk::Window,
    app_window: &adw::ApplicationWindow,
    root: &gtk::Widget,
) {
    let zoom_label = find_button(root, "100%").expect("zoom label button");
    checks.check(
        zoom_label.label().as_deref() == Some("100%"),
        "zoom starts at 100%",
    );

    window
        .activate_action("win.zoom-in", None)
        .expect("win.zoom-in exists");
    checks.check(
        zoom_label.label().as_deref() == Some("110%"),
        "zoom-in increases the percentage by one step",
    );

    window
        .activate_action("win.zoom-reset", None)
        .expect("win.zoom-reset exists");
    checks.check(
        zoom_label.label().as_deref() == Some("100%"),
        "zoom-reset returns to 100%",
    );

    for _ in 0..5 {
        window
            .activate_action("win.zoom-out", None)
            .expect("win.zoom-out exists");
    }
    checks.check(
        zoom_label.label().as_deref() == Some("50%"),
        "zoom-out clamps at the minimum",
    );
    let zoom_out_action = app_window
        .lookup_action("zoom-out")
        .and_downcast::<gio::SimpleAction>()
        .expect("win.zoom-out action");
    checks.check(
        !zoom_out_action.is_enabled(),
        "zoom-out disables itself at the minimum",
    );

    window
        .activate_action("win.zoom-reset", None)
        .expect("win.zoom-reset exists");
    checks.check(
        zoom_out_action.is_enabled(),
        "zoom-out re-enables once away from the minimum",
    );

    window
        .activate_action("win.zoom-in", None)
        .expect("win.zoom-in exists");
    checks.check(
        zoom_label.label().as_deref() == Some("110%"),
        "zoom-in increases to 110% again",
    );
    let state_path = state::state_path().expect("state path resolves under XDG_STATE_HOME");
    checks.check(
        State::load_from(&state_path).zoom == Some(110),
        "the zoom level is persisted to the state file",
    );
}

fn find(checks: &mut Checks, window: &gtk::Window, root: &gtk::Widget, editor: &sourceview5::View) {
    let bar = widgets_under(root)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::SearchBar>().ok())
        .expect("find bar");
    let entry = widgets_under(&bar)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::SearchEntry>().ok())
        .expect("find entry");
    let buffer = editor.buffer();
    let selected = || {
        buffer
            .selection_bounds()
            .map(|(start, end)| (start.offset(), end.offset()))
    };
    let count_shows = |text: &str| find_label(&bar, |t| t == text).is_some();
    buffer.set_text("One fish, two fish, red fish.\nBlue FISH.");
    buffer.place_cursor(&buffer.start_iter());
    checks.check(!bar.is_search_mode(), "the find bar starts closed");

    window
        .activate_action("win.find", None)
        .expect("win.find exists");
    checks.check(bar.is_search_mode(), "win.find opens the find bar");
    checks.check(
        GtkWindowExt::focus(window)
            .is_some_and(|focus| focus.is_ancestor(&entry) || focus == entry),
        "win.find focuses the find entry",
    );

    entry.set_text("fish");
    pump_until(
        checks,
        "typing selects the first match and counts all, ignoring case",
        DIALOG_TIMEOUT,
        || count_shows("1 of 4"),
    );
    checks.check(selected() == Some((4, 8)), "the first match is selected");

    window
        .activate_action("win.find-next", None)
        .expect("win.find-next exists");
    checks.check(
        selected() == Some((14, 18)) && count_shows("2 of 4"),
        "win.find-next selects the next match",
    );
    entry.emit_activate();
    checks.check(
        selected() == Some((24, 28)) && count_shows("3 of 4"),
        "Enter in the find entry selects the next match",
    );
    for _ in 0..3 {
        window
            .activate_action("win.find-previous", None)
            .expect("win.find-previous exists");
    }
    checks.check(
        selected() == Some((35, 39)) && count_shows("4 of 4"),
        "win.find-previous wraps around to the last match",
    );

    entry.set_text("whale");
    pump_until(
        checks,
        "a search text without matches says so",
        DIALOG_TIMEOUT,
        || count_shows("No matches"),
    );
    checks.check(
        entry.has_css_class("error"),
        "the find entry is marked when nothing matches",
    );

    bar.set_search_mode(false);
    checks.check(
        editor.has_focus(),
        "closing the find bar returns focus to the editor",
    );

    let start = buffer.iter_at_offset(20);
    let end = buffer.iter_at_offset(23);
    buffer.select_range(&start, &end);
    window
        .activate_action("win.find", None)
        .expect("win.find exists");
    checks.check(
        entry.text() == "red",
        "win.find searches for the selected text",
    );
    let start = buffer.iter_at_offset(0);
    let end = buffer.iter_at_offset(35);
    buffer.select_range(&start, &end);
    window
        .activate_action("win.find", None)
        .expect("win.find exists");
    checks.check(
        entry.text() == "red",
        "win.find keeps the search text when the selection spans lines",
    );
    bar.set_search_mode(false);
    pump();
}

fn new_document(
    checks: &mut Checks,
    window: &gtk::Window,
    root: &gtk::Widget,
    buffer: &gtk::TextBuffer,
) {
    // The unsaved-changes guard sees an unmodified buffer here, so it proceeds without an alert.
    buffer.set_modified(false);
    window
        .activate_action("win.new", None)
        .expect("win.new exists");
    pump_until(checks, "win.new empties the editor", DIALOG_TIMEOUT, || {
        buffer.char_count() == 0
    });
    checks.check(
        !buffer.is_modified(),
        "win.new leaves the buffer unmodified",
    );
    checks.check(!buffer.can_undo(), "win.new leaves nothing to undo");
    let title = widgets_under(root)
        .into_iter()
        .find_map(|w| w.downcast::<adw::WindowTitle>().ok())
        .expect("window title widget");
    checks.check(
        title.title() == "Untitled",
        "win.new resets the title to \"Untitled\"",
    );
}

fn undo_redo(
    checks: &mut Checks,
    window: &gtk::Window,
    root: &gtk::Widget,
    buffer: &gtk::TextBuffer,
) {
    let icon_button = |icon: &str| {
        widgets_under(root)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.icon_name().as_deref() == Some(icon))
    };
    let undo = icon_button("edit-undo-symbolic").expect("Undo button");
    let redo = icon_button("edit-redo-symbolic").expect("Redo button");
    checks.check(
        !undo.is_sensitive() && !redo.is_sensitive(),
        "Undo and Redo are greyed out with nothing to undo or redo",
    );

    buffer.insert_at_cursor("Draft");
    checks.check(
        undo.is_sensitive() && !redo.is_sensitive(),
        "an edit enables Undo",
    );
    window
        .activate_action("win.undo", None)
        .expect("win.undo exists");
    checks.check(buffer.char_count() == 0, "win.undo undoes the edit");
    checks.check(
        !undo.is_sensitive() && redo.is_sensitive(),
        "undoing the only edit greys out Undo and enables Redo",
    );
    window
        .activate_action("win.redo", None)
        .expect("win.redo exists");
    checks.check(
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .as_str()
            == "Draft",
        "win.redo redoes the edit",
    );
    checks.check(
        undo.is_sensitive() && !redo.is_sensitive(),
        "redoing the edit enables Undo and greys out Redo",
    );
}

fn unsaved_changes_dialog(
    checks: &mut Checks,
    window: &gtk::Window,
    root: &gtk::Widget,
    buffer: &gtk::TextBuffer,
) {
    buffer.set_text("Unsaved work.");
    buffer.set_modified(true);
    window
        .activate_action("win.new", None)
        .expect("win.new exists");
    if !pump_until(
        checks,
        "win.new on a modified document asks",
        DIALOG_TIMEOUT,
        || find_alert(root).is_some(),
    ) {
        return;
    }
    let alert = find_alert(root).unwrap();
    checks.check(
        alert.heading().as_deref() == Some("Save Changes?"),
        "the alert heading follows the HIG",
    );
    checks.check(
        alert.body().contains("“Untitled”"),
        "the alert names the document",
    );
    // Asking twice while the alert is open must not open a second alert.
    window
        .activate_action("win.new", None)
        .expect("win.new exists");
    pump();
    let alerts = widgets_under(root)
        .into_iter()
        .filter(|w| w.is::<adw::AlertDialog>())
        .count();
    checks.check(alerts == 1, "a second request waits for the open alert");

    find_button(&alert, "_Cancel")
        .expect("Cancel response")
        .emit_clicked();
    pump_until(checks, "Cancel closes the alert", DIALOG_TIMEOUT, || {
        find_alert(root).is_none()
    });
    checks.check(
        buffer.is_modified() && buffer.char_count() > 0,
        "Cancel keeps the document",
    );

    window
        .activate_action("win.new", None)
        .expect("win.new exists");
    pump_until(checks, "win.new asks again", DIALOG_TIMEOUT, || {
        find_alert(root).is_some()
    });
    if let Some(alert) = find_alert(root) {
        find_button(&alert, "_Discard")
            .expect("Discard response")
            .emit_clicked();
    }
    pump_until(
        checks,
        "Discard starts a new document",
        DIALOG_TIMEOUT,
        || buffer.char_count() == 0 && !buffer.is_modified(),
    );
}

fn files(
    checks: &mut Checks,
    main: &Rc<MainWindow>,
    window: &gtk::Window,
    root: &gtk::Widget,
    buffer: &gtk::TextBuffer,
    work_dir: &Path,
) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crlf.md");
    let path = work_dir.join("crlf.md");
    fs::copy(&fixture, &path).unwrap();

    checks.check(main.load_path(&path).is_ok(), "load_path opens a file");
    let title = widgets_under(root)
        .into_iter()
        .find_map(|w| w.downcast::<adw::WindowTitle>().ok())
        .expect("window title widget");
    checks.check(title.title() == "crlf.md", "the title shows the file name");
    checks.check(
        title.subtitle() == work_dir.display().to_string(),
        "the subtitle shows the folder",
    );
    checks.check(
        !buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .contains('\r'),
        "CRLF files are edited with LF",
    );
    let state_path = state::state_path().unwrap();
    checks.check(
        State::load_from(&state_path).last_folder.as_deref() == Some(work_dir),
        "opening a file remembers its folder",
    );

    let end = buffer.end_iter();
    buffer.insert(&mut end.clone(), "Added line\n");
    checks.check(buffer.is_modified(), "typing marks the document modified");
    checks.check(
        title.title() == "• crlf.md",
        "the title carries the modified marker",
    );
    window
        .activate_action("win.save", None)
        .expect("win.save exists");
    pump_until(checks, "win.save writes the file", DIALOG_TIMEOUT, || {
        !buffer.is_modified()
    });
    let saved = fs::read(&path).unwrap();
    let text = String::from_utf8(saved).unwrap();
    checks.check(
        text.ends_with("Added line\r\n") && !text.replace("\r\n", "").contains('\n'),
        "the saved file keeps CRLF line endings throughout",
    );

    // Another program changes the file: saving must ask before overwriting it.
    fs::write(&path, "changed elsewhere\r\n").unwrap();
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(30))
        .unwrap();
    drop(file);
    buffer.insert(&mut buffer.end_iter(), "More\n");
    window
        .activate_action("win.save", None)
        .expect("win.save exists");
    if pump_until(
        checks,
        "saving over a changed file asks",
        DIALOG_TIMEOUT,
        || find_alert(root).is_some(),
    ) {
        let alert = find_alert(root).unwrap();
        checks.check(
            alert.heading().as_deref() == Some("Overwrite Changed File?"),
            "the alert explains the file changed on disk",
        );
        find_button(&alert, "_Cancel").unwrap().emit_clicked();
        pump_until(
            checks,
            "Cancel closes the overwrite alert",
            DIALOG_TIMEOUT,
            || find_alert(root).is_none(),
        );
        checks.check(
            buffer.is_modified() && fs::read_to_string(&path).unwrap() == "changed elsewhere\r\n",
            "Cancel leaves the file on disk alone",
        );
        window
            .activate_action("win.save", None)
            .expect("win.save exists");
        pump_until(checks, "saving asks again", DIALOG_TIMEOUT, || {
            find_alert(root).is_some()
        });
        if let Some(alert) = find_alert(root) {
            find_button(&alert, "_Overwrite").unwrap().emit_clicked();
        }
        pump_until(
            checks,
            "Overwrite saves the document",
            DIALOG_TIMEOUT,
            || !buffer.is_modified(),
        );
        checks.check(
            fs::read_to_string(&path).unwrap().ends_with("More\r\n"),
            "Overwrite writes the document",
        );
    }

    // A failing save reports the error and keeps the document modified.
    #[cfg(unix)]
    if std::env::var("USER") != Ok("root".to_string()) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(work_dir, fs::Permissions::from_mode(0o555)).unwrap();
        let read_only = fs::write(work_dir.join(".probe"), "x").is_err();
        if read_only {
            buffer.insert(&mut buffer.end_iter(), "Again\n");
            window
                .activate_action("win.save", None)
                .expect("win.save exists");
            if pump_until(
                checks,
                "a failing save shows an alert",
                DIALOG_TIMEOUT,
                || find_alert(root).is_some(),
            ) {
                let alert = find_alert(root).unwrap();
                checks.check(
                    alert.heading().as_deref() == Some("Could Not Save File"),
                    "the alert names the failure",
                );
                checks.check(
                    buffer.is_modified(),
                    "a failed save keeps the document modified",
                );
                find_button(&alert, "_OK").unwrap().emit_clicked();
                pump_until(checks, "OK closes the alert", DIALOG_TIMEOUT, || {
                    find_alert(root).is_none()
                });
            }
        }
        fs::set_permissions(work_dir, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_file(work_dir.join(".probe"));
    }

    checks.check(
        main.load_path(&work_dir.join("missing.md")).is_err(),
        "load_path reports a missing file",
    );
}

/// Open Recent, starting from where `files` left off: `crlf.md` is the only file opened so far.
/// Adding a file by Save As is not covered, since it needs the file chooser.
fn recent_files(
    checks: &mut Checks,
    main: &Rc<MainWindow>,
    window: &gtk::Window,
    root: &gtk::Widget,
    buffer: &gtk::TextBuffer,
    work_dir: &Path,
) {
    let recent_names = || -> Vec<String> {
        menu_entries(&recent_submenu(root))
            .into_iter()
            .map(|(label, _, _)| label)
            .collect()
    };
    let crlf = work_dir.join("crlf.md");
    let other = work_dir.join("other_post.md");
    fs::write(&other, "Other post.\n").unwrap();

    checks.check(
        recent_names() == ["crlf.md"],
        "a file opened earlier is listed under Open Recent, by its name",
    );

    main.load_path(&other).expect("the second file opens");
    checks.check(
        recent_names() == ["other__post.md", "crlf.md"],
        "the newest file comes first, with underscores escaped",
    );

    activate_recent(window, root, "crlf.md");
    let title = widgets_under(root)
        .into_iter()
        .find_map(|w| w.downcast::<adw::WindowTitle>().ok())
        .expect("window title widget");
    pump_until(
        checks,
        "activating an Open Recent entry opens that file",
        DIALOG_TIMEOUT,
        || title.title() == "crlf.md",
    );
    checks.check(
        !buffer.is_modified() && buffer.char_count() > 0,
        "the reopened file is loaded unmodified",
    );
    checks.check(
        recent_names() == ["crlf.md", "other__post.md"],
        "reopening a file moves it to the top without duplicating it",
    );

    fs::remove_file(&other).unwrap();
    activate_recent(window, root, "other__post.md");
    if pump_until(
        checks,
        "opening a missing recent file shows an alert",
        DIALOG_TIMEOUT,
        || find_alert(root).is_some(),
    ) {
        let alert = find_alert(root).unwrap();
        checks.check(
            alert.heading().as_deref() == Some("Could Not Open File"),
            "the alert explains the file could not be opened",
        );
        find_button(&alert, "_OK").unwrap().emit_clicked();
        pump_until(checks, "OK closes the alert", DIALOG_TIMEOUT, || {
            find_alert(root).is_none()
        });
    }
    checks.check(
        recent_names() == ["crlf.md"],
        "a recent file that can no longer be opened is dropped from the list",
    );
    let state_path = state::state_path().unwrap();
    checks.check(
        State::load_from(&state_path).recent_files == [crlf.clone()],
        "the recent files are persisted to the state file",
    );

    // Once the last recent file cannot be opened either, the list is empty again.
    let moved = work_dir.join("crlf.md.moved");
    fs::rename(&crlf, &moved).unwrap();
    activate_recent(window, root, "crlf.md");
    if pump_until(
        checks,
        "opening the last, missing recent file shows an alert",
        DIALOG_TIMEOUT,
        || find_alert(root).is_some(),
    ) {
        find_button(&find_alert(root).unwrap(), "_OK")
            .unwrap()
            .emit_clicked();
        pump_until(checks, "OK closes the alert", DIALOG_TIMEOUT, || {
            find_alert(root).is_none()
        });
    }
    checks.check(
        recent_submenu(root).n_items() == 0,
        "Open Recent keeps its submenu, now empty, once the last file is dropped",
    );
    fs::rename(&moved, &crlf).unwrap();
}

#[allow(clippy::too_many_arguments)]
fn chat(
    checks: &mut Checks,
    window: &gtk::Window,
    root: &gtk::Widget,
    input: &gtk::TextView,
    send: &gtk::Button,
    stop: &gtk::Button,
    mode: &adw::ToggleGroup,
    buffer: &gtk::TextBuffer,
    banner: &adw::Banner,
) {
    let mut server = mockito::Server::new();
    let config = Config {
        base_url: format!("{}/v1", server.url()),
        api_key: None,
        model: Some("test".to_string()),
    };
    config.save().expect("settings saved under XDG_CONFIG_HOME");
    server
        .mock("GET", "/v1/models")
        .with_body(r#"{"data":[{"id":"test"}]}"#)
        .create();

    // The banner goes away once Preferences closes with a model configured.
    window
        .activate_action("win.preferences", None)
        .expect("win.preferences exists");
    pump();
    let preferences = widgets_under(root)
        .into_iter()
        .filter_map(|w| w.downcast::<adw::Dialog>().ok())
        .find(|d| d.title() == "Preferences");
    checks.check(preferences.is_some(), "win.preferences opens the dialog");
    if let Some(dialog) = preferences {
        dialog.close();
    }
    pump_until(
        checks,
        "closing Preferences hides the banner",
        DIALOG_TIMEOUT,
        || !banner.is_revealed(),
    );

    buffer.set_text("# T\n\nOld text.\n");
    buffer.set_modified(false);
    let reply = |content: &str| {
        serde_json::json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
            .to_string()
    };

    // Sparring: a plain reply, rendered as Markdown.
    let sparring = server
        .mock("POST", "/v1/chat/completions")
        .with_body(reply("*Hello* there"))
        .create();
    input.buffer().set_text("Thoughts?");
    send.emit_clicked();
    checks.check(
        !send.is_visible() && stop.is_visible(),
        "Send gives way to Stop while waiting",
    );
    checks.check(
        find_label(root, |t| t == "Waiting for the LLM…").is_some(),
        "the busy label shows while waiting",
    );
    let chat_pane = widgets_under(root)
        .iter()
        .find_map(|w| w.downcast_ref::<gtk::Paned>())
        .and_then(|paned| paned.end_child())
        .expect("the chat pane");
    let (chat_min_width, ..) = chat_pane.measure(gtk::Orientation::Horizontal, -1);
    checks.check(
        chat_min_width == 300,
        &format!("waiting keeps the chat's minimum width at 300, not {chat_min_width}"),
    );
    checks.check(
        chat_rows(root, "chat-user").len() == 1,
        "the user's message appears at once",
    );
    pump_until(checks, "a sparring reply arrives", REPLY_TIMEOUT, || {
        !chat_rows(root, "chat-assistant").is_empty()
    });
    let assistant = chat_rows(root, "chat-assistant");
    let assistant_text = assistant
        .first()
        .and_then(|row| row.child())
        .and_downcast::<gtk::Label>()
        .map(|label| label.text().to_string());
    checks.check(
        assistant_text.as_deref() == Some("Hello there"),
        &format!("the reply is rendered from Markdown, got {assistant_text:?}"),
    );
    checks.check(
        send.is_visible() && !stop.is_visible(),
        "Send comes back once the reply arrived",
    );
    sparring.remove();

    // Ghostwriting: a proposal that applies as one undo step.
    mode.set_active_name(Some("ghostwriting"));
    let proposal = server
        .mock("POST", "/v1/chat/completions")
        .with_body(reply(
            "Shorter.\n\n```original\nOld text.\n```\n```replacement\nNew text.\n```",
        ))
        .create();
    input.buffer().set_text("Tighten it.");
    send.emit_clicked();
    pump_until(checks, "a proposal arrives", REPLY_TIMEOUT, || {
        !chat_rows(root, "chat-proposal").is_empty()
    });
    let card = chat_rows(root, "chat-proposal");
    let Some(card) = card.first() else {
        return;
    };
    checks.check(
        find_label(card, |t| t == "Proposed Change").is_some(),
        "the proposal card has its heading",
    );
    find_button(card, "_Apply")
        .expect("Apply button")
        .emit_clicked();
    pump();
    checks.check(
        buffer.text(&buffer.start_iter(), &buffer.end_iter(), true) == "# T\n\nNew text.\n",
        "Apply changes the document",
    );
    checks.check(
        find_label(root, |t| t == "✓ Applied").is_some(),
        "the card shows the proposal as applied",
    );
    buffer.undo();
    checks.check(
        buffer.text(&buffer.start_iter(), &buffer.end_iter(), true) == "# T\n\nOld text.\n"
            && !buffer.can_undo(),
        "an applied proposal is one undo step",
    );

    // A stale proposal (the text changed) fails with an explanation and stays pending.
    input.buffer().set_text("Again.");
    send.emit_clicked();
    pump_until(checks, "a second proposal arrives", REPLY_TIMEOUT, || {
        chat_rows(root, "chat-proposal").len() == 2
    });
    buffer.set_text("Something else entirely.");
    let cards = chat_rows(root, "chat-proposal");
    if let Some(card) = cards.get(1) {
        find_button(card, "_Apply")
            .expect("Apply button")
            .emit_clicked();
        pump();
        find_button(card, "_Apply")
            .expect("Apply button")
            .emit_clicked();
        pump();
    }
    let errors = chat_rows(root, "chat-error");
    let error_text = errors
        .first()
        .and_then(|row| row.child())
        .and_downcast::<gtk::Label>()
        .map(|l| l.text().to_string())
        .unwrap_or_default();
    checks.check(
        errors.len() == 1 && error_text.contains("Edit 1") && error_text.contains("not found"),
        &format!("a stale proposal explains itself once, got {error_text:?}"),
    );
    let cards = chat_rows(root, "chat-proposal");
    checks.check(
        cards
            .get(1)
            .is_some_and(|card| find_button(card, "_Reject").is_some()),
        "a stale proposal stays pending",
    );
    if let Some(card) = cards.get(1) {
        find_button(card, "_Reject").unwrap().emit_clicked();
        pump();
    }
    checks.check(
        find_label(root, |t| t == "✗ Rejected").is_some(),
        "Reject marks the proposal rejected",
    );
    proposal.remove();

    // In an empty document there is nothing to quote: a blank original writes the first draft.
    buffer.set_text("");
    let kickstart = server
        .mock("POST", "/v1/chat/completions")
        .with_body(reply(
            "A first draft.\n\n```original\n```\n```replacement\n# Draft\n\nOpening.\n```",
        ))
        .create();
    input.buffer().set_text("Kickstart a post.");
    send.emit_clicked();
    pump_until(
        checks,
        "a kickstart proposal arrives",
        REPLY_TIMEOUT,
        || chat_rows(root, "chat-proposal").len() == 3,
    );
    if let Some(card) = chat_rows(root, "chat-proposal").get(2) {
        checks.check(
            !widgets_under(card)
                .iter()
                .any(|w| w.has_css_class("edit-original")),
            "a blank original shows no empty before box",
        );
        find_button(card, "_Apply")
            .expect("Apply button")
            .emit_clicked();
        pump();
    }
    checks.check(
        buffer.text(&buffer.start_iter(), &buffer.end_iter(), true) == "# Draft\n\nOpening.",
        "Apply fills an empty document",
    );
    checks.check(
        chat_rows(root, "chat-error").len() == 1,
        "filling an empty document reports no error",
    );
    kickstart.remove();

    // An HTTP error becomes an error row and frees the input.
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(500)
        .with_body("boom")
        .create();
    input.buffer().set_text("Once more.");
    send.emit_clicked();
    pump_until(checks, "an HTTP error is reported", REPLY_TIMEOUT, || {
        chat_rows(root, "chat-error").len() == 2
    });
    let error_text = chat_rows(root, "chat-error")
        .get(1)
        .and_then(|row| row.child())
        .and_downcast::<gtk::Label>()
        .map(|l| l.text().to_string())
        .unwrap_or_default();
    checks.check(
        error_text.contains("HTTP 500") && error_text.contains("boom"),
        &format!("the error row names the status, got {error_text:?}"),
    );
    checks.check(send.is_visible(), "Send is back after an error");

    // Stop abandons a slow request and gives the message back.
    server.reset();
    server
        .mock("POST", "/v1/chat/completions")
        .with_chunked_body(|w| {
            std::thread::sleep(Duration::from_millis(1500));
            w.write_all(br#"{"choices":[{"message":{"content":"late"}}]}"#)
        })
        .create();
    let rows_before = chat_rows(root, "chat-user").len();
    input.buffer().set_text("Slow one");
    send.emit_clicked();
    pump();
    checks.check(stop.is_visible(), "Stop shows for the slow request");
    stop.emit_clicked();
    pump();
    let restored = input.buffer();
    checks.check(
        restored.text(&restored.start_iter(), &restored.end_iter(), false) == "Slow one",
        "Stop puts the message back into the input",
    );
    checks.check(
        chat_rows(root, "chat-user").len() == rows_before && send.is_visible(),
        "Stop removes the pending message and frees the input",
    );
    std::thread::sleep(Duration::from_millis(1800));
    pump();
    checks.check(
        chat_rows(root, "chat-assistant").len() == 1,
        "the late reply of a stopped request is discarded",
    );
}

/// The chat history, starting from where `chat` left off: one conversation about `crlf.md`.
fn chat_history(
    checks: &mut Checks,
    app: &adw::Application,
    main: &Rc<MainWindow>,
    window: &gtk::Window,
    root: &gtk::Widget,
    buffer: &gtk::TextBuffer,
    work_dir: &Path,
) {
    let crlf = work_dir.join("crlf.md");
    let list = widgets_under(root)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::DropDown>().ok())
        .expect("chat list");
    let labels = || -> Vec<String> {
        let model = list.model().expect("the chat list has a model");
        (0..model.n_items())
            .filter_map(|i| model.item(i).and_downcast::<gtk::StringObject>())
            .map(|item| item.string().to_string())
            .collect()
    };
    let user_rows = chat_rows(root, "chat-user").len();

    let saved = ChatHistory::load_from(&chat_history::history_path().unwrap());
    let saved_entries = saved
        .as_ref()
        .ok()
        .and_then(|history| history.chats(&crlf).first().map(|chat| chat.entries.len()));
    checks.check(
        saved
            .as_ref()
            .is_ok_and(|history| history.chats(&crlf).len() == 1)
            && saved_entries.is_some_and(|count| count > user_rows),
        "the conversation is saved as a chat about the open file",
    );
    let shown = labels();
    checks.check(
        shown.len() == 1 && shown[0].ends_with(" · Thoughts?") && !list.is_sensitive(),
        &format!("the chat list names the saved chat by date and first message, got {shown:?}"),
    );

    main.load_path(&crlf).expect("crlf.md opens again");
    pump();
    let shown = labels();
    checks.check(
        chat_rows(root, "chat-user").is_empty(),
        "opening a file starts a new conversation",
    );
    checks.check(
        shown.len() == 2
            && shown[0] == "New Conversation"
            && list.selected() == 0
            && list.is_sensitive(),
        &format!("the chat list offers the file's earlier chat, got {shown:?}"),
    );

    list.set_selected(1);
    pump();
    checks.check(
        chat_rows(root, "chat-user").len() == user_rows
            && !chat_rows(root, "chat-proposal").is_empty(),
        "choosing an earlier chat shows its conversation again",
    );
    checks.check(
        labels().len() == 1 && list.selected() == 0,
        "the empty new conversation leaves the list once an earlier chat is chosen",
    );
    restored_session(checks, app, window, &crlf, user_rows);

    buffer.set_modified(false);
    window
        .activate_action("win.new", None)
        .expect("win.new exists");
    pump_until(checks, "win.new empties the editor", DIALOG_TIMEOUT, || {
        buffer.char_count() == 0
    });
    let shown = labels();
    checks.check(
        chat_rows(root, "chat-user").len() == user_rows,
        "a new document keeps the conversation",
    );
    checks.check(
        shown.len() == 1 && shown[0] == "Thoughts?" && !list.is_sensitive(),
        &format!("an untitled document lists no saved chats, got {shown:?}"),
    );
    checks.check(
        State::load_from(&state::state_path().unwrap())
            .open_document
            .is_none(),
        "a new document is remembered as no file to reopen",
    );
}

/// A new window, as on the next start, reopens the file and continues the chat recorded in the
/// state, or keeps its new document when that file is gone. `crlf` is open in `window`, with its
/// earlier chat of `user_rows` messages shown.
/// Closing a window remembers its size and whether it is maximized, and the next window opens
/// the same way.
fn window_size(checks: &mut Checks, app: &adw::Application, window: &gtk::Window) {
    let state_path = state::state_path().unwrap();
    let start = || -> (Rc<MainWindow>, gtk::Window) {
        let next = MainWindow::new(app);
        let next_window = app
            .windows()
            .into_iter()
            .find(|w| w != window)
            .expect("a second window was created");
        (next, next_window)
    };

    let (next, next_window) = start();
    checks.check(
        next_window.default_size() == (1200, 800) && !next_window.is_maximized(),
        "without a remembered size, a window starts 1200 by 800 pixels and unmaximized",
    );
    next_window.set_default_size(1000, 700);
    // `close` only asks a window that is shown to close.
    next_window.present();
    pump();
    next_window.close();
    drop(next);
    pump();
    let state = State::load_from(&state_path);
    checks.check(
        state.window_size == Some((1000, 700)) && !state.window_maximized,
        &format!(
            "closing a window remembers its size, got {:?}, maximized {}",
            state.window_size, state.window_maximized
        ),
    );

    let (next, next_window) = start();
    checks.check(
        next_window.default_size() == (1000, 700) && !next_window.is_maximized(),
        "a new window starts at the remembered size",
    );
    next_window.present();
    next_window.maximize();
    let maximized = pump_until(checks, "the window maximizes", DIALOG_TIMEOUT, || {
        next_window.is_maximized()
    });
    next_window.close();
    drop(next);
    pump();
    if !maximized {
        return;
    }
    let state = State::load_from(&state_path);
    checks.check(
        state.window_size == Some((1000, 700)) && state.window_maximized,
        &format!(
            "closing a maximized window remembers that and its unmaximized size, got {:?}, \
             maximized {}",
            state.window_size, state.window_maximized
        ),
    );

    let (next, next_window) = start();
    checks.check(
        next_window.is_maximized() && next_window.default_size() == (1000, 700),
        "a new window starts maximized, keeping the size to unmaximize to",
    );
    drop(next);
    next_window.destroy();
}

fn restored_session(
    checks: &mut Checks,
    app: &adw::Application,
    window: &gtk::Window,
    crlf: &Path,
    user_rows: usize,
) {
    let state_path = state::state_path().unwrap();
    let state = State::load_from(&state_path);
    checks.check(
        state.open_document.as_deref() == Some(crlf) && state.open_chat == Some(1),
        &format!(
            "the open file and chat are remembered, got {:?} and {:?}",
            state.open_document, state.open_chat
        ),
    );

    let start = |checks: &mut Checks| -> Option<(Rc<MainWindow>, gtk::Window)> {
        let next = MainWindow::new(app);
        next.restore_session();
        pump();
        let next_window = app.windows().into_iter().find(|w| w != window);
        checks.check(next_window.is_some(), "a second window was created");
        next_window.map(|w| (next, w))
    };
    let title = |root: &gtk::Window| {
        widgets_under(root)
            .into_iter()
            .find_map(|w| w.downcast::<adw::WindowTitle>().ok())
            .map(|title| title.title().to_string())
    };

    if let Some((next, next_window)) = start(checks) {
        let root = next_window.clone().upcast::<gtk::Widget>();
        checks.check(
            title(&next_window).as_deref() == Some("crlf.md"),
            "a new window reopens the file that was open",
        );
        checks.check(
            chat_rows(&root, "chat-user").len() == user_rows
                && !chat_rows(&root, "chat-proposal").is_empty(),
            "a new window continues the chat that was shown",
        );
        drop(next);
        next_window.destroy();
    }

    let mut gone = State::load_from(&state_path);
    gone.open_document = Some(crlf.with_file_name("gone.md"));
    gone.save_to(&state_path).unwrap();
    if let Some((next, next_window)) = start(checks) {
        checks.check(
            title(&next_window).as_deref() == Some("Untitled")
                && find_alert(&next_window).is_none(),
            "a file that is gone leaves a new window untitled, without an alert",
        );
        checks.check(
            State::load_from(&state_path).open_document.is_none(),
            "a file that is gone is no longer reopened",
        );
        drop(next);
        next_window.destroy();
    }
}
