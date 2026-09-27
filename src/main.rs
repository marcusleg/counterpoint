mod bridge;
mod chat;
mod config;
mod document;
mod llm;
mod prompt;
mod proposal;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

/// GNOME's default Qt integration (gtk3) reports a light colour scheme even when GNOME prefers
/// dark; the XDG desktop portal integration reports the real scheme, and Main.qml switches to a
/// dark palette from it.
fn wants_portal_theme(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|desktop| {
        desktop
            .split(':')
            .any(|name| name.eq_ignore_ascii_case("GNOME"))
    })
}

fn main() {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    if std::env::var_os("QT_QPA_PLATFORMTHEME").is_none() && wants_portal_theme(desktop.as_deref())
    {
        std::env::set_var("QT_QPA_PLATFORMTHEME", "xdgdesktopportal");
    }

    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Fusion");
    }

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/Counterpoint/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_portal_theme_on_gnome_only() {
        assert!(wants_portal_theme(Some("GNOME")));
        assert!(wants_portal_theme(Some("ubuntu:GNOME")));
        assert!(!wants_portal_theme(Some("KDE")));
        assert!(!wants_portal_theme(None));
    }
}
