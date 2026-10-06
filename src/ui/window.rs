//! The main window: header bar, editor, chat pane and file handling.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::document::{self, DiskFormat};
use crate::state::State;
use crate::ui::chat_pane::ChatPane;
use crate::ui::editor::EditorView;
use crate::ui::find_bar::FindBar;
use crate::ui::preferences_dialog::PreferencesDialog;
use crate::zoom::Zoom;

/// Below this width the chat pane overlays the editor instead of squeezing it.
const COLLAPSE_BELOW_SP: f64 = 900.0;

pub struct MainWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    editor: EditorView,
    find_bar: Rc<FindBar>,
    chat: Rc<ChatPane>,
    split: adw::OverlaySplitView,
    /// The button showing the current zoom percentage, e.g. "100%".
    zoom_label: gtk::Button,
    /// The editor's current zoom level.
    zoom: Cell<Zoom>,
    /// The remembered folder, recent files and zoom level, loaded once and saved on every change.
    state: RefCell<State>,
    /// The **Open Recent** submenu, rebuilt whenever the recent files change.
    recent_menu: gio::Menu,
    /// The open file, or `None` for a new document.
    path: RefCell<Option<PathBuf>>,
    /// Line endings and byte order mark of the open file, restored on save.
    format: Cell<DiskFormat>,
    /// The file's modification time when it was last read or written, to notice changes made
    /// by other programs before overwriting them.
    modified_on_disk: Cell<Option<SystemTime>>,
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

        let find_bar = FindBar::new(editor.clone());
        let editor_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
        editor_area.append(find_bar.widget());
        editor_area.append(&editor_overlay);
        editor_overlay.set_vexpand(true);

        let toasts = adw::ToastOverlay::new();
        let chat = ChatPane::new(editor.clone(), toasts.clone());

        let split = adw::OverlaySplitView::builder()
            .content(&editor_area)
            .sidebar(chat.widget())
            .sidebar_position(gtk::PackType::End)
            .min_sidebar_width(280.0)
            .max_sidebar_width(600.0)
            .sidebar_width_fraction(0.3)
            .build();
        toasts.set_child(Some(&split));

        let open_button = gtk::Button::builder()
            .label("_Open…")
            .use_underline(true)
            .tooltip_text("Open a Markdown File")
            .action_name("win.open")
            .build();
        let chat_toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-right-symbolic")
            .tooltip_text("Show or Hide the Chat")
            .action_name("win.toggle-chat")
            .build();
        split
            .bind_property("show-sidebar", &chat_toggle, "active")
            .sync_create()
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
            .build();
        let zoom_in_button = gtk::Button::builder()
            .icon_name("zoom-in-symbolic")
            .tooltip_text("Zoom In")
            .action_name("win.zoom-in")
            .build();
        let zoom_box = gtk::Box::builder()
            .css_classes(["linked", "zoom-controls"])
            .halign(gtk::Align::Center)
            .build();
        zoom_box.append(&zoom_out_button);
        zoom_box.append(&zoom_label);
        zoom_box.append(&zoom_in_button);

        let recent_menu = gio::Menu::new();
        let menu_popover = gtk::PopoverMenu::from_model(Some(&primary_menu(&recent_menu)));
        menu_popover.add_child(&zoom_box, "zoom");
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main Menu")
            .popover(&menu_popover)
            .primary(true)
            .build();

        let title = adw::WindowTitle::new("", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_start(&open_button);
        header.pack_end(&menu_button);
        header.pack_end(&chat_toggle);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&toasts));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1200)
            .default_height(800)
            .content(&toolbar)
            .build();
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            COLLAPSE_BELOW_SP,
            adw::LengthUnit::Sp,
        ));
        narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(narrow);

        let state = State::load();
        let initial_zoom = state.zoom.map(Zoom::from_percent).unwrap_or_default();
        let this = Rc::new(Self {
            window,
            title,
            editor,
            find_bar,
            chat,
            split,
            zoom_label,
            zoom: Cell::new(Zoom::default()),
            state: RefCell::new(state),
            recent_menu,
            path: RefCell::new(None),
            format: Cell::new(DiskFormat::default()),
            modified_on_disk: Cell::new(None),
            close_confirmed: Cell::new(false),
            confirm_pending: Cell::new(false),
        });
        this.update_title();
        this.update_recent_menu();
        this.follow_dark_mode();
        this.add_actions();
        this.apply_zoom(initial_zoom);
        this.accept_dropped_files(&editor_overlay);

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

    pub fn window(&self) -> &adw::ApplicationWindow {
        &self.window
    }

    pub fn present(&self) {
        self.window.present();
        self.editor.widget().grab_focus();
    }

    /// Opens `file` once the unsaved-changes guard allows it; errors are shown in a dialog.
    pub fn open_file(self: &Rc<Self>, file: gio::File) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            if !this.confirm_discard().await {
                return;
            }
            match file.path() {
                Some(path) => {
                    if let Err(message) = this.load_path(&path) {
                        this.show_error("Could Not Open File", &message);
                    }
                }
                None => this.show_error("Could Not Open File", ONLY_LOCAL_FILES),
            }
        });
    }

    /// Replaces the document with the file at `path`, with nothing to undo. Does not ask about
    /// unsaved changes; `open_file` does. A file that cannot be read leaves **Open Recent**.
    pub fn load_path(&self, path: &Path) -> Result<(), String> {
        let contents = document::read_file(path).inspect_err(|_| self.forget_recent(path))?;
        let (text, format) = document::from_disk(&contents);
        self.format.set(format);
        self.editor.load(&text);
        self.modified_on_disk.set(modification_time(path));
        self.remember_file(path);
        self.set_path(path.to_path_buf());
        Ok(())
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
        let open_uri = gio::ActionEntry::builder("open-uri")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, parameter| {
                    if let Some(uri) = parameter.and_then(|p| p.get::<String>()) {
                        this.open_file(gio::File::for_uri(&uri));
                    }
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
        let find = gio::ActionEntry::builder("find")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| this.find_bar.open()
            ))
            .build();
        let find_next = gio::ActionEntry::builder("find-next")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| this.find_bar.find_next()
            ))
            .build();
        let find_previous = gio::ActionEntry::builder("find-previous")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| this.find_bar.find_previous()
            ))
            .build();
        let preferences = gio::ActionEntry::builder("preferences")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |window: &adw::ApplicationWindow, _, _| {
                    let dialog = PreferencesDialog::present(window);
                    dialog.connect_closed(glib::clone!(
                        #[weak]
                        this,
                        move |_| this.chat.refresh_config()
                    ));
                }
            ))
            .build();
        let toggle_chat = gio::ActionEntry::builder("toggle-chat")
            .activate(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_: &adw::ApplicationWindow, _, _| {
                    this.split.set_show_sidebar(!this.split.shows_sidebar());
                }
            ))
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
            open_uri,
            save,
            save_as,
            find,
            find_next,
            find_previous,
            preferences,
            toggle_chat,
            zoom_in,
            zoom_out,
            zoom_reset,
        ]);
    }

    /// Opens a Markdown file dropped onto the editor, through the unsaved-changes guard.
    fn accept_dropped_files(self: &Rc<Self>, target: &gtk::Overlay) {
        let drop = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| match value.get::<gio::File>() {
                Ok(file) => {
                    this.open_file(file);
                    true
                }
                Err(_) => false,
            }
        ));
        target.add_controller(drop);
    }

    /// Applies `zoom` to the editor and the zoom controls.
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
        let mut state = self.state.borrow_mut();
        state.zoom = Some(zoom.percent());
        let _ = state.save();
    }

    /// Persists `path` at the top of **Open Recent** and its folder as the most recently used
    /// one, so **Open…** starts there next time. Saving the state is a convenience, so a failure
    /// here must not interrupt the user.
    fn remember_file(&self, path: &Path) {
        {
            let mut state = self.state.borrow_mut();
            state.remember_file(path);
            let _ = state.save();
        }
        self.update_recent_menu();
    }

    /// Removes `path` from **Open Recent**, if it is there.
    fn forget_recent(&self, path: &Path) {
        {
            let mut state = self.state.borrow_mut();
            if !state.recent_files.iter().any(|known| known == path) {
                return;
            }
            state.forget_recent(path);
            let _ = state.save();
        }
        self.update_recent_menu();
    }

    /// Rebuilds **Open Recent** from the recent files. Each entry opens its file through
    /// `win.open-uri`, and so through the unsaved-changes guard.
    fn update_recent_menu(&self) {
        // A copy, so no borrow of the state is held while the menu emits `items-changed`.
        let recent_files = self.state.borrow().recent_files.clone();
        let labels = recent_labels(&recent_files, &glib::home_dir());
        self.recent_menu.remove_all();
        for (path, label) in recent_files.iter().zip(labels) {
            let item = gio::MenuItem::new(Some(&label), None);
            let uri = gio::File::for_path(path).uri();
            item.set_action_and_target_value(Some("win.open-uri"), Some(&uri.to_variant()));
            self.recent_menu.append_item(&item);
        }
    }

    fn remembered_folder(&self) -> Option<gio::File> {
        self.state
            .borrow()
            .remembered_folder()
            .map(gio::File::for_path)
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
            .heading("Save Changes?")
            .body(format!(
                "“{}” has unsaved changes. Changes which are not saved will be permanently lost.",
                self.document_name()
            ))
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
        self.format.set(DiskFormat::default());
        self.modified_on_disk.set(None);
        self.update_title();
    }

    async fn open(self: &Rc<Self>) {
        if !self.confirm_discard().await {
            return;
        }
        let dialog = file_dialog("Open Markdown File");
        if let Some(folder) = self.remembered_folder() {
            dialog.set_initial_folder(Some(&folder));
        }
        let Ok(file) = dialog.open_future(Some(&self.window)).await else {
            return;
        };
        let Some(path) = file.path() else {
            self.show_error("Could Not Open File", ONLY_LOCAL_FILES);
            return;
        };
        if let Err(message) = self.load_path(&path) {
            self.show_error("Could Not Open File", &message);
        }
    }

    /// Saves to the open file, or asks for a file name first. Returns true once saved.
    async fn save(self: &Rc<Self>) -> bool {
        let path = self.path.borrow().clone();
        match path {
            Some(path) => self.write_to(path).await,
            None => self.save_as().await,
        }
    }

    async fn save_as(self: &Rc<Self>) -> bool {
        let dialog = file_dialog("Save Markdown File");
        let current = self.path.borrow().clone();
        match current {
            Some(path) => dialog.set_initial_file(Some(&gio::File::for_path(path))),
            None => {
                dialog.set_initial_name(Some("Untitled.md"));
                if let Some(folder) = self.remembered_folder() {
                    dialog.set_initial_folder(Some(&folder));
                }
            }
        }
        let Ok(file) = dialog.save_future(Some(&self.window)).await else {
            return false;
        };
        match file.path() {
            Some(path) => self.write_to(path).await,
            None => {
                self.show_error("Could Not Save File", ONLY_LOCAL_FILES);
                false
            }
        }
    }

    /// Writes the document to `path`. If that is the open file and another program changed it
    /// since it was read, asks before overwriting those changes.
    async fn write_to(self: &Rc<Self>, path: PathBuf) -> bool {
        let is_open_file = self.path.borrow().as_deref() == Some(path.as_path());
        if is_open_file && !self.confirm_overwrite_changed_file(&path).await {
            return false;
        }
        let contents = document::to_disk(&self.editor.text(), self.format.get());
        match document::write_file(&path, &contents) {
            Ok(()) => {
                self.editor.buffer().set_modified(false);
                self.modified_on_disk.set(modification_time(&path));
                self.remember_file(&path);
                self.set_path(path);
                true
            }
            Err(message) => {
                self.show_error("Could Not Save File", &message);
                false
            }
        }
    }

    /// True unless the file changed on disk since it was read and the user chooses to keep the
    /// version on disk.
    async fn confirm_overwrite_changed_file(self: &Rc<Self>, path: &Path) -> bool {
        let (Some(known), Some(current)) = (self.modified_on_disk.get(), modification_time(path))
        else {
            return true;
        };
        if known == current {
            return true;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Overwrite Changed File?")
            .body(format!(
                "“{}” was changed on disk after it was opened here. Saving will overwrite those changes.",
                self.document_name()
            ))
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", "_Cancel"), ("overwrite", "_Overwrite")]);
        dialog.set_response_appearance("overwrite", adw::ResponseAppearance::Destructive);
        dialog.choose_future(Some(&self.window)).await == "overwrite"
    }

    fn set_path(&self, path: PathBuf) {
        *self.path.borrow_mut() = Some(path);
        self.update_title();
    }

    /// The open file's name, or "Untitled".
    fn document_name(&self) -> String {
        self.path
            .borrow()
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    fn update_title(&self) {
        let name = self.document_name();
        let folder = self
            .path
            .borrow()
            .as_deref()
            .and_then(Path::parent)
            .map(|folder| abbreviate_home(folder, &glib::home_dir()))
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

    fn show_error(&self, heading: &str, body: &str) {
        let dialog = adw::AlertDialog::new(Some(heading), Some(body));
        dialog.add_response("ok", "_OK");
        dialog.present(Some(&self.window));
    }
}

const ONLY_LOCAL_FILES: &str = "Only files on this computer can be opened and saved.";

fn modification_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// `folder` with the home directory shortened to `~`, as file managers show it.
fn abbreviate_home(folder: &Path, home: &Path) -> String {
    match folder.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => folder.display().to_string(),
    }
}

/// The longest file name and folder an **Open Recent** entry shows. Menu labels do not
/// ellipsize, and every submenu is as wide as the widest one, so a long entry would widen the
/// whole main menu.
const RECENT_NAME_MAX_CHARS: usize = 30;
const RECENT_FOLDER_MAX_CHARS: usize = 20;

/// The **Open Recent** entries for `paths`: each file's name, plus its folder ("readme.md —
/// ~/blog") when another entry has the same name. Long names are shortened in the middle, long
/// folders at the start. Underscores are doubled because menu labels treat a single one as a
/// mnemonic marker.
fn recent_labels(paths: &[PathBuf], home: &Path) -> Vec<String> {
    let name = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    paths
        .iter()
        .map(|path| {
            let own_name = name(path);
            let mut label = shorten_middle(&own_name, RECENT_NAME_MAX_CHARS);
            if paths.iter().filter(|other| name(other) == own_name).count() > 1 {
                let folder = path
                    .parent()
                    .map(|folder| abbreviate_home(folder, home))
                    .unwrap_or_default();
                label = format!(
                    "{label} — {}",
                    shorten_folder(&folder, RECENT_FOLDER_MAX_CHARS)
                );
            }
            label.replace('_', "__")
        })
        .collect()
}

/// `folder` if it has at most `max` characters, otherwise "…" followed by its end, `max`
/// characters at most: the last folders that fit whole ("…/personal-blog"), or else the end of
/// the last one.
fn shorten_folder(folder: &str, max: usize) -> String {
    let chars: Vec<char> = folder.chars().collect();
    if chars.len() <= max {
        return folder.to_string();
    }
    let tail = &chars[chars.len() - (max - 1)..];
    let start = tail.iter().position(|&c| c == '/').unwrap_or(0);
    format!("…{}", tail[start..].iter().collect::<String>())
}

/// `text` if it has at most `max` characters, otherwise its start and end joined by "…", `max`
/// characters in all.
fn shorten_middle(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let head = (max - 1) / 2;
    let tail = max - 1 - head;
    let mut shortened: String = chars[..head].iter().collect();
    shortened.push('…');
    shortened.extend(&chars[chars.len() - tail..]);
    shortened
}

fn primary_menu(recent: &gio::Menu) -> gio::Menu {
    let file = gio::Menu::new();
    file.append(Some("_New"), Some("win.new"));
    file.append(Some("_Open…"), Some("win.open"));
    file.append_submenu(Some("Open _Recent"), recent);
    file.append(Some("_Save"), Some("win.save"));
    file.append(Some("Save _As…"), Some("win.save-as"));
    let edit = gio::Menu::new();
    edit.append(Some("_Find…"), Some("win.find"));
    let zoom = gio::Menu::new();
    let zoom_item = gio::MenuItem::new(None, None);
    zoom_item.set_attribute_value("custom", Some(&"zoom".to_variant()));
    zoom.append_item(&zoom_item);
    let tools = gio::Menu::new();
    tools.append(Some("_Preferences"), Some("win.preferences"));
    tools.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
    let about = gio::Menu::new();
    about.append(Some("_About Counterpoint"), Some("app.about"));
    let menu = gio::Menu::new();
    menu.append_section(None, &file);
    menu.append_section(None, &edit);
    menu.append_section(None, &zoom);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(paths: &[&str]) -> Vec<String> {
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        recent_labels(&paths, Path::new("/home/u"))
    }

    #[test]
    fn recent_labels_show_only_the_file_name() {
        assert_eq!(
            labels(&["/home/u/blog/post.md", "/srv/notes/todo.md"]),
            ["post.md", "todo.md"]
        );
    }

    #[test]
    fn recent_labels_add_the_folder_when_two_files_share_a_name() {
        assert_eq!(
            labels(&[
                "/home/u/blog/readme.md",
                "/home/u/blog/post.md",
                "/srv/notes/readme.md",
            ]),
            ["readme.md — ~/blog", "post.md", "readme.md — /srv/notes"]
        );
    }

    #[test]
    fn recent_labels_shorten_a_long_name_and_folder_in_the_middle() {
        assert_eq!(
            labels(&[
                "/home/u/Documents/Writing/Blog/2026/drafts-for-review/\
                 a-rather-long-article-title-about-writing.md",
                "/home/u/a-rather-long-article-title-about-writing.md",
            ]),
            [
                "a-rather-long-…bout-writing.md — …/drafts-for-review",
                "a-rather-long-…bout-writing.md — ~",
            ]
        );
    }

    #[test]
    fn recent_labels_cut_a_long_folder_inside_its_last_part_if_that_alone_is_too_long() {
        assert_eq!(
            labels(&[
                "/home/u/an-extremely-long-folder-name/post.md",
                "/srv/post.md"
            ]),
            ["post.md — …ly-long-folder-name", "post.md — /srv"]
        );
    }

    #[test]
    fn recent_labels_double_underscores_so_they_are_not_mnemonics() {
        assert_eq!(
            labels(&["/home/u/my_blog/draft_1.md", "/srv/draft_1.md"]),
            ["draft__1.md — ~/my__blog", "draft__1.md — /srv"]
        );
    }
}
