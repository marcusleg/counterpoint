//! Renders the README screenshot: the main window with a sample blog post, a sparring reply and
//! a ghostwriting proposal, answered by a canned endpoint. Run it through `dev/screenshot.sh`,
//! which provides a private headless compositor. (Broadway, as in `dev/headless.sh`, does not
//! draw frames while no browser is attached, so the window would never be laid out.)
//!
//! Usage: screenshot <output.png> [--dark]

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib, graphene};

use counterpoint::config::Config;
use counterpoint::ui::window::MainWindow;

const TIMEOUT: Duration = Duration::from_secs(10);

const DOCUMENT: &str = "\
---
title: Why I still write first drafts by hand
date: 2026-09-27
tags: [writing, tools]
---

# Why I still write first drafts by hand

Every article on this blog starts as a plain Markdown file. No CMS, no preview pane, just text that I can diff, grep and keep in `git`.

## The first draft is for me

In the first draft, I am basically just trying to figure out what it is that I actually want to say, and so it tends to be quite long and a bit repetitive in places.

The second draft is for the reader. That is where I cut, reorder and sharpen until every paragraph earns its place.

## Where the model comes in

I don't let a model write for me. I ask it to argue with me: *Is this claim supported? What would a sceptical reader ask here?*

When I do want help with the wording, I want to see the exact change before it lands in my text, and I want to be able to undo it.
";

const SELECTED: &str = "In the first draft, I am basically just trying to figure out what it is that I actually want to say, and so it tends to be quite long and a bit repetitive in places.";

const SPARRING_QUESTION: &str = "Is this paragraph pulling its weight?";

const SPARRING_REPLY: &str = "\
Partly. It sets up the contrast with the second draft, but *basically*, *just* and *actually* \
are filler, and it calls itself long instead of being short.

The idea fits in one sentence.";

const GHOSTWRITING_QUESTION: &str = "Tighten it to one sentence.";

const REPLACEMENT: &str = "The first draft is where I find out what I want to say.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dark = args.iter().any(|a| a == "--dark");
    let Some(output) = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
    else {
        eprintln!("usage: screenshot <output.png> [--dark]");
        return ExitCode::FAILURE;
    };
    let output = std::path::absolute(&output).expect("absolute output path");

    // Safe: the process is still single-threaded here, before GTK starts. A fresh home keeps
    // the run away from the user's settings, and makes the window subtitle read `~/Blog`.
    let home = tempfile::tempdir().expect("temp dir for HOME");
    unsafe {
        std::env::set_var("HOME", home.path());
        std::env::set_var("XDG_STATE_HOME", home.path().join(".local/state"));
        std::env::set_var("XDG_CONFIG_HOME", home.path().join(".config"));
        std::env::set_var("XDG_DATA_HOME", home.path().join(".local/share"));
    }
    let blog = home.path().join("Blog");
    fs::create_dir_all(&blog).unwrap();
    let post = blog.join("first-drafts.md");
    fs::write(&post, DOCUMENT).unwrap();

    let mut server = mockito::Server::new();
    Config {
        base_url: format!("{}/v1", server.url()),
        api_key: None,
        model: Some("gemma3:27b".to_string()),
    }
    .save()
    .expect("settings saved under XDG_CONFIG_HOME");
    let reply = |content: &str| {
        serde_json::json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
            .to_string()
    };
    let proposal = format!(
        "One sentence, and the next paragraph still reads as the contrast.\n\n\
         ```original\n{SELECTED}\n```\n```replacement\n{REPLACEMENT}\n```"
    );
    // The whole conversation is sent with every request, so the second one is told apart by
    // its latest question.
    server
        .mock("POST", "/v1/chat/completions")
        .match_body(mockito::Matcher::Regex(GHOSTWRITING_QUESTION.to_string()))
        .with_body(reply(&proposal))
        .create();
    server
        .mock("POST", "/v1/chat/completions")
        .with_body(reply(SPARRING_REPLY))
        .create();

    let app = adw::Application::builder()
        .application_id("de.marcusleg.Counterpoint.Screenshot")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let result = Rc::new(RefCell::new(Err("the window never activated".to_string())));
    app.connect_activate(glib::clone!(
        #[strong]
        result,
        move |app| {
            sourceview5::init();
            counterpoint::ui::load_style();
            if let Some(settings) = gtk::Settings::default() {
                settings.set_gtk_enable_animations(false);
                // GNOME's default, which a bare compositor does not pass on to GTK.
                settings.set_gtk_font_name(Some("Adwaita Sans 11"));
            }
            adw::StyleManager::default().set_color_scheme(if dark {
                adw::ColorScheme::ForceDark
            } else {
                adw::ColorScheme::ForceLight
            });
            let main = MainWindow::new(app);
            // A little taller than the default, so the whole conversation fits.
            main.window().set_default_size(1200, 860);
            main.present();
            let app = app.clone();
            let result = Rc::clone(&result);
            let post = post.clone();
            let output = output.clone();
            glib::idle_add_local_once(move || {
                *result.borrow_mut() = stage(&main, &post).and_then(|()| save(&main, &output));
                app.quit();
            });
        }
    ));
    app.run_with_args::<&str>(&[]);

    let result = result.borrow();
    match &*result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("screenshot failed: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Opens the post, selects a paragraph, and holds one sparring and one ghostwriting exchange.
fn stage(main: &Rc<MainWindow>, post: &Path) -> Result<(), String> {
    main.load_path(post)?;
    let root = main.window().clone().upcast::<gtk::Widget>();
    let widgets = widgets_under(&root);
    let editor = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<sourceview5::View>())
        .ok_or("no editor view")?
        .clone();
    // The chat input is a plain `gtk::TextView`; the editor is a subclass of it.
    let input = widgets
        .iter()
        .find(|w| w.type_() == gtk::TextView::static_type())
        .and_then(|w| w.downcast_ref::<gtk::TextView>())
        .ok_or("no chat input")?
        .clone();
    let mode = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<adw::ToggleGroup>())
        .ok_or("no mode toggle")?
        .clone();
    let send = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::Button>())
        .find(|b| b.label().as_deref() == Some("_Send"))
        .ok_or("no Send button")?
        .clone();

    let buffer = editor.buffer();
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    let start = text
        .find(SELECTED)
        .ok_or("the selected paragraph is missing")?;
    let chars = |bytes: usize| text[..bytes].chars().count() as i32;
    let from = buffer.iter_at_offset(chars(start));
    let to = buffer.iter_at_offset(chars(start + SELECTED.len()));
    buffer.select_range(&from, &to);

    input.buffer().set_text(SPARRING_QUESTION);
    send.emit_clicked();
    pump_until(|| rows(&root, "chat-assistant") == 1)?;

    mode.set_active_name(Some("ghostwriting"));
    input.buffer().set_text(GHOSTWRITING_QUESTION);
    send.emit_clicked();
    pump_until(|| rows(&root, "chat-proposal") == 1)?;
    // Lets the chat list scroll to its end once the new rows have a size.
    let settle = Instant::now() + Duration::from_millis(500);
    pump_until(|| Instant::now() > settle)
}

/// Writes the window, as it is drawn now, to `output` as a PNG.
fn save(main: &Rc<MainWindow>, output: &Path) -> Result<(), String> {
    let window = main.window();
    let (width, height) = (window.width() as f32, window.height() as f32);
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, width as f64, height as f64);
    let node = snapshot.to_node().ok_or("the window drew nothing")?;
    let renderer = window.renderer().ok_or("the window has no renderer")?;
    let texture =
        renderer.render_texture(&node, Some(&graphene::Rect::new(0.0, 0.0, width, height)));
    texture
        .save_to_png(output)
        .map_err(|e| format!("could not write {}: {e}", output.display()))
}

fn widgets_under(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut out = vec![root.clone()];
    let mut child = root.first_child();
    while let Some(c) = child {
        out.extend(widgets_under(&c));
        child = c.next_sibling();
    }
    out
}

/// The number of chat rows with the given CSS class.
fn rows(root: &gtk::Widget, class: &str) -> usize {
    widgets_under(root)
        .into_iter()
        .filter(|w| w.is::<gtk::ListBoxRow>() && w.has_css_class(class))
        .count()
}

/// Runs the main loop until `done` holds, or gives up after `TIMEOUT`.
fn pump_until(mut done: impl FnMut() -> bool) -> Result<(), String> {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        while context.iteration(false) {}
        if done() {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("timed out after {}s", TIMEOUT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
