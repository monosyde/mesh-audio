#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod error;
mod tray;

use app::{AppState, UiStatus};
use audio::{AudioDevice, AudioMirrorService, MirrorStatus};
use tauri::{Manager, State, WindowEvent};

fn emit_and_refresh(app: &tauri::AppHandle, status: &UiStatus) {
  if let Err(err) = tray::rebuild_tray(app, status) {
    tray::emit_error(app, &err.to_string());
  }
  if let Err(err) = app.emit_all("state-updated", status) {
    tray::emit_error(app, &format!("Failed to notify UI: {err}"));
  }
}

#[tauri::command]
fn get_status(state: State<AppState>) -> Result<UiStatus, String> {
  state.status().map_err(|err| err.to_string())
}

// profile-management commands removed for mesh-audio

#[tauri::command]
fn get_audio_settings(service: State<AudioMirrorService>) -> (u32, u32) {
  service.get_sync_settings()
}

#[tauri::command]
fn set_audio_settings(service: State<AudioMirrorService>, warmup_ms: u32, ring_ms: u32) {
  service.set_sync_settings(warmup_ms, ring_ms);
}

#[tauri::command]
fn set_device_delay(service: State<AudioMirrorService>, device_id: String, delay_ms: u32) {
  service.set_device_delay(device_id, delay_ms);
}

#[tauri::command]
fn set_auto_adjust(service: State<AudioMirrorService>, enabled: bool) {
  service.set_auto_adjust(enabled);
}

#[tauri::command]
fn audio_resync(service: State<AudioMirrorService>) {
  service.resync_now();
}

fn main() {
  let app_state = match AppState::initialize() {
    Ok(state) => state,
    Err(err) => {
      eprintln!("Failed to initialize: {err}");
      return;
    }
  };

  let initial_status = match app_state.status() {
    Ok(status) => status,
    Err(err) => {
      eprintln!("Failed to read initial status: {err}");
      return;
    }
  };

  let initial_tray = tray::build_tray(&initial_status);

  tauri::Builder::default()
    .manage(app_state)
    .manage(AudioMirrorService::new())
    .system_tray(initial_tray)
    .on_window_event(|event| {
      if let WindowEvent::CloseRequested { api, .. } = event.event() {
        let _ = event.window().hide();
        api.prevent_close();
      }
    })
    .on_system_tray_event(|app, event| {
      let state = app.state::<AppState>();
      tray::handle_event(app, &state, event);
    })
    .setup(|app| {
      let state = app.state::<AppState>();
      let status = state
        .status()
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
      tray::rebuild_tray(&app.handle(), &status)
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_status,
      get_audio_settings,
      set_audio_settings,
      set_device_delay,
      set_auto_adjust,
      audio_resync,
      list_audio_devices,
      enable_audio_mirror,
      disable_audio_mirror,
      audio_mirror_status,
      set_render_volume
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[tauri::command]
fn list_audio_devices(service: State<AudioMirrorService>) -> Result<Vec<AudioDevice>, String> {
  service.list_devices().map_err(|err| err.to_string())
}

#[tauri::command]
fn enable_audio_mirror(service: State<AudioMirrorService>, device_ids: Vec<String>) -> Result<MirrorStatus, String> {
  service.set_targets(device_ids).map_err(|err| err.to_string())?;
  Ok(service.status())
}

#[tauri::command]
fn disable_audio_mirror(service: State<AudioMirrorService>) -> Result<(), String> {
  service.stop();
  Ok(())
}

#[tauri::command]
fn audio_mirror_status(service: State<AudioMirrorService>) -> MirrorStatus {
  service.status()
}

#[tauri::command]
fn set_render_volume(service: State<AudioMirrorService>, device_id: String, volume: f32) -> Result<(), String> {
  service.set_volume(device_id, volume).map_err(|e| e.to_string())
}
