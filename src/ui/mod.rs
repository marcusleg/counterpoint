//! The GTK user interface.

pub mod chat_pane;
pub mod editor;
pub mod preferences_dialog;
pub mod window;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use window::MainWindow;

pub const APP_ID: &str = "de.marcusleg.Counterpoint";

const STYLE: &str = "
.boxed-list-separate > row.chat-user { background-color: alpha(var(--accent-bg-color), 0.12); }
.boxed-list-separate > row.chat-error { background-color: alpha(var(--error-bg-color), 0.12); }
label.edit-original,
label.edit-replacement { border-radius: 6px; padding: 6px 8px; }
label.edit-original { background-color: alpha(var(--error-bg-color), 0.15); }
label.edit-replacement { background-color: alpha(var(--success-bg-color), 0.15); }
";

/// (action, accelerators, label) for every application shortcut, in menu order.
const SHORTCUTS: &[(&str, &str, &str)] = &[
    ("win.open", "<Control>o", "Open"),
    ("win.save", "<Control>s", "Save"),
    ("win.save-as", "<Control><Shift>s", "Save As"),
    ("win.preferences", "<Control>comma", "Preferences"),
    ("app.shortcuts", "<Control>question", "Keyboard Shortcuts"),
    ("app.quit", "<Control>q", "Quit"),
];

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|app| {
        sourceview5::init();
        load_style();
        add_app_actions(app);
    });
    app.connect_activate(|app| match app.active_window() {
        Some(window) => window.present(),
        None => MainWindow::new(app).present(),
    });
    app.run()
}

fn load_style() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn add_app_actions(app: &adw::Application) {
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| {
            // Closing goes through the window's unsaved-changes check; the app exits with it.
            match app.active_window() {
                Some(window) => window.close(),
                None => app.quit(),
            }
        })
        .build();
    let shortcuts = gio::ActionEntry::builder("shortcuts")
        .activate(|app: &adw::Application, _, _| {
            shortcuts_dialog().present(app.active_window().as_ref());
        })
        .build();
    let about = gio::ActionEntry::builder("about")
        .activate(|app: &adw::Application, _, _| {
            about_dialog().present(app.active_window().as_ref());
        })
        .build();
    app.add_action_entries([quit, shortcuts, about]);
    for (action, accel, _) in SHORTCUTS {
        app.set_accels_for_action(action, &[accel]);
    }
}

fn about_dialog() -> adw::AboutDialog {
    adw::AboutDialog::builder()
        .application_name("Counterpoint")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(env!("CARGO_PKG_DESCRIPTION"))
        .developer_name("Marcus Legendre")
        .license_type(gtk::License::MitX11)
        .website("https://github.com/marcusleg/counterpoint")
        .issue_url("https://github.com/marcusleg/counterpoint/issues")
        .build()
}

fn shortcuts_dialog() -> adw::ShortcutsDialog {
    let file = adw::ShortcutsSection::new(Some("Application"));
    for (_, accel, label) in SHORTCUTS {
        file.add(adw::ShortcutsItem::new(label, accel));
    }
    let editor = adw::ShortcutsSection::new(Some("Editor"));
    editor.add(adw::ShortcutsItem::new("Undo", "<Control>z"));
    editor.add(adw::ShortcutsItem::new("Redo", "<Control><Shift>z"));
    let chat = adw::ShortcutsSection::new(Some("Chat"));
    chat.add(adw::ShortcutsItem::new("Send message", "Return"));
    chat.add(adw::ShortcutsItem::new("Send message", "<Control>Return"));
    chat.add(adw::ShortcutsItem::new("New line", "<Shift>Return"));
    let dialog = adw::ShortcutsDialog::new();
    dialog.add(file);
    dialog.add(editor);
    dialog.add(chat);
    dialog
}
