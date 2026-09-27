//! The main window: header bar, editor, chat pane and file handling.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::document;
use crate::state::State;
use crate::ui::chat_pane::ChatPane;
use crate::ui::editor::EditorView;
use crate::ui::preferences_dialog::PreferencesDialog;
use crate::zoom::Zoom;

pub struct MainWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    editor: EditorView,
    /// Owned here: the pane's own signal handlers only hold weak references; never read, held
    /// only to keep the pane alive.
    _chat: Rc<ChatPane>,
    /// The button showing the current zoom percentage, e.g. "100%".
    zoom_label: gtk::Button,
    /// The editor's current zoom level.
    zoom: Cell<Zoom>,
    /// The open file, or `None` for a new document.
    path: RefCell<Option<PathBuf>>,
    /// True if the open file uses CRLF line endings, so saving restores them.
    crlf: Cell<bool>,
    /// Set once the user has saved or discarded changes, so the window may close.
    close_confirmed: Cell<bool>,
    /// Set while the unsaved-changes alert (and a save it starts) is in progress, so another
    /// close, Quit or Open waits for it instead of opening a second alert.
    confirm_pending: Cell<bool>,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let editor = EditorView::new();
        let editor_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .width_request(300)
            .child(editor.widget())
            .build();
        let editor_placeholder = gtk::Label::builder()
            .label("Open a Markdown file or start writing…")
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_start(24)
            .margin_top(16)
            .can_target(false)
            .css_classes(["dim-label"])
            .build();
        let editor_overlay = gtk::Overlay::builder().child(&editor_scroller).build();
        editor_overlay.add_overlay(&editor_placeholder);
        editor_placeholder.set_visible(editor.buffer().char_count() == 0);
        editor.buffer().connect_changed(glib::clone!(
            #[weak]
            editor_placeholder,
            move |buffer| editor_placeholder.set_visible(buffer.char_count() == 0)
        ));
        let chat = ChatPane::new(editor.clone());
        chat.widget().set_width_request(280);

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&editor_overlay)
            .end_child(chat.widget())
            .resize_start_child(true)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(960)
            .build();

        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main Menu")
            .menu_model(&primary_menu())
            .primary(true)
            .build();

        let zoom_out_button = gtk::Button::builder()
            .icon_name("zoom-out-symbolic")
            .tooltip_text("Zoom Out")
            .action_name("win.zoom-out")
            .build();
        let zoom_label = gtk::Button::builder()
            .label(Zoom::default().label())
            .tooltip_text("Reset Zoom")
            .action_name("win.zoom-reset")
            .css_classes(["flat"])
            .build();
        let zoom_in_button = gtk::Button::builder()
            .icon_name("zoom-in-symbolic")
            .tooltip_text("Zoom In")
            .action_name("win.zoom-in")
            .build();
        let zoom_box = gtk::Box::builder().css_classes(["linked"]).build();
        zoom_box.append(&zoom_out_button);
        zoom_box.append(&zoom_label);
        zoom_box.append(&zoom_in_button);

        let title = adw::WindowTitle::new("", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_start(&zoom_box);
        header.pack_end(&menu_button);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&paned));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1400)
            .default_height(850)
            .content(&toolbar)
            .build();

        let this = Rc::new(Self {
            window,
            title,
            editor,
            _chat: chat,
            zoom_label,
            zoom: Cell::new(Zoom::default()),
            path: RefCell::new(None),
            crlf: Cell::new(false),
            close_confirmed: Cell::new(false),
            confirm_pending: Cell::new(false),
        });
        this.update_title();
        this.follow_dark_mode();
        this.add_actions();
        let initial_zoom = State::load()
            .zoom
            .map(Zoom::from_percent)
            .unwrap_or_default();
        this.apply_zoom(initial_zoom);

        this.editor.buffer().connect_modified_changed(glib::clone!(
            #[weak]
            this,
            move |_| this.update_title()
        ));
        // Holds the only strong reference; GTK drops it when the window is destroyed.
        this.window.connect_close_request(glib::clone!(
            #[strong]
            this,
            move |_| this.on_close_request()
        ));
        this
    }

    pub fn present(&self) {
        self.window.present();
        self.editor.widget().grab_focus();
    }

    fn add_actions(self: &Rc<Self>) {
        let new = gio::ActionEntry::builder("new")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    glib::spawn_future_local(async move { this.new_document().await });
                }
            ))
            .build();
        let open = gio::ActionEntry::builder("open")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    glib::spawn_future_local(async move { this.open().await });
                }
            ))
            .build();
        let save = gio::ActionEntry::builder("save")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    glib::spawn_future_local(async move {
                        this.save().await;
                    });
                }
            ))
            .build();
        let save_as = gio::ActionEntry::builder("save-as")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    glib::spawn_future_local(async move {
                        this.save_as().await;
                    });
                }
            ))
            .build();
        let preferences = gio::ActionEntry::builder("preferences")
            .activate(|window: &adw::ApplicationWindow, _, _| PreferencesDialog::present(window))
            .build();
        let zoom_in = gio::ActionEntry::builder("zoom-in")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    let zoom = this.zoom.get().zoom_in();
                    this.apply_zoom(zoom);
                    this.remember_zoom(zoom);
                }
            ))
            .build();
        let zoom_out = gio::ActionEntry::builder("zoom-out")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    let zoom = this.zoom.get().zoom_out();
                    this.apply_zoom(zoom);
                    this.remember_zoom(zoom);
                }
            ))
            .build();
        let zoom_reset = gio::ActionEntry::builder("zoom-reset")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    let zoom = Zoom::reset();
                    this.apply_zoom(zoom);
                    this.remember_zoom(zoom);
                }
            ))
            .build();
        self.window.add_action_entries([
            new,
            open,
            save,
            save_as,
            preferences,
            zoom_in,
            zoom_out,
            zoom_reset,
        ]);
    }

    /// Applies `zoom` to the editor and the header bar's zoom controls.
    fn apply_zoom(&self, zoom: Zoom) {
        self.zoom.set(zoom);
        self.editor.set_zoom(zoom.percent());
        self.zoom_label.set_label(&zoom.label());
        self.set_zoom_action_enabled("zoom-in", zoom.can_zoom_in());
        self.set_zoom_action_enabled("zoom-out", zoom.can_zoom_out());
    }

    fn set_zoom_action_enabled(&self, name: &str, enabled: bool) {
        if let Some(action) = self
            .window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(enabled);
        }
    }

    /// Persists the zoom level for the next restart. Saving the state is a convenience, so a
    /// failure here must not interrupt the user.
    fn remember_zoom(&self, zoom: Zoom) {
        let mut state = State::load();
        state.zoom = Some(zoom.percent());
        let _ = state.save();
    }

    /// Persists `path`'s folder as the most recently used one, so **Open…** starts there next
    /// time. Saving the state is a convenience, so a failure here must not interrupt the user.
    fn remember_folder(&self, path: &Path) {
        let Some(folder) = path.parent() else {
            return;
        };
        let mut state = State::load();
        state.last_folder = Some(folder.to_path_buf());
        let _ = state.save();
    }

    fn on_close_request(self: &Rc<Self>) -> glib::Propagation {
        if self.close_confirmed.get() || !self.editor.buffer().is_modified() {
            return glib::Propagation::Proceed;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            if this.confirm_discard().await {
                this.close_confirmed.set(true);
                this.window.close();
            }
        });
        glib::Propagation::Stop
    }

    /// Asks what to do with unsaved changes. Returns true if the caller may go on: there were no
    /// changes, the user discarded them, or they were saved.
    async fn confirm_discard(self: &Rc<Self>) -> bool {
        if !self.editor.buffer().is_modified() {
            return true;
        }
        if self.confirm_pending.replace(true) {
            return false;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Save changes?")
            .body("The document has unsaved changes. Save them first?")
            .default_response("save")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[
            ("cancel", "_Cancel"),
            ("discard", "_Discard"),
            ("save", "_Save"),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        let proceed = match dialog.choose_future(Some(&self.window)).await.as_str() {
            "discard" => true,
            "save" => self.save().await,
            _ => false,
        };
        self.confirm_pending.set(false);
        proceed
    }

    /// Starts a new, untitled document, once the unsaved-changes guard allows it. The chat
    /// conversation, the remembered folder and the zoom level are untouched.
    async fn new_document(self: &Rc<Self>) {
        if !self.confirm_discard().await {
            return;
        }
        self.reset_document();
    }

    /// Clears the document to a fresh, untitled state: nothing to undo, unmodified, no open
    /// path, LF line endings, and the title back to "Untitled".
    fn reset_document(&self) {
        self.editor.load("");
        *self.path.borrow_mut() = None;
        self.crlf.set(false);
        self.update_title();
    }

    async fn open(self: &Rc<Self>) {
        if !self.confirm_discard().await {
            return;
        }
        let dialog = file_dialog("Open Markdown File");
        if let Some(folder) = State::load().remembered_folder() {
            dialog.set_initial_folder(Some(&gio::File::for_path(folder)));
        }
        let Ok(file) = dialog.open_future(Some(&self.window)).await else {
            return;
        };
        let Some(path) = file.path() else {
            return;
        };
        match document::read_file(&path) {
            Ok(contents) => {
                let (text, crlf) = document::from_disk(&contents);
                self.crlf.set(crlf);
                self.editor.load(&text);
                self.remember_folder(&path);
                self.set_path(path);
            }
            Err(message) => self.show_error(&message),
        }
    }

    /// Saves to the open file, or asks for a file name first. Returns true once saved.
    async fn save(self: &Rc<Self>) -> bool {
        let path = self.path.borrow().clone();
        match path {
            Some(path) => self.write_to(path),
            None => self.save_as().await,
        }
    }

    async fn save_as(self: &Rc<Self>) -> bool {
        let dialog = file_dialog("Save Markdown File");
        match self.path.borrow().as_ref() {
            Some(path) => dialog.set_initial_file(Some(&gio::File::for_path(path))),
            None => {
                dialog.set_initial_name(Some("Untitled.md"));
                if let Some(folder) = State::load().remembered_folder() {
                    dialog.set_initial_folder(Some(&gio::File::for_path(folder)));
                }
            }
        }
        let Ok(file) = dialog.save_future(Some(&self.window)).await else {
            return false;
        };
        match file.path() {
            Some(path) => self.write_to(path),
            None => false,
        }
    }

    fn write_to(&self, path: PathBuf) -> bool {
        let contents = document::to_disk(&self.editor.text(), self.crlf.get());
        match document::write_file(&path, &contents) {
            Ok(()) => {
                self.editor.buffer().set_modified(false);
                self.remember_folder(&path);
                self.set_path(path);
                true
            }
            Err(message) => {
                self.show_error(&message);
                false
            }
        }
    }

    fn set_path(&self, path: PathBuf) {
        *self.path.borrow_mut() = Some(path);
        self.update_title();
    }

    fn update_title(&self) {
        let path = self.path.borrow();
        let name = path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        let folder = path
            .as_deref()
            .and_then(Path::parent)
            .map(|folder| folder.display().to_string())
            .unwrap_or_default();
        let marker = if self.editor.buffer().is_modified() {
            "• "
        } else {
            ""
        };
        self.title.set_title(&format!("{marker}{name}"));
        self.title.set_subtitle(&folder);
        self.window
            .set_title(Some(&format!("{marker}{name} — Counterpoint")));
    }

    fn follow_dark_mode(self: &Rc<Self>) {
        let style = adw::StyleManager::default();
        self.editor.set_dark(style.is_dark());
        style.connect_dark_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |style| this.editor.set_dark(style.is_dark())
        ));
    }

    fn show_error(&self, message: &str) {
        let dialog = adw::AlertDialog::new(Some("Error"), Some(message));
        dialog.add_response("ok", "_OK");
        dialog.present(Some(&self.window));
    }
}

fn primary_menu() -> gio::Menu {
    let file = gio::Menu::new();
    file.append(Some("_New"), Some("win.new"));
    file.append(Some("_Open…"), Some("win.open"));
    file.append(Some("_Save"), Some("win.save"));
    file.append(Some("Save _As…"), Some("win.save-as"));
    let tools = gio::Menu::new();
    tools.append(Some("_Preferences"), Some("win.preferences"));
    tools.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
    let about = gio::Menu::new();
    about.append(Some("_About Counterpoint"), Some("app.about"));
    let menu = gio::Menu::new();
    menu.append_section(None, &file);
    menu.append_section(None, &tools);
    menu.append_section(None, &about);
    menu
}

fn file_dialog(title: &str) -> gtk::FileDialog {
    let markdown = gtk::FileFilter::new();
    markdown.set_name(Some("Markdown files"));
    markdown.add_pattern("*.md");
    markdown.add_pattern("*.markdown");
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&markdown);
    filters.append(&all);
    gtk::FileDialog::builder()
        .title(title)
        .modal(true)
        .filters(&filters)
        .default_filter(&markdown)
        .build()
}
