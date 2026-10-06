//! The main window: header bar, editor, chat pane and file handling.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::document::OpenFile;
use crate::state::State;
use crate::ui::chat_pane::ChatPane;
use crate::ui::editor::EditorView;
use crate::ui::file_labels::{abbreviate_home, recent_labels};
use crate::ui::find_bar::FindBar;
use crate::ui::preferences_dialog::PreferencesDialog;
use crate::zoom::Zoom;

/// The window's size until the user resizes it.
const WINDOW_DEFAULT_SIZE: (i32, i32) = (1200, 800);

/// The narrowest the chat pane can be dragged: wide enough for its buttons while it waits for a
/// reply, so that sending a message does not push the divider aside.
const CHAT_MIN_WIDTH: i32 = 300;
/// The chat pane's width until the user drags it.
const CHAT_DEFAULT_WIDTH: i32 = 360;
/// A remembered chat width is restored up to this, so the window's default width still leaves
/// the editor room.
const CHAT_MAX_RESTORED_WIDTH: i32 = 800;

pub struct MainWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    editor: EditorView,
    find_bar: Rc<FindBar>,
    chat: Rc<ChatPane>,
    /// The chat pane as laid out beside the editor; hidden to hide the chat.
    chat_pane: adw::Bin,
    /// The button showing the current zoom percentage, e.g. "100%".
    zoom_label: gtk::Button,
    /// The editor's current zoom level.
    zoom: Cell<Zoom>,
    /// The remembered folder, recent files, zoom level, chat width and window size, loaded once
    /// and saved on every change.
    state: RefCell<State>,
    /// The **Open Recent** submenu, rebuilt whenever the recent files change.
    recent_menu: gio::Menu,
    /// The file the document was read from or last saved to; a new document has none.
    file: RefCell<OpenFile>,
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

        let state = State::load();
        let chat_width = state.chat_width.map_or(CHAT_DEFAULT_WIDTH, |width| {
            i32::try_from(width)
                .unwrap_or(i32::MAX)
                .clamp(CHAT_MIN_WIDTH, CHAT_MAX_RESTORED_WIDTH)
        });
        let chat_pane = adw::Bin::builder()
            .child(chat.widget())
            .css_classes(["sidebar-pane"])
            .width_request(chat_width)
            .build();
        // Only the editor grows and shrinks with the window; the chat keeps the width the user
        // dragged it to.
        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&editor_area)
            .end_child(&chat_pane)
            .resize_start_child(true)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();
        start_chat_at_its_width(&paned, &chat_pane);
        toasts.set_child(Some(&paned));

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
        chat_pane
            .bind_property("visible", &chat_toggle, "active")
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

        let (width, height) = remembered_window_size(&state).unwrap_or(WINDOW_DEFAULT_SIZE);
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(width)
            .default_height(height)
            .maximized(state.window_maximized)
            .content(&toolbar)
            .build();

        let initial_zoom = state.zoom.map(Zoom::from_percent).unwrap_or_default();
        let this = Rc::new(Self {
            window,
            title,
            editor,
            find_bar,
            chat,
            chat_pane,
            zoom_label,
            zoom: Cell::new(Zoom::default()),
            state: RefCell::new(state),
            recent_menu,
            file: RefCell::new(OpenFile::default()),
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
        this.chat.connect_chat_changed(glib::clone!(
            #[weak]
            this,
            move |id| this.remember_open_chat(id)
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

    /// Reopens the file that was open when the app was last used and continues the chat that
    /// was shown with it. A file that can no longer be read is left out silently, as it is from
    /// **Open Recent**, and the window keeps its new document.
    pub fn restore_session(self: &Rc<Self>) {
        let (path, chat) = {
            let state = self.state.borrow();
            (state.open_document.clone(), state.open_chat)
        };
        let Some(path) = path else {
            return;
        };
        if self.load_path(&path).is_err() {
            self.remember_open_document(None);
            return;
        }
        if let Some(chat) = chat {
            self.chat.switch_to_chat(chat);
        }
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
        let (text, file) = OpenFile::read(path).inspect_err(|_| self.forget_recent(path))?;
        self.editor.load(&text);
        self.remember_file(path);
        self.set_file(file);
        self.chat.open_document(path);
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
                    let shown = this.chat_pane.is_visible();
                    if shown {
                        this.remember_chat_width();
                    }
                    this.chat_pane.set_visible(!shown);
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

    /// Applies `change` to the remembered state and saves it if `change` returns true, that is,
    /// if it changed anything, and returns that answer. The state is only a convenience for the
    /// next start, so a failed save is ignored rather than interrupting the user.
    fn update_state(&self, change: impl FnOnce(&mut State) -> bool) -> bool {
        let mut state = self.state.borrow_mut();
        let changed = change(&mut state);
        if changed {
            let _ = state.save();
        }
        changed
    }

    /// Persists the zoom level for the next restart.
    fn remember_zoom(&self, zoom: Zoom) {
        self.update_state(|state| {
            state.zoom = Some(zoom.percent());
            true
        });
    }

    /// Persists the chat pane's width for the next start while the chat is shown, so hiding the
    /// chat or closing the window keeps the width it was dragged to.
    fn remember_chat_width(&self) {
        let Ok(width) = u32::try_from(self.chat_pane.width()) else {
            return;
        };
        if !self.chat_pane.is_visible() || width == 0 {
            return;
        }
        self.update_state(|state| state.chat_width.replace(width) != Some(width));
    }

    /// Persists the window's unmaximized size and whether it is maximized, so the next start
    /// opens the window the same way. GTK keeps the unmaximized size in the default size, so
    /// un-maximizing the next window restores the size it had before it was maximized.
    fn remember_window_size(&self) {
        let (width, height) = self.window.default_size();
        let size = u32::try_from(width)
            .ok()
            .zip(u32::try_from(height).ok())
            .filter(|&(width, height)| width > 0 && height > 0);
        let maximized = self.window.is_maximized();
        self.update_state(|state| {
            let size = size.or(state.window_size);
            let changed = state.window_size != size || state.window_maximized != maximized;
            state.window_size = size;
            state.window_maximized = maximized;
            changed
        });
    }

    /// Persists `path` at the top of **Open Recent** and its folder as the most recently used
    /// one, so **Open…** starts there next time.
    fn remember_file(&self, path: &Path) {
        self.update_state(|state| {
            state.remember_file(path);
            true
        });
        self.update_recent_menu();
    }

    /// Persists the open file, or none for an untitled document, so the next start reopens it.
    fn remember_open_document(&self, path: Option<&Path>) {
        self.update_state(|state| {
            state.remember_open_document(path);
            true
        });
    }

    /// Persists the chat shown in the chat pane, so the next start continues it.
    fn remember_open_chat(&self, id: Option<u64>) {
        self.update_state(|state| std::mem::replace(&mut state.open_chat, id) != id);
    }

    /// Removes `path` from **Open Recent**, if it is there.
    fn forget_recent(&self, path: &Path) {
        let forgotten = self.update_state(|state| {
            let known = state.recent_files.iter().any(|known| known == path);
            if known {
                state.forget_recent(path);
            }
            known
        });
        if forgotten {
            self.update_recent_menu();
        }
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
            self.remember_chat_width();
            self.remember_window_size();
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
                self.file.borrow().name()
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
    /// conversation goes on, but is no longer saved as a chat about the previous document; the
    /// remembered folder and the zoom level are untouched.
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
        self.file.replace(OpenFile::default());
        self.update_title();
        self.remember_open_document(None);
        self.chat.close_document();
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
        let path = self.file.borrow().path().map(Path::to_path_buf);
        match path {
            Some(path) => self.write_to(path).await,
            None => self.save_as().await,
        }
    }

    async fn save_as(self: &Rc<Self>) -> bool {
        let dialog = file_dialog("Save Markdown File");
        let current = self.file.borrow().path().map(Path::to_path_buf);
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
        let is_open_file = self.file.borrow().path() == Some(path.as_path());
        if is_open_file && !self.confirm_overwrite_changed_file().await {
            return false;
        }
        let written = self.file.borrow().write(&path, &self.editor.text());
        match written {
            Ok(file) => {
                self.editor.buffer().set_modified(false);
                self.remember_file(&path);
                self.chat.document_saved(&path);
                self.set_file(file);
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
    async fn confirm_overwrite_changed_file(self: &Rc<Self>) -> bool {
        if !self.file.borrow().changed_on_disk() {
            return true;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Overwrite Changed File?")
            .body(format!(
                "“{}” was changed on disk after it was opened here. Saving will overwrite those changes.",
                self.file.borrow().name()
            ))
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", "_Cancel"), ("overwrite", "_Overwrite")]);
        dialog.set_response_appearance("overwrite", adw::ResponseAppearance::Destructive);
        dialog.choose_future(Some(&self.window)).await == "overwrite"
    }

    fn set_file(&self, file: OpenFile) {
        self.remember_open_document(file.path());
        self.file.replace(file);
        self.update_title();
    }

    fn update_title(&self) {
        let (name, folder) = {
            let file = self.file.borrow();
            let folder = file
                .path()
                .and_then(Path::parent)
                .map(|folder| abbreviate_home(folder, &glib::home_dir()))
                .unwrap_or_default();
            (file.name(), folder)
        };
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

/// The window size remembered in `state`, if there is a usable one.
fn remembered_window_size(state: &State) -> Option<(i32, i32)> {
    let (width, height) = state.window_size?;
    let width = i32::try_from(width).ok().filter(|&width| width > 0)?;
    let height = i32::try_from(height).ok().filter(|&height| height > 0)?;
    Some((width, height))
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

/// Lays the chat out at `chat_pane`'s width request, then lets it be dragged narrower.
///
/// Until its divider has been placed, a `gtk::Paned` gives an end child that does not resize
/// its minimum width, so the chat starts with its intended width as its minimum. After that
/// first layout the divider is pinned where it landed and the minimum drops to
/// `CHAT_MIN_WIDTH`.
fn start_chat_at_its_width(paned: &gtk::Paned, chat_pane: &adw::Bin) {
    paned.connect_position_notify(glib::clone!(
        #[weak]
        chat_pane,
        move |paned| {
            if paned.is_position_set() || chat_pane.width_request() == CHAT_MIN_WIDTH {
                return;
            }
            // The position changes while the paned is being laid out, when it must not be set.
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                paned,
                #[weak]
                chat_pane,
                move || {
                    paned.set_position(paned.position());
                    chat_pane.set_width_request(CHAT_MIN_WIDTH);
                }
            ));
        }
    ));
}
