//! QObject behind the Options dialog: loads and saves the LLM settings and lists the endpoint's
//! models.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, base_url, cxx_name = "baseUrl")]
        #[qproperty(QString, api_key, cxx_name = "apiKey")]
        #[qproperty(QString, model)]
        #[qproperty(QString, models_json, cxx_name = "modelsJson")]
        #[qproperty(bool, loading_models, cxx_name = "loadingModels")]
        #[qproperty(QString, status)]
        #[qproperty(QString, load_error, cxx_name = "loadError")]
        #[qproperty(QString, settings_path, cxx_name = "settingsPath")]
        type SettingsController = super::SettingsControllerRust;

        #[qinvokable]
        fn load(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "refreshModels"]
        fn refresh_models(self: Pin<&mut Self>, base_url: &QString, api_key: &QString);

        #[qinvokable]
        fn save(
            self: Pin<&mut Self>,
            base_url: &QString,
            api_key: &QString,
            model: &QString,
        ) -> bool;
    }

    impl cxx_qt::Threading for SettingsController {}
}

use core::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use crate::config::{self, Config};
use crate::llm;

pub struct SettingsControllerRust {
    base_url: QString,
    api_key: QString,
    model: QString,
    models_json: QString,
    loading_models: bool,
    status: QString,
    load_error: QString,
    settings_path: QString,
    /// Incremented per model request; only the latest request's reply is shown.
    models_generation: u64,
}

impl Default for SettingsControllerRust {
    fn default() -> Self {
        let settings_path = config::settings_path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Self {
            base_url: QString::default(),
            api_key: QString::default(),
            model: QString::default(),
            models_json: QString::from("[]"),
            loading_models: false,
            status: QString::default(),
            load_error: QString::default(),
            settings_path: QString::from(settings_path.as_str()),
            models_generation: 0,
        }
    }
}

impl qobject::SettingsController {
    /// Reloads the saved settings into the properties.
    fn load(mut self: Pin<&mut Self>) {
        match Config::load() {
            Ok(config) => {
                self.as_mut()
                    .set_base_url(QString::from(config.base_url.as_str()));
                self.as_mut()
                    .set_api_key(QString::from(config.api_key.as_deref().unwrap_or("")));
                self.as_mut()
                    .set_model(QString::from(config.model.as_deref().unwrap_or("")));
                self.as_mut().set_load_error(QString::default());
            }
            Err(message) => self
                .as_mut()
                .set_load_error(QString::from(message.as_str())),
        }
    }

    fn refresh_models(mut self: Pin<&mut Self>, base_url: &QString, api_key: &QString) {
        let config = Config::from_fields(&base_url.to_string(), &api_key.to_string(), "");
        self.as_mut().rust_mut().models_generation += 1;
        let generation = self.rust().models_generation;
        self.as_mut().set_loading_models(true);
        self.as_mut().set_status(QString::from("Loading models…"));

        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = llm::list_models(&config);
            // Queueing only fails once the QObject is destroyed; the reply has nowhere to go then.
            let _ = qt_thread.queue(move |mut settings| {
                if settings.rust().models_generation != generation {
                    return;
                }
                settings.as_mut().set_loading_models(false);
                match result {
                    Ok(models) => {
                        let status = match models.len() {
                            0 => "The endpoint lists no models.".to_string(),
                            1 => "1 model available.".to_string(),
                            n => format!("{n} models available."),
                        };
                        let json =
                            serde_json::to_string(&models).expect("model IDs always serialize");
                        settings
                            .as_mut()
                            .set_models_json(QString::from(json.as_str()));
                        settings.as_mut().set_status(QString::from(status.as_str()));
                    }
                    Err(error) => {
                        settings.as_mut().set_models_json(QString::from("[]"));
                        settings
                            .as_mut()
                            .set_status(QString::from(error.to_string().as_str()));
                    }
                }
            });
        });
    }

    fn save(
        mut self: Pin<&mut Self>,
        base_url: &QString,
        api_key: &QString,
        model: &QString,
    ) -> bool {
        let config = Config::from_fields(
            &base_url.to_string(),
            &api_key.to_string(),
            &model.to_string(),
        );
        match config.save() {
            Ok(()) => {
                self.as_mut().load();
                true
            }
            Err(message) => {
                self.as_mut().set_status(QString::from(message.as_str()));
                false
            }
        }
    }
}
