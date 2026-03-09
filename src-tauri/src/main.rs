#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod error;
mod tray;

use app::{AppState, UiStatus};
use audio::{AudioDevice, AudioMirrorService, MirrorStatus};
use std::process::Command;
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
fn get_audio_settings(service: State<AudioMirrorService>) -> (u32, u32, bool, u32) {
  service.get_sync_settings()
}

#[tauri::command]
fn set_audio_settings(service: State<AudioMirrorService>, warmup_ms: u32, ring_ms: u32, global_delay_ms: u32) {
  service.set_sync_settings(warmup_ms, ring_ms, global_delay_ms);
}

#[tauri::command]
fn set_device_delay(service: State<AudioMirrorService>, device_id: String, delay_ms: i32) {
  service.set_device_delay(device_id, delay_ms);
}

#[tauri::command]
fn get_device_delays(service: State<AudioMirrorService>) -> std::collections::HashMap<String, i32> {
  service.get_device_delays()
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
      get_device_delays,
      set_auto_adjust,
      audio_resync,
      list_audio_devices,
      enable_audio_mirror,
      disable_audio_mirror,
      audio_mirror_status,
      set_render_volume,
      check_vbcable_installed,
      install_vbcable
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

#[tauri::command]
fn check_vbcable_installed(service: State<AudioMirrorService>) -> Result<bool, String> {
  let devices = service.list_devices().map_err(|err| err.to_string())?;
  Ok(devices.iter().any(|d| {
    let name = d.display_name.to_ascii_lowercase();
    name.contains("cable input") || (name.contains("vb-audio") && name.contains("cable"))
  }))
}

#[tauri::command]
fn install_vbcable(service: State<AudioMirrorService>) -> Result<bool, String> {
  #[cfg(not(target_os = "windows"))]
  {
    let _ = service;
    return Err("VB-Cable installation is available only on Windows".into());
  }

  #[cfg(target_os = "windows")]
  {
    let script = r#"
$ErrorActionPreference = 'Stop'
$zip = Join-Path $env:TEMP 'VBCABLE_Driver_Pack43.zip'
$dir = Join-Path $env:TEMP 'VBCABLE_Driver_Pack43_unpacked'
Invoke-WebRequest -Uri 'https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack43.zip' -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $dir -Force
$setup = Join-Path $dir 'VBCABLE_Setup_x64.exe'
if (-not (Test-Path $setup)) { throw 'VB-Cable installer not found after extraction' }
Start-Process -FilePath $setup -Verb RunAs -Wait
"#;

    let status = Command::new("powershell")
      .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
      .status()
      .map_err(|e| format!("Failed to start installer: {e}"))?;

    if !status.success() {
      return Err("VB-Cable installer did not complete successfully".into());
    }

    let devices = service.list_devices().map_err(|err| err.to_string())?;
    Ok(devices.iter().any(|d| {
      let name = d.display_name.to_ascii_lowercase();
      name.contains("cable input") || (name.contains("vb-audio") && name.contains("cable"))
    }))
  }
}
