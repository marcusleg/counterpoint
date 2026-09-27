//! The GTK user interface.

pub mod chat_pane;
pub mod editor;
pub mod preferences_dialog;
pub mod window;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use window::MainWindow;

pub const APP_ID: &str = "de.marcusleg.Counterpoint";

/// Shown in place of a result when the worker thread running a request panicked.
const WORKER_FAILED: &str = "The request failed unexpectedly.";

const STYLE: &str = "
.boxed-list-separate > row.chat-user { background-color: alpha(var(--accent-bg-color), 0.12); }
.boxed-list-separate > row.chat-error { background-color: alpha(var(--error-bg-color), 0.12); }
label.edit-original,
label.edit-replacement { border-radius: 6px; padding: 6px 8px; }
label.edit-original { background-color: alpha(var(--error-bg-color), 0.15); }
label.edit-replacement { background-color: alpha(var(--success-bg-color), 0.15); }
.zoom-controls { margin: 6px 12px; }
";

/// (action, accelerators, label) for every application shortcut. An action may have several
/// accelerators; all are shown in the Keyboard Shortcuts dialog.
const SHORTCUTS: &[(&str, &[&str], &str)] = &[
    ("win.new", &["<Control>n"], "New"),
    ("win.open", &["<Control>o"], "Open"),
    ("win.save", &["<Control>s"], "Save"),
    ("win.save-as", &["<Control><Shift>s"], "Save As"),
    ("win.toggle-chat", &["F9"], "Show or Hide the Chat"),
    ("win.preferences", &["<Control>comma"], "Preferences"),
    (
        "win.zoom-in",
        &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
        "Zoom In",
    ),
    (
        "win.zoom-out",
        &["<Control>minus", "<Control>KP_Subtract"],
        "Zoom Out",
    ),
    (
        "win.zoom-reset",
        &["<Control>0", "<Control>KP_0"],
        "Reset Zoom",
    ),
    (
        "app.shortcuts",
        &["<Control>question"],
        "Keyboard Shortcuts",
    ),
    ("app.quit", &["<Control>q"], "Quit"),
];

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|app| {
        sourceview5::init();
        load_style();
        add_app_actions(app);
    });
    app.connect_activate(|app| {
        main_window(app).present();
    });
    // `counterpoint post.md`, or a file manager: the (single) window opens the first file,
    // through its own unsaved-changes guard.
    app.connect_open(|app, files, _| {
        let window = main_window(app);
        window.present();
        if let Some(file) = files.first() {
            let _ = window.activate_action("win.open-uri", Some(&file.uri().to_variant()));
        }
    });
    app.run()
}

/// The existing window, or a new one.
fn main_window(app: &adw::Application) -> gtk::Window {
    match app.active_window() {
        Some(window) => window,
        None => MainWindow::new(app).window().clone().upcast(),
    }
}

/// Adds the application's stylesheet to the default display. Needs an initialised display.
pub fn load_style() {
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
            // Closing goes through each window's unsaved-changes check; the app exits with the
            // last window.
            let windows = app.windows();
            if windows.is_empty() {
                app.quit();
            }
            for window in windows {
                window.close();
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
    for (action, accels, _) in SHORTCUTS {
        app.set_accels_for_action(action, accels);
    }
}

fn about_dialog() -> adw::AboutDialog {
    let repository = env!("CARGO_PKG_REPOSITORY");
    adw::AboutDialog::builder()
        .application_name("Counterpoint")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(env!("CARGO_PKG_DESCRIPTION"))
        .developer_name("Marcus Legendre")
        .license_type(gtk::License::MitX11)
        .website(repository)
        .issue_url(format!("{repository}/issues"))
        .build()
}

fn shortcuts_dialog() -> adw::ShortcutsDialog {
    let application = adw::ShortcutsSection::new(Some("Application"));
    for (_, accels, label) in SHORTCUTS {
        application.add(adw::ShortcutsItem::new(label, &accels.join(" ")));
    }
    let editor = adw::ShortcutsSection::new(Some("Editor"));
    editor.add(adw::ShortcutsItem::new("Undo", "<Control>z"));
    editor.add(adw::ShortcutsItem::new(
        "Redo",
        "<Control><Shift>z <Control>y",
    ));
    let chat = adw::ShortcutsSection::new(Some("Chat"));
    chat.add(adw::ShortcutsItem::new(
        "Send Message",
        "Return <Control>Return",
    ));
    chat.add(adw::ShortcutsItem::new("New Line", "<Shift>Return"));
    let dialog = adw::ShortcutsDialog::new();
    dialog.add(application);
    dialog.add(editor);
    dialog.add(chat);
    dialog
}

/// Runs `work` on a worker thread and returns its result on the main loop. A panic in the
/// worker becomes an error message rather than a lost reply.
pub(crate) async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    gio::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| Err(WORKER_FAILED.to_string()))
}
