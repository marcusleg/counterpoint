//! The GTK user interface.

use adw::prelude::*;
use gtk::glib;

pub const APP_ID: &str = "de.marcusleg.Counterpoint";

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        adw::ApplicationWindow::builder()
            .application(app)
            .title("Counterpoint")
            .default_width(1400)
            .default_height(850)
            .build()
            .present();
    });
    app.run()
}
