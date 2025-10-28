use crate::{app::{AppState, UiStatus}, audio::AudioMirrorService, error::AppError};
use hex;
use tauri::{AppHandle, CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem};

const CONFIG_PREFIX: &str = "cfg-";

pub fn build_tray(status: &UiStatus) -> SystemTray {
  SystemTray::new().with_menu(build_menu(status))
}

pub fn rebuild_tray(app: &AppHandle, status: &UiStatus) -> Result<(), AppError> {
  let tray = app.tray_handle();
  tray
    .set_menu(build_menu(status))
    .map_err(|err| AppError::Runtime(format!("Tray update failed: {err}")))?;

  let icon_path: &[u8] = if status.configs.iter().any(|cfg| cfg.active) {
    include_bytes!("../icons/tray-active.png").as_ref()
  } else {
    include_bytes!("../icons/tray-inactive.png").as_ref()
  };

  tray
    .set_icon(tauri::Icon::Raw(icon_path.to_vec()))
    .map_err(|err| AppError::Runtime(format!("Could not set tray icon: {err}")))?;

  Ok(())
}

pub fn handle_event(app: &AppHandle, state: &AppState, event: SystemTrayEvent) {
  if let SystemTrayEvent::MenuItemClick { id, .. } = event {
    let result = if id == "allow-multiple" {
      let current = match state.status() {
        Ok(status) => status.allow_multiple,
        Err(err) => {
          emit_error(app, &err.to_string());
          return;
        }
      };
      state.set_allow_multiple(!current)
    } else if id == "uncheck-all" {
      state.uncheck_all()
    } else if id == "open-window" {
      if let Some(window) = app.get_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
      }
      state.status()
    } else if id == "mirror-start" {
      let service = app.state::<AudioMirrorService>();
      match service.resume() {
        Ok(_) => state.status(),
        Err(err) => Err(err),
      }
    } else if id == "mirror-stop" {
      let service = app.state::<AudioMirrorService>();
      service.stop();
      state.status()
    } else if id == "quit" {
      app.exit(0);
      return;
    } else if let Some(file_name) = decode_id(&id) {
      state.toggle_config(&file_name)
    } else {
      return;
    };

    match result {
      Ok(status) => {
        if let Err(err) = rebuild_tray(app, &status) {
          emit_error(app, &err.to_string());
        }
        if let Err(err) = app.emit_all("state-updated", &status) {
          emit_error(app, &format!("Failed to notify UI: {err}"));
        }
      }
      Err(err) => emit_error(app, &err.to_string()),
    }
  }
}

fn build_menu(status: &UiStatus) -> SystemTrayMenu {
  let mut menu = SystemTrayMenu::new();

  if status.configs.is_empty() {
    menu = menu.add_item(CustomMenuItem::new("empty", "No config files found").disabled());
  } else {
    for cfg in &status.configs {
      let mut item = CustomMenuItem::new(encode_id(&cfg.file_name), cfg.display_name.clone());
      if cfg.active {
        item = item.selected();
      }
      menu = menu.add_item(item);
    }
  }

  menu = menu.add_native_item(SystemTrayMenuItem::Separator);
  menu = menu.add_item(CustomMenuItem::new("mirror-start", "Start Mirroring"));
  menu = menu.add_item(CustomMenuItem::new("mirror-stop", "Stop Mirroring"));
  
  let mut allow_multiple = CustomMenuItem::new("allow-multiple", "Allow Multiple");
  if status.allow_multiple {
    allow_multiple = allow_multiple.selected();
  }
  menu = menu.add_item(allow_multiple);
  menu = menu.add_item(CustomMenuItem::new("uncheck-all", "Uncheck All"));
  menu = menu.add_native_item(SystemTrayMenuItem::Separator);
  menu = menu.add_item(CustomMenuItem::new("open-window", "Open Window"));
  menu = menu.add_item(CustomMenuItem::new("quit", "Quit"));

  menu
}

fn encode_id(file_name: &str) -> String {
  format!("{CONFIG_PREFIX}{}", hex::encode(file_name))
}

fn decode_id(id: &str) -> Option<String> {
  if !id.starts_with(CONFIG_PREFIX) {
    return None;
  }
  let hex_part = &id[CONFIG_PREFIX.len()..];
  let bytes = hex::decode(hex_part).ok()?;
  String::from_utf8(bytes).ok()
}

pub(crate) fn emit_error(app: &AppHandle, message: &str) {
  let _ = app.emit_all("eacs-error", message);
}
