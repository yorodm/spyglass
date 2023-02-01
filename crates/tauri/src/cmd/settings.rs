use std::collections::HashMap;
use std::path::PathBuf;

use tauri::Manager;
use tauri::State;

use shared::config::{Config, Limit, UserSettings};
use shared::form::SettingOpts;

#[tauri::command]
pub async fn save_user_settings(
    window: tauri::Window,
    config: State<'_, Config>,
    settings: HashMap<String, String>,
) -> Result<(), HashMap<String, String>> {
    let mut current_settings = config.user_settings.clone();

    let config_list: Vec<(String, SettingOpts)> = config.user_settings.clone().into();
    let setting_configs: HashMap<String, SettingOpts> = config_list.into_iter().collect();
    let mut errors: HashMap<String, String> = HashMap::new();

    let plugin_configs = config.load_plugin_config();

    let mut fields_updated: usize = 0;

    // Loop through each updated settings value sent from the front-end and
    // validate the values.
    for (key, value) in settings.iter() {
        // Update spyglass or plugin settings?
        if let Some((parent, field)) = key.split_once('.') {
            match parent {
                // Hacky way to update user settings directly.
                "_" => {
                    if let Some(opt) = setting_configs.get(key) {
                        match opt.form_type.validate(value) {
                            Ok(val) => {
                                fields_updated += 1;
                                match field {
                                    "data_directory" => {
                                        current_settings.data_directory = PathBuf::from(val);
                                    }
                                    "disable_autolaunch" => {
                                        current_settings.disable_autolaunch =
                                            serde_json::from_str(value).unwrap_or_default();
                                    }
                                    "disable_telemetry" => {
                                        current_settings.disable_telemetry =
                                            serde_json::from_str(value).unwrap_or_default();
                                    }
                                    "inflight_crawl_limit" => {
                                        let limit: u32 = serde_json::from_str(value).unwrap_or(10);
                                        current_settings.inflight_crawl_limit =
                                            Limit::Finite(limit);
                                    }
                                    "inflight_domain_limit" => {
                                        let limit: u32 = serde_json::from_str(value).unwrap_or(2);
                                        current_settings.inflight_domain_limit =
                                            Limit::Finite(limit);
                                    }
                                    "port" => {
                                        current_settings.port = serde_json::from_str(value)
                                            .unwrap_or_else(|_| UserSettings::default_port());
                                    }
                                    _ => {}
                                }
                            }
                            Err(err) => {
                                errors.insert(key.to_string(), err);
                            }
                        }
                    }
                }
                plugin_name => {
                    // Load plugin settings configurations
                    if let Some(plugin_config) = plugin_configs.get(plugin_name) {
                        let to_update = current_settings
                            .plugin_settings
                            .entry(plugin_name.to_string())
                            .or_default();

                        if let Some(field_opts) = plugin_config.user_settings.get(field) {
                            // Validate & serialize value into something we can save.
                            match field_opts.form_type.validate(value) {
                                Ok(val) => {
                                    fields_updated += 1;
                                    to_update.insert(field.into(), val);
                                }
                                Err(err) => {
                                    errors.insert(key.to_string(), err);
                                }
                            }
                        }
                    } else {
                        errors.insert(key.to_string(), format!("Config not found for {key}"));
                    }
                }
            }
        }
    }

    // Only save settings if everything is valid.
    if errors.is_empty() && fields_updated > 0 {
        let _ = config.save_user_settings(&current_settings);
        let app = window.app_handle();
        app.restart();
        Ok(())
    } else {
        Err(errors)
    }
}

#[tauri::command]
pub async fn load_user_settings(
    _: tauri::Window,
    config: State<'_, Config>,
) -> Result<Vec<(String, SettingOpts)>, String> {
    let current_settings = Config::load_user_settings().expect("Unable to read user settings");

    let plugin_configs = config.load_plugin_config();
    let mut list: Vec<(String, SettingOpts)> = current_settings.clone().into();

    let current_plug_settings = current_settings.plugin_settings;
    for (pname, pconfig) in plugin_configs {
        for (setting_name, setting_opts) in pconfig.user_settings {
            let mut opts = setting_opts.clone();

            let value = current_plug_settings
                .get(&pname)
                .and_then(|settings| settings.get(&setting_name))
                // Reverse backslash escaping
                .map(|value| value.to_string().replace("\\\\", "\\"));

            if let Some(value) = value {
                opts.value = value.to_string();
            }

            list.push((format!("{pname}.{setting_name}"), opts));
        }
    }

    list.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(list)
}