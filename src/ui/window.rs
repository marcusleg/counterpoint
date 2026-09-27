//! The main window: header bar, editor, chat pane and file handling.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::document;
use crate::prompt::Mode;
use crate::ui::chat_pane::ChatPane;
use crate::ui::editor::EditorView;
use crate::ui::options_dialog::OptionsDialog;

pub struct MainWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    editor: EditorView,
    /// Owned here: the pane's own signal handlers only hold weak references.
    chat: Rc<ChatPane>,
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
        let chat = ChatPane::new(editor.clone());
        chat.widget().set_width_request(280);

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&editor_scroller)
            .end_child(chat.widget())
            .resize_start_child(true)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(960)
            .build();

        let mode = adw::ToggleGroup::new();
        mode.add(
            adw::Toggle::builder()
                .name("sparring")
                .label("Sparring")
                .tooltip("The LLM can read the document but not change it")
                .build(),
        );
        mode.add(
            adw::Toggle::builder()
                .name("ghostwriting")
                .label("Ghostwriting")
                .tooltip("The LLM can propose changes that you apply or reject")
                .build(),
        );
        mode.set_active_name(Some("sparring"));
        mode.connect_active_name_notify(glib::clone!(
            #[weak]
            chat,
            move |group| {
                chat.set_mode(match group.active_name().as_deref() {
                    Some("ghostwriting") => Mode::Ghostwriting,
                    _ => Mode::Sparring,
                });
            }
        ));

        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main Menu")
            .menu_model(&primary_menu())
            .primary(true)
            .build();

        let title = adw::WindowTitle::new("", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_end(&menu_button);
        header.pack_end(&mode);

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
            chat,
            path: RefCell::new(None),
            crlf: Cell::new(false),
            close_confirmed: Cell::new(false),
            confirm_pending: Cell::new(false),
        });
        this.update_title();
        this.follow_dark_mode();
        this.add_actions();
        // Explicit rather than relying on both defaults happening to agree.
        this.chat.set_mode(match mode.active_name().as_deref() {
            Some("ghostwriting") => Mode::Ghostwriting,
            _ => Mode::Sparring,
        });

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
        let options = gio::ActionEntry::builder("options")
            .activate(|window: &adw::ApplicationWindow, _, _| OptionsDialog::present(window))
            .build();
        self.window
            .add_action_entries([open, save, save_as, options]);
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

    async fn open(self: &Rc<Self>) {
        if !self.confirm_discard().await {
            return;
        }
        let dialog = file_dialog("Open Markdown File");
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
            None => dialog.set_initial_name(Some("Untitled.md")),
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
    file.append(Some("_Open…"), Some("win.open"));
    file.append(Some("_Save"), Some("win.save"));
    file.append(Some("Save _As…"), Some("win.save-as"));
    let tools = gio::Menu::new();
    tools.append(Some("_Options…"), Some("win.options"));
    tools.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
    let quit = gio::Menu::new();
    quit.append(Some("_Quit"), Some("app.quit"));
    let menu = gio::Menu::new();
    menu.append_section(None, &file);
    menu.append_section(None, &tools);
    menu.append_section(None, &quit);
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
