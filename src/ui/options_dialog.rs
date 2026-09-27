//! The Options dialog: LLM endpoint settings and the endpoint's model list.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::config::{self, Config};
use crate::llm;
use crate::model_requests::{self, ModelRequests};

const WORKER_FAILED: &str = "The request failed unexpectedly.";

pub struct OptionsDialog {
    dialog: adw::Dialog,
    base_url: adw::EntryRow,
    api_key: adw::PasswordEntryRow,
    model: adw::EntryRow,
    models: gtk::ListBox,
    models_button: gtk::MenuButton,
    refresh: gtk::Button,
    spinner: adw::Spinner,
    status: gtk::Label,
    requests: RefCell<ModelRequests>,
}

impl OptionsDialog {
    /// Shows the dialog over `parent` with the saved settings and a freshly loaded model list.
    pub fn present(parent: &impl IsA<gtk::Widget>) {
        let base_url = adw::EntryRow::builder()
            .title("Base URL (OpenAI-compatible)")
            .build();
        let api_key = adw::PasswordEntryRow::builder()
            .title("API key (optional)")
            .build();
        let model = adw::EntryRow::builder().title("Model").build();

        let models = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["navigation-sidebar"])
            .build();
        let models_popover = gtk::Popover::builder()
            .child(
                &gtk::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk::PolicyType::Never)
                    .propagate_natural_height(true)
                    .max_content_height(300)
                    .child(&models)
                    .build(),
            )
            .build();
        let models_button = gtk::MenuButton::builder()
            .icon_name("pan-down-symbolic")
            .tooltip_text("Available models")
            .valign(gtk::Align::Center)
            .sensitive(false)
            .popover(&models_popover)
            .css_classes(["flat"])
            .build();
        let refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh the model list")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        model.add_suffix(&models_button);
        model.add_suffix(&refresh);

        let group = adw::PreferencesGroup::new();
        group.add(&base_url);
        group.add(&api_key);
        group.add(&model);

        let spinner = adw::Spinner::builder().visible(false).build();
        let status = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .hexpand(true)
            .css_classes(["dim-label"])
            .build();
        let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        status_row.append(&spinner);
        status_row.append(&status);

        let load_error = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["error"])
            .build();
        let path = config::settings_path()
            .map(|path| format!("Stored in {} (readable only by you).", path.display()))
            .unwrap_or_else(|message| message);
        let path_label = gtk::Label::builder()
            .label(path)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label", "caption"])
            .build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        content.append(&group);
        content.append(&status_row);
        content.append(&load_error);
        content.append(&path_label);

        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::builder()
            .label("Save")
            .css_classes(["suggested-action"])
            .build();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&save);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&content));

        let dialog = adw::Dialog::builder()
            .title("Options")
            .content_width(560)
            .child(&toolbar)
            .build();

        let this = Rc::new(Self {
            dialog,
            base_url,
            api_key,
            model,
            models,
            models_button,
            refresh,
            spinner,
            status,
            requests: RefCell::new(ModelRequests::default()),
        });

        match Config::load() {
            Ok(config) => this.show_config(&config),
            Err(message) => {
                this.show_config(&Config::default());
                load_error.set_text(&message);
                load_error.set_visible(true);
                save.set_label("Overwrite");
            }
        }

        cancel.connect_clicked(glib::clone!(
            #[weak(rename_to = dialog)]
            this.dialog,
            move |_| {
                dialog.close();
            }
        ));
        save.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| this.save()
        ));
        this.refresh.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| this.fetch_models(true)
        ));
        this.models.connect_row_activated(glib::clone!(
            #[weak]
            this,
            move |_, row| {
                if let Some(label) = row.child().and_downcast::<gtk::Label>() {
                    this.model.set_text(&label.text());
                }
                this.models_button.popdown();
            }
        ));
        for row in [
            this.base_url.upcast_ref::<adw::EntryRow>(),
            this.api_key.upcast_ref(),
        ] {
            row.connect_entry_activated(glib::clone!(
                #[weak]
                this,
                move |_| this.fetch_models(false)
            ));
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(
                #[weak]
                this,
                move |_| this.fetch_models(false)
            ));
            row.add_controller(focus);
        }

        // The dialog's widgets only hold weak references; keep the state alive until it closes.
        let keep_alive = RefCell::new(Some(this.clone()));
        this.dialog.connect_closed(move |_| {
            keep_alive.take();
        });

        this.dialog.present(Some(parent));
        this.fetch_models(true);
    }

    fn show_config(&self, config: &Config) {
        self.base_url.set_text(&config.base_url);
        self.api_key
            .set_text(config.api_key.as_deref().unwrap_or(""));
        self.model.set_text(config.model.as_deref().unwrap_or(""));
    }

    fn fetch_models(self: &Rc<Self>, force: bool) {
        let base_url = self.base_url.text();
        let api_key = self.api_key.text();
        let Some(ticket) = self.requests.borrow_mut().begin(&base_url, &api_key, force) else {
            return;
        };
        self.spinner.set_visible(true);
        self.refresh.set_sensitive(false);
        self.status.set_text("Loading models…");

        let config = Config::from_fields(&base_url, &api_key, "");
        let models =
            gio::spawn_blocking(move || llm::list_models(&config).map_err(|e| e.to_string()));
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = models
                .await
                .unwrap_or_else(|_| Err(WORKER_FAILED.to_string()));
            let Some(this) = this.upgrade() else {
                return;
            };
            if this.requests.borrow().is_current(ticket) {
                this.show_models(result);
            }
        });
    }

    fn show_models(&self, result: Result<Vec<String>, String>) {
        self.spinner.set_visible(false);
        self.refresh.set_sensitive(true);
        self.models.remove_all();
        let models = match result {
            Ok(models) => {
                self.status.set_text(&model_requests::summary(models.len()));
                models
            }
            Err(message) => {
                self.status.set_text(&message);
                Vec::new()
            }
        };
        for id in &models {
            self.models
                .append(&gtk::Label::builder().label(id).xalign(0.0).build());
        }
        self.models_button.set_sensitive(!models.is_empty());
    }

    fn save(&self) {
        let config = Config::from_fields(
            &self.base_url.text(),
            &self.api_key.text(),
            &self.model.text(),
        );
        match config.save() {
            Ok(()) => {
                self.dialog.close();
            }
            Err(message) => self.status.set_text(&message),
        }
    }
}
