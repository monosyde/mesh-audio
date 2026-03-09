use crate::error::AppError;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
  pub id: String,
  pub display_name: String,
  pub is_default: bool,
  pub volume: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorStatus {
  pub enabled: bool,
  pub targets: Vec<AudioDevice>,
}

#[cfg(target_os = "windows")]
mod platform {
  use super::{AppError, AudioDevice, MirrorStatus};
  use std::{
    ffi::c_void,
    sync::{
      atomic::{AtomicBool, Ordering},
      Arc,
      Mutex,
    },
    thread,
    time::Duration,
  };
  use windows::{
    core::PCWSTR,
    Win32::{
      Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
      Foundation::{CloseHandle, HANDLE, RPC_E_CHANGED_MODE, WAIT_OBJECT_0, WAIT_TIMEOUT},
      Media::Audio::{
        eConsole,
        eRender,
        EDataFlow,
        ERole,
        AUDCLNT_BUFFERFLAGS_SILENT,
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_E_UNSUPPORTED_FORMAT,
        AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        AUDCLNT_STREAMFLAGS_LOOPBACK,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
        IAudioCaptureClient,
        IAudioClient,
        IAudioRenderClient,
        ISimpleAudioVolume,
        IMMDevice,
        IMMDeviceEnumerator,
        MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
        WAVEFORMATEX,
      },
      System::{
        Com::{
          CoCreateInstance,
          CoInitializeEx,
          CoTaskMemFree,
          CoUninitialize,
          CLSCTX_ALL,
          COINIT_MULTITHREADED,
          StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
          STGM_READ,
        },
        Threading::{CreateEventW, WaitForMultipleObjects},
      },
      UI::Shell::PropertiesSystem::PROPERTYKEY,
    },
  };
  

  

  pub struct AudioMirrorService {
    inner: Arc<AudioMirrorInner>,
  }

  use std::collections::HashMap;

  struct AudioMirrorInner {
    selected_ids: Mutex<Vec<String>>,
    worker: Mutex<Option<MirrorWorker>>,
    volumes: Mutex<HashMap<String, f32>>,
    settings: Mutex<(u32, u32, bool, u32)>, // (warmup_ms, ring_ms, auto_adjust, global_delay_ms)
    delays: Mutex<HashMap<String, i32>>, // signed per-device offset in ms
  }

  impl AudioMirrorService {
    pub fn new() -> Self {
      Self {
        inner: Arc::new(AudioMirrorInner {
          selected_ids: Mutex::new(Vec::new()),
          worker: Mutex::new(None),
          volumes: Mutex::new(HashMap::new()),
          settings: Mutex::new((40, 200, false, 70)),
          delays: Mutex::new(HashMap::new()),
        }),
      }
    }

    pub fn list_devices(&self) -> Result<Vec<AudioDevice>, AppError> {
      unsafe {
        let _com = ComScope::new()?;
        let enumerator: IMMDeviceEnumerator =
          CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(win_to_app_err("CoCreateInstance"))?;

        let default_device = enumerator
          .GetDefaultAudioEndpoint(eRender, eConsole)
          .map_err(win_to_app_err("GetDefaultAudioEndpoint"))?;
        let default_id = get_device_id(&default_device)?;

        let collection = enumerator
          .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
          .map_err(win_to_app_err("EnumAudioEndpoints"))?;

        let count = collection.GetCount().map_err(win_to_app_err("GetCount"))?;
        let mut devices = Vec::with_capacity(count as usize);
        let volumes = self.inner.volumes.lock().unwrap().clone();
        for idx in 0..count {
          let device = collection.Item(idx).map_err(win_to_app_err("Collection::Item"))?;
          let id = get_device_id(&device)?;
          let display_name = match get_device_friendly_name(&device) { Ok(Some(f)) => f, _ => id.clone() };
          let is_default = id == default_id;
          let volume = volumes.get(&id).copied().unwrap_or(1.0);
          devices.push(AudioDevice { is_default, display_name, id, volume });
        }

        Ok(devices)
      }
    }

    pub fn set_targets(&self, device_ids: Vec<String>) -> Result<(), AppError> {
      unsafe {
        // Исключаем системное основное устройство из целей и удаляем дубли
        let _com = ComScope::new()?;
        let enumerator: IMMDeviceEnumerator =
          CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(win_to_app_err("CoCreateInstance"))?;
        let default_device = enumerator
          .GetDefaultAudioEndpoint(eRender, eConsole)
          .map_err(win_to_app_err("GetDefaultAudioEndpoint"))?;
        let default_id = get_device_id(&default_device)?;

        use std::collections::HashSet;
        let mut unique = HashSet::new();
        let filtered: Vec<String> = device_ids
          .into_iter()
          .filter(|id| id != &default_id)
          .filter(|id| unique.insert(id.clone()))
          .collect();

        {
          let mut guard = self.inner.selected_ids.lock().unwrap();
          *guard = filtered.clone();
        }

        let mut worker_guard = self.inner.worker.lock().unwrap();
        if let Some(worker) = worker_guard.as_mut() {
          worker.stop();
        }

        if filtered.is_empty() {
          *worker_guard = None;
          return Ok(());
        }

        let volumes = self.inner.volumes.lock().unwrap().clone();
        let delays = self.inner.delays.lock().unwrap().clone();
        let settings = *self.inner.settings.lock().unwrap();
        let worker = MirrorWorker::spawn(filtered, volumes, delays, settings)?;
        *worker_guard = Some(worker);

        // Registration moved into worker (thread-bound COM)
        Ok(())
      }
    }

    pub fn set_volume(&self, device_id: String, volume: f32) -> Result<(), AppError> {
      let v = volume.clamp(0.0, 1.0);
      self.inner.volumes.lock().unwrap().insert(device_id.clone(), v);
      if let Some(w) = self.inner.worker.lock().unwrap().as_ref() {
        // fire-and-forget to worker
        let _ = w.tx.send(Control::SetVolume(device_id, v));
      }
      Ok(())
    }

    pub fn get_sync_settings(&self) -> (u32, u32, bool, u32) {
      let s = *self.inner.settings.lock().unwrap();
      (s.0, s.1, s.2, s.3)
    }

    pub fn set_sync_settings(&self, warmup_ms: u32, ring_ms: u32, global_delay_ms: u32) {
      let mut s = self.inner.settings.lock().unwrap();
      s.0 = warmup_ms.min(1000);
      s.1 = ring_ms.clamp(100, 2000);
      s.3 = global_delay_ms.min(2000);
      if let Some(w) = self.inner.worker.lock().unwrap().as_ref() {
        let _ = w.tx.send(Control::SetSettings(s.0, s.1, s.3));
      }
    }

    pub fn set_auto_adjust(&self, enabled: bool) {
      let mut s = self.inner.settings.lock().unwrap();
      s.2 = enabled;
      if let Some(w) = self.inner.worker.lock().unwrap().as_ref() {
        let _ = w.tx.send(Control::SetAutoAdjust(enabled));
      }
    }

    pub fn resync_now(&self) {
      if let Some(w) = self.inner.worker.lock().unwrap().as_ref() {
        let _ = w.tx.send(Control::Resync);
      }
    }

    pub fn set_device_delay(&self, device_id: String, delay_ms: i32) {
      let clamped = delay_ms.clamp(-800, 800);
      self.inner.delays.lock().unwrap().insert(device_id.clone(), clamped);
      if let Some(w) = self.inner.worker.lock().unwrap().as_ref() {
        let _ = w.tx.send(Control::SetDelay(device_id, clamped));
      }
    }

    pub fn get_device_delays(&self) -> HashMap<String, i32> {
      self.inner.delays.lock().unwrap().clone()
    }

    pub fn stop(&self) {
      if let Ok(mut worker_guard) = self.inner.worker.lock() {
        if let Some(worker) = worker_guard.as_mut() {
          worker.stop();
        }
        *worker_guard = None;
      }
      if let Ok(mut guard) = self.inner.selected_ids.lock() {
        guard.clear();
      }
    }

    pub fn resume(&self) -> Result<(), AppError> {
      let ids = self.inner.selected_ids.lock().unwrap().clone();
      self.set_targets(ids)
    }

    pub fn status(&self) -> MirrorStatus {
      let selected = self.inner.selected_ids.lock().unwrap().clone();
      let enabled = !selected.is_empty()
        && self
          .inner
          .worker
          .lock()
          .unwrap()
          .as_ref()
          .map(|w| w.is_running())
          .unwrap_or(false);
      let targets = self
        .list_devices()
        .unwrap_or_default()
        .into_iter()
        .filter(|dev| selected.iter().any(|id| id == &dev.id))
        .collect();

      MirrorStatus { enabled, targets }
    }
  }

  struct MirrorWorker {
    stop_flag: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    tx: std::sync::mpsc::Sender<Control>,
  }

  enum Control {
    SetVolume(String, f32),
    DefaultChanged,
    SetDelay(String, i32),
    SetSettings(u32, u32, u32),
    SetAutoAdjust(bool),
    Resync,
  }

  // Raw COM sink implementation to avoid windows version conflicts
  use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
  use windows::core::{GUID, IUnknown, IUnknown_Vtbl, HRESULT, Interface};
  use windows::Win32::Media::Audio::IMMNotificationClient_Vtbl;

  #[repr(C)]
  struct NotificationSinkRaw {
    vtbl: *const IMMNotificationClient_Vtbl,
    ref_count: AtomicU32,
    tx: std::sync::mpsc::Sender<Control>,
  }

  unsafe extern "system" fn sink_query_interface(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() || riid.is_null() { return HRESULT(-2147467261); /* E_POINTER */ }
    *ppv = std::ptr::null_mut();
    let iid = &*riid;
    if *iid == <IUnknown as Interface>::IID || *iid == IAudio_IID_IMMNotificationClient() {
      sink_add_ref(this);
      *ppv = this;
      return HRESULT(0); // S_OK
    }
    HRESULT(-2147467262) // E_NOINTERFACE
  }

  unsafe extern "system" fn sink_add_ref(this: *mut c_void) -> u32 {
    let obj = this as *mut NotificationSinkRaw;
    (*obj).ref_count.fetch_add(1, AtomicOrdering::Relaxed) + 1
  }

  unsafe extern "system" fn sink_release(this: *mut c_void) -> u32 {
    let obj = this as *mut NotificationSinkRaw;
    let prev = (*obj).ref_count.fetch_sub(1, AtomicOrdering::Release);
    if prev == 1 {
      std::sync::atomic::fence(AtomicOrdering::Acquire);
      drop(Box::from_raw(obj));
      0
    } else {
      prev - 1
    }
  }

  // Helpers to get IID for IMMNotificationClient at runtime
  #[inline]
  fn IAudio_IID_IMMNotificationClient() -> GUID { windows::Win32::Media::Audio::IMMNotificationClient::IID }

  use windows::Win32::Media::Audio::DEVICE_STATE;
  unsafe extern "system" fn sink_on_dev_state_changed(_this: *mut c_void, _id: PCWSTR, _state: DEVICE_STATE) -> HRESULT { HRESULT(0) }
  unsafe extern "system" fn sink_on_added(_this: *mut c_void, _id: PCWSTR) -> HRESULT { HRESULT(0) }
  unsafe extern "system" fn sink_on_removed(_this: *mut c_void, _id: PCWSTR) -> HRESULT { HRESULT(0) }
  unsafe extern "system" fn sink_on_prop_changed(_this: *mut c_void, _id: PCWSTR, _key: windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY) -> HRESULT { HRESULT(0) }
  unsafe extern "system" fn sink_on_default_changed(this: *mut c_void, _flow: EDataFlow, _role: ERole, _id: PCWSTR) -> HRESULT {
    let obj = this as *mut NotificationSinkRaw;
    let _ = (*obj).tx.send(Control::DefaultChanged);
    HRESULT(0)
  }

  static SINK_VTBL: IMMNotificationClient_Vtbl = IMMNotificationClient_Vtbl {
    base__: IUnknown_Vtbl {
      QueryInterface: sink_query_interface,
      AddRef: sink_add_ref,
      Release: sink_release,
    },
    OnDeviceStateChanged: sink_on_dev_state_changed,
    OnDeviceAdded: sink_on_added,
    OnDeviceRemoved: sink_on_removed,
    OnDefaultDeviceChanged: sink_on_default_changed,
    OnPropertyValueChanged: sink_on_prop_changed,
  };


  impl MirrorWorker {
    fn spawn(
      device_ids: Vec<String>,
      initial_volumes: std::collections::HashMap<String, f32>,
      initial_delays: std::collections::HashMap<String, i32>,
      settings: (u32, u32, bool, u32),
    ) -> Result<Self, AppError> {
      let stop_flag = Arc::new(AtomicBool::new(false));
      let stop_clone = stop_flag.clone();
      let running = Arc::new(AtomicBool::new(true));
      let running_clone = running.clone();
      let (tx, rx) = std::sync::mpsc::channel::<Control>();
      let tx_clone = tx.clone();
      let handle = thread::spawn(move || {
        if let Err(err) = unsafe { run_loop(device_ids, initial_volumes, initial_delays, settings, stop_clone, rx, tx_clone) } {
          eprintln!("Audio mirror thread failed: {err}");
        }
        running_clone.store(false, Ordering::SeqCst);
      });

      Ok(Self { stop_flag, running, handle: Some(handle), tx })
    }

    fn set_volume(&self, id: String, v: f32) {
      let _ = self.tx.send(Control::SetVolume(id, v));
    }

    fn stop(&mut self) {
      self.stop_flag.store(true, Ordering::SeqCst);
      if let Some(handle) = self.handle.take() {
        let _ = handle.join();
      }
      self.running.store(false, Ordering::SeqCst);
    }

    fn is_running(&self) -> bool {
      self.running.load(Ordering::SeqCst)
    }
  }

  impl Drop for MirrorWorker {
    fn drop(&mut self) {
      self.stop();
    }
  }

  struct RingBuffer {
    buf: Vec<u8>,
    cap: usize,
    start: usize,
    len: usize,
    // multi-reader cursors (bytes from start)
    readers: Vec<usize>,
  }

  impl RingBuffer {
    fn new(capacity: usize) -> Self {
      let cap = capacity.max(1);
      Self { buf: vec![0u8; cap], cap, start: 0, len: 0, readers: Vec::new() }
    }

    fn ensure_reader(&mut self, idx: usize) {
      if idx >= self.readers.len() {
        self.readers.resize(idx + 1, 0);
      }
    }

    fn push(&mut self, data: &[u8]) {
      if data.is_empty() { return; }
      if data.len() >= self.cap {
        let tail = &data[data.len() - self.cap..];
        self.buf[..].copy_from_slice(tail);
        self.start = 0;
        self.len = self.cap;
        for reader in &mut self.readers {
          *reader = self.len;
        }
        return;
      }
      while self.len + data.len() > self.cap {
        let drop_len = (self.len + data.len()) - self.cap;
        self.start = (self.start + drop_len) % self.cap;
        self.len -= drop_len;
        for reader in &mut self.readers {
          *reader = reader.saturating_sub(drop_len);
          if *reader > self.len {
            *reader = self.len;
          }
        }
      }
      let end = (self.start + self.len) % self.cap;
      let first = (self.cap - end).min(data.len());
      self.buf[end..end + first].copy_from_slice(&data[..first]);
      let remain = data.len() - first;
      if remain > 0 { self.buf[..remain].copy_from_slice(&data[first..]); }
      self.len += data.len();
    }

    fn push_silence(&mut self, bytes: usize) {
      if bytes == 0 { return; }
      let zeros = vec![0u8; bytes];
      self.push(&zeros);
    }

    fn read_for(&mut self, reader_idx: usize, want: usize) -> Vec<u8> {
      self.ensure_reader(reader_idx);
      let read_offset = self.readers[reader_idx];
      let available = self.len.saturating_sub(read_offset);
      let take = want.min(available);
      if take == 0 { return Vec::new(); }
      // absolute start in buffer for this reader
      let abs = (self.start + read_offset) % self.cap;
      let mut out = vec![0u8; take];
      let first = (self.cap - abs).min(take);
      out[..first].copy_from_slice(&self.buf[abs..abs + first]);
      let remain = take - first;
      if remain > 0 { out[first..].copy_from_slice(&self.buf[..remain]); }
      // advance reader
      self.readers[reader_idx] = read_offset + take;
      out
    }

    fn len_bytes(&self) -> usize { self.len }

    fn available_for(&mut self, reader_idx: usize) -> usize {
      self.ensure_reader(reader_idx);
      self.len.saturating_sub(self.readers[reader_idx])
    }

    fn skip_for(&mut self, reader_idx: usize, bytes: usize) -> usize {
      self.ensure_reader(reader_idx);
      let available = self.len.saturating_sub(self.readers[reader_idx]);
      let take = bytes.min(available);
      self.readers[reader_idx] += take;
      take
    }

    fn gc(&mut self) {
      if self.readers.is_empty() { return; }
      let min_read = *self.readers.iter().min().unwrap_or(&0);
      if min_read == 0 { return; }
      // drop min_read bytes from head
      self.start = (self.start + min_read) % self.cap;
      self.len -= min_read;
      for r in &mut self.readers { *r -= min_read; }
    }
  }

  struct ComScope {
    should_uninit: bool,
  }

  impl ComScope {
    fn new() -> Result<Self, AppError> {
      unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_ok() {
          Ok(Self { should_uninit: true })
        } else if hr == RPC_E_CHANGED_MODE {
          Ok(Self { should_uninit: false })
        } else {
          Err(AppError::Runtime(format!("COM initialization failed: {hr}")))
        }
      }
    }
  }

  impl Drop for ComScope {
    fn drop(&mut self) {
      if self.should_uninit {
        unsafe {
          CoUninitialize();
        }
      }
    }
  }

  struct CaptureDevice {
    client: IAudioClient,
    capture_client: IAudioCaptureClient,
    frame_size: u32,
    event: HANDLE,
    format: *const WAVEFORMATEX,
  }

  impl CaptureDevice {
    fn start(&self) -> Result<(), AppError> {
      unsafe { self.client.Start().map_err(win_to_app_err("Capture Start")) }
    }

    fn stop(&self) {
      unsafe {
        let _ = self.client.Stop();
      }
    }
  }

  impl Drop for CaptureDevice {
    fn drop(&mut self) {
      unsafe {
        if !self.event.is_invalid() {
          let _ = CloseHandle(self.event);
        }
        if !self.format.is_null() {
          CoTaskMemFree(Some(self.format as *const c_void));
        }
      }
    }
  }

  struct RenderDevice {
    client: IAudioClient,
    render_client: IAudioRenderClient,
    simple_volume: ISimpleAudioVolume,
    event: HANDLE,
    frame_size: u32,
    buffer_size: u32,
  }

  impl RenderDevice {
    fn start(&self) -> Result<(), AppError> {
      unsafe { self.client.Start().map_err(win_to_app_err("Render Start")) }
    }

    fn stop(&self) {
      unsafe {
        let _ = self.client.Stop();
      }
    }

    fn write(&self, data: Option<&[u8]>, frames: u32) -> Result<(), AppError> {
      let frame_bytes = self.frame_size as usize;
      let mut remaining = frames;
      let mut offset = 0usize;

      while remaining > 0 {
        let padding = unsafe { self.client.GetCurrentPadding() }.map_err(win_to_app_err("GetCurrentPadding"))?;
        let available = self.buffer_size.saturating_sub(padding);

        if available == 0 {
          thread::sleep(Duration::from_millis(2));
          continue;
        }

        let chunk = remaining.min(available);
        let chunk_bytes = chunk as usize * frame_bytes;

        let buffer = unsafe {
          self
            .render_client
            .GetBuffer(chunk)
            .map_err(win_to_app_err("Render GetBuffer"))?
        };

        unsafe {
          if let Some(payload) = data {
            let end = offset + chunk_bytes;
            let slice = &payload[offset..end];
            std::ptr::copy_nonoverlapping(slice.as_ptr(), buffer, chunk_bytes);
          } else {
            std::ptr::write_bytes(buffer, 0, chunk_bytes);
          }
        }

        unsafe {
          self
            .render_client
            .ReleaseBuffer(chunk, 0)
            .map_err(win_to_app_err("Render ReleaseBuffer"))?;
        }

        remaining -= chunk;
        offset += chunk_bytes;
      }

      Ok(())
    }

    fn set_volume(&self, v: f32) -> Result<(), AppError> {
      unsafe { self.simple_volume.SetMasterVolume(v, std::ptr::null()) }
        .map_err(win_to_app_err("SetMasterVolume"))
    }
  }

  impl Drop for RenderDevice {
    fn drop(&mut self) {
      unsafe {
        let _ = self.client.Stop();
        if !self.event.is_invalid() { let _ = CloseHandle(self.event); }
      }
    }
  }

  unsafe fn run_loop(
    device_ids: Vec<String>,
    initial_volumes: std::collections::HashMap<String, f32>,
    mut device_delays: std::collections::HashMap<String, i32>,
    settings: (u32, u32, bool, u32),
    stop_flag: Arc<AtomicBool>,
    rx: std::sync::mpsc::Receiver<Control>,
    tx: std::sync::mpsc::Sender<Control>,
  ) -> Result<(), AppError> {
    let _com = ComScope::new()?;
    let enumerator: IMMDeviceEnumerator =
      CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(win_to_app_err("CoCreateInstance"))?;

    // Register callback for default device changes; keep sink alive for the loop scope
    // Allocate raw COM object
    let sink_raw = Box::new(NotificationSinkRaw { vtbl: &SINK_VTBL as *const _, ref_count: AtomicU32::new(1), tx: tx.clone() });
    let sink_ptr = Box::into_raw(sink_raw) as *mut c_void;
    let sink_iface: windows::Win32::Media::Audio::IMMNotificationClient = windows::Win32::Media::Audio::IMMNotificationClient::from_raw(sink_ptr);
    let _ = enumerator.RegisterEndpointNotificationCallback(&sink_iface);

    let default_device = enumerator
      .GetDefaultAudioEndpoint(eRender, eConsole)
      .map_err(win_to_app_err("GetDefaultAudioEndpoint"))?;
    let mut capture = initialize_capture(&default_device)?;
    let mut last_default_id = get_device_id(&default_device)?;
    let mut last_check = std::time::Instant::now();

    let mut warmup_ms = settings.0.min(1000);
    let mut ring_ms = settings.1.clamp(100, 2000);
    let mut auto_adjust = settings.2;
    let mut global_delay_ms = settings.3.min(2000);

    let mut renderers: Vec<RenderDevice> = Vec::new();
    let mut target_ids: Vec<String> = Vec::new();
    for id in device_ids {
      let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
      match enumerator.GetDevice(PCWSTR(wide.as_ptr())) {
      Ok(device) => match initialize_renderer(&device, capture.format)? {
        Some(renderer) => { if let Some(v)=initial_volumes.get(&id){ let _=renderer.set_volume(*v);} target_ids.push(id.clone()); renderers.push(renderer) },
        None => eprintln!("Устройство `{id}` пропущено — несовместимый формат или параметры канала"),
      },
        Err(err) => eprintln!("Failed to open render device `{}`: {err}", id),
      }
    }

    if renderers.is_empty() {
      return Err(AppError::Runtime(
        "Не найдено совместимых устройств для дублирования".into(),
      ));
    }

    // Prepare event array: capture + renderers
    let mut handles: Vec<HANDLE> = Vec::with_capacity(1 + renderers.len());
    handles.push(capture.event);
    for r in &renderers { handles.push(r.event); }

    // Ring buffer ~ configurable ms
    let mut frame_bytes = capture.frame_size as usize;
    let mut sample_rate = (*capture.format).nSamplesPerSec as usize;
    let mut ring = RingBuffer::new(ring_capacity_bytes(sample_rate, frame_bytes, ring_ms));

    capture.start()?;
    warmup_capture(&capture, &mut ring, frame_bytes, sample_rate, warmup_ms)?;

    for (idx, renderer) in renderers.iter().enumerate() {
      renderer.start()?;
      if let Some(offset_ms) = device_delays.get(&target_ids[idx]).copied() {
        let target_ms = (global_delay_ms as i64 + offset_ms as i64).max(0) as u32;
        if target_ms > 0 {
          let silence_frames = ((*capture.format).nSamplesPerSec as u64 * target_ms as u64 / 1000) as u32;
          if silence_frames > 0 {
            let _ = renderer.write(None, silence_frames);
          }
        }
      }
    }

    while !stop_flag.load(Ordering::SeqCst) {
      if last_check.elapsed() >= Duration::from_millis(120) {
        if let Ok(cur_dev) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
          if let Ok(cur_id) = get_device_id(&cur_dev) {
            if cur_id != last_default_id {
              capture.stop();
              capture = initialize_capture(&cur_dev)?;
              handles[0] = capture.event;
              frame_bytes = capture.frame_size as usize;
              sample_rate = (*capture.format).nSamplesPerSec as usize;
              ring = RingBuffer::new(ring_capacity_bytes(sample_rate, frame_bytes, ring_ms));
              capture.start()?;
              warmup_capture(&capture, &mut ring, frame_bytes, sample_rate, warmup_ms)?;
              last_default_id = cur_id;
            }
          }
        }
        last_check = std::time::Instant::now();
      }
      while let Ok(cmd) = rx.try_recv() {
        match cmd {
          Control::SetVolume(id, v) => {
            for (idx, rid) in target_ids.iter().enumerate() {
              if *rid == id {
                let _ = renderers[idx].set_volume(v);
              }
            }
          }
          Control::DefaultChanged => {
            // Пересоздаём loopback на новом default
            let new_default = enumerator
              .GetDefaultAudioEndpoint(eRender, eConsole)
              .map_err(win_to_app_err("GetDefaultAudioEndpoint"))?;
            capture.stop();
            capture = initialize_capture(&new_default)?;
            handles[0] = capture.event;
            frame_bytes = capture.frame_size as usize;
            sample_rate = (*capture.format).nSamplesPerSec as usize;
            ring = RingBuffer::new(ring_capacity_bytes(sample_rate, frame_bytes, ring_ms));
            capture.start()?;
            warmup_capture(&capture, &mut ring, frame_bytes, sample_rate, warmup_ms)?;
          }
          Control::SetDelay(id, ms) => {
            device_delays.insert(id.clone(), ms);
            for (idx, rid) in target_ids.iter().enumerate() {
              if *rid == id {
                let target_ms = (global_delay_ms as i64 + ms as i64).max(0) as u32;
                let frames = ((*capture.format).nSamplesPerSec as u64 * target_ms as u64 / 1000) as u32;
                if frames > 0 { let _ = renderers[idx].write(None, frames); }
              }
            }
          }
          Control::SetSettings(new_warmup, new_ring, new_global_delay) => {
            warmup_ms = new_warmup.min(1000);
            ring_ms = new_ring.clamp(100, 2000);
            global_delay_ms = new_global_delay.min(2000);
            let cur_dev = enumerator
              .GetDefaultAudioEndpoint(eRender, eConsole)
              .map_err(win_to_app_err("GetDefaultAudioEndpoint"))?;
            capture.stop();
            capture = initialize_capture(&cur_dev)?;
            handles[0] = capture.event;
            frame_bytes = capture.frame_size as usize;
            sample_rate = (*capture.format).nSamplesPerSec as usize;
            ring = RingBuffer::new(ring_capacity_bytes(sample_rate, frame_bytes, ring_ms));
            capture.start()?;
            warmup_capture(&capture, &mut ring, frame_bytes, sample_rate, warmup_ms)?;
          }
          Control::SetAutoAdjust(enabled) => {
            auto_adjust = enabled;
          }
          Control::Resync => {
            let cur_dev = enumerator
              .GetDefaultAudioEndpoint(eRender, eConsole)
              .map_err(win_to_app_err("GetDefaultAudioEndpoint"))?;
            capture.stop();
            capture = initialize_capture(&cur_dev)?;
            handles[0] = capture.event;
            frame_bytes = capture.frame_size as usize;
            sample_rate = (*capture.format).nSamplesPerSec as usize;
            ring = RingBuffer::new(ring_capacity_bytes(sample_rate, frame_bytes, ring_ms));
            capture.start()?;
            warmup_capture(&capture, &mut ring, frame_bytes, sample_rate, warmup_ms)?;
          }
        }
      }
      let result = WaitForMultipleObjects(&handles, false, 100);
      if result == WAIT_TIMEOUT { continue; }
      let index = (result.0 - WAIT_OBJECT_0.0) as usize;
      if index >= handles.len() { continue; }

      if index == 0 {
        // capture
        loop {
          let packet = capture
            .capture_client
            .GetNextPacketSize()
            .map_err(win_to_app_err("GetNextPacketSize"))?;
          if packet == 0 {
            break;
          }

          let mut data_ptr: *mut u8 = std::ptr::null_mut();
          let mut frames = 0;
          let mut flags = 0;
          capture
            .capture_client
            .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
            .map_err(win_to_app_err("GetBuffer"))?;

          let bytes = frames as usize * frame_bytes;
          if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data_ptr.is_null() {
            ring.push_silence(bytes);
          } else {
            let slice = std::slice::from_raw_parts(data_ptr as *const u8, bytes);
            ring.push(slice);
          }

          capture
            .capture_client
            .ReleaseBuffer(frames)
            .map_err(win_to_app_err("ReleaseBuffer"))?;
        }
        // GC after capture burst
        ring.gc();
      } else {
        // render
        let ridx = index - 1;
        if let Some(r) = renderers.get(ridx) {
          let padding = r.client.GetCurrentPadding().map_err(win_to_app_err("GetCurrentPadding"))?;
          let available = r.buffer_size.saturating_sub(padding);
          if available > 0 {
            let frame_bytes = r.frame_size as usize;
            let need_bytes = available as usize * frame_bytes;

            let mut silence_prefix_bytes = 0usize;
            let offset_ms = device_delays.get(&target_ids[ridx]).copied().unwrap_or(0);
            let base_target_ms = global_delay_ms.max(warmup_ms.max(35)).min(ring_ms.saturating_sub(20).max(35));
            let desired_total_ms = (base_target_ms as i64 + offset_ms as i64)
              .clamp(20, ring_ms.saturating_sub(10).max(20) as i64) as u64;
            let desired_delay_bytes =
              ((sample_rate as u64 * desired_total_ms / 1000) as usize) * frame_bytes;
            let tolerance_ms = if auto_adjust { 12 } else { 6 };
            let max_adjust_ms = if auto_adjust { 8 } else { 4 };
            let tolerance_bytes = ((sample_rate as u64 * tolerance_ms / 1000) as usize) * frame_bytes;
            let max_adjust_bytes = ((sample_rate as u64 * max_adjust_ms / 1000) as usize) * frame_bytes;
            let min_headroom_bytes = ((sample_rate as u64 * 15 / 1000) as usize) * frame_bytes;

            let current_available = ring.available_for(ridx);
            if current_available > desired_delay_bytes.saturating_add(tolerance_bytes) {
              let excess = current_available - desired_delay_bytes;
              let to_skip = excess.min(max_adjust_bytes);
              let max_skippable = current_available.saturating_sub(min_headroom_bytes);
              let aligned = (to_skip.min(max_skippable) / frame_bytes) * frame_bytes;
              if aligned > 0 {
                let _ = ring.skip_for(ridx, aligned);
              }
            } else if current_available.saturating_add(tolerance_bytes) < desired_delay_bytes {
              let deficit = desired_delay_bytes - current_available;
              let inject = deficit.min(max_adjust_bytes).min(need_bytes / 4);
              silence_prefix_bytes = (inject / frame_bytes) * frame_bytes;
            }

            let data_need_bytes = need_bytes.saturating_sub(silence_prefix_bytes);
            let chunk = ring.read_for(ridx, data_need_bytes);
            if chunk.is_empty() {
              if auto_adjust {
                thread::sleep(Duration::from_millis(1));
              }
              r.write(None, available)?;
            } else {
              if silence_prefix_bytes > 0 {
                let silence_frames = (silence_prefix_bytes / frame_bytes) as u32;
                if silence_frames > 0 {
                  r.write(None, silence_frames)?;
                }
              }
              let frames = (chunk.len() / frame_bytes) as u32;
              r.write(Some(&chunk), frames)?;
              let written_bytes = frames as usize * frame_bytes + silence_prefix_bytes;
              if written_bytes < need_bytes {
                let remain_frames = ((need_bytes - written_bytes) / frame_bytes) as u32;
                if remain_frames > 0 { r.write(None, remain_frames)?; }
              }
            }
          }
        }
      }
    }

    capture.stop();
    for renderer in &renderers {
      renderer.stop();
    }

    // Unregister and finish
    let _ = enumerator.UnregisterEndpointNotificationCallback(&sink_iface);
    // Loop finished cleanly
    Ok(())
  }

  fn ring_capacity_bytes(sample_rate: usize, frame_bytes: usize, ring_ms: u32) -> usize {
    let bytes = ((sample_rate as u64 * ring_ms as u64 / 1000) as usize) * frame_bytes;
    bytes.max(frame_bytes.max(1))
  }

  unsafe fn warmup_capture(
    capture: &CaptureDevice,
    ring: &mut RingBuffer,
    frame_bytes: usize,
    sample_rate: usize,
    warmup_ms: u32,
  ) -> Result<(), AppError> {
    let warmup_bytes_target = ((sample_rate * warmup_ms as usize / 1000) * frame_bytes) as usize;
    let warmup_deadline = std::time::Instant::now() + Duration::from_millis(120);
    while ring.len_bytes() < warmup_bytes_target && std::time::Instant::now() < warmup_deadline {
      let res = WaitForMultipleObjects(&[capture.event], false, 20);
      if res == WAIT_TIMEOUT {
        continue;
      }

      loop {
        let packet = capture
          .capture_client
          .GetNextPacketSize()
          .map_err(win_to_app_err("GetNextPacketSize"))?;
        if packet == 0 {
          break;
        }

        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut frames = 0u32;
        let mut flags = 0u32;
        capture
          .capture_client
          .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
          .map_err(win_to_app_err("GetBuffer"))?;

        let bytes = frames as usize * frame_bytes;
        if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data_ptr.is_null() {
          ring.push_silence(bytes);
        } else {
          let slice = std::slice::from_raw_parts(data_ptr as *const u8, bytes);
          ring.push(slice);
        }

        capture
          .capture_client
          .ReleaseBuffer(frames)
          .map_err(win_to_app_err("ReleaseBuffer"))?;
      }
    }

    Ok(())
  }

  unsafe fn initialize_capture(device: &IMMDevice) -> Result<CaptureDevice, AppError> {
    let client: IAudioClient = device
      .Activate::<IAudioClient>(CLSCTX_ALL, None)
      .map_err(win_to_app_err("Activate capture"))?;

    let fmt_ptr = client
      .GetMixFormat()
      .map_err(win_to_app_err("GetMixFormat"))?;
    let format = fmt_ptr as *const WAVEFORMATEX;
    let frame_size = (*format).nBlockAlign as u32;

    let event = CreateEventW(None, false, false, PCWSTR::null())
      .map_err(win_to_app_err("CreateEvent"))?;
    if event.is_invalid() {
      CoTaskMemFree(Some(fmt_ptr as *const c_void));
      return Err(AppError::Runtime("Не удалось создать событие для loopback".into()));
    }

    client
      .Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        0,
        0,
        format,
        None,
      )
      .map_err(win_to_app_err("Capture Initialize"))?;

    client
      .SetEventHandle(event)
      .map_err(win_to_app_err("SetEventHandle"))?;

    let capture_client: IAudioCaptureClient = client
      .GetService::<IAudioCaptureClient>()
      .map_err(win_to_app_err("GetService capture"))?;

    Ok(CaptureDevice {
      client,
      capture_client,
      frame_size,
      event,
      format,
    })
  }

  unsafe fn initialize_renderer(
    device: &IMMDevice,
    source_format: *const WAVEFORMATEX,
  ) -> Result<Option<RenderDevice>, AppError> {
    let device_id = get_device_id(device)?;
    let client: IAudioClient = device
      .Activate::<IAudioClient>(CLSCTX_ALL, None)
      .map_err(win_to_app_err("Activate render"))?;

    let fmt_ptr = client
      .GetMixFormat()
      .map_err(win_to_app_err("Render GetMixFormat"))?;
    let device_format = &*(fmt_ptr as *const WAVEFORMATEX);
    let source = &*source_format;
    CoTaskMemFree(Some(fmt_ptr as *const c_void));

    let device_channels = device_format.nChannels;
    let source_channels = source.nChannels;
    let source_rate = source.nSamplesPerSec;

    // Разрешаем различие по числу каналов в shared-режиме — Windows миксер выполнит up/down-mix
    if source_channels != device_channels {
      eprintln!(
        "Внимание: `{device_id}` имеет {} каналов, источник {} — выполняется микширование Windows",
        device_channels,
        source_channels
      );
    }

    if let Err(err) = client.Initialize(
      AUDCLNT_SHAREMODE_SHARED,
      AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
      0,
      0,
      source_format,
      None,
    ) {
      if err.code() == AUDCLNT_E_UNSUPPORTED_FORMAT {
        eprintln!(
          "Пропуск `{device_id}`: формат источника {} Гц не поддержан. Код {:#x}",
          source_rate,
          err.code().0
        );
        return Ok(None);
      }
      return Err(win_to_app_err("Render Initialize")(err));
    }

    let render_client: IAudioRenderClient = client
      .GetService::<IAudioRenderClient>()
      .map_err(win_to_app_err("GetService render"))?;

    let simple_volume: ISimpleAudioVolume = client
      .GetService::<ISimpleAudioVolume>()
      .map_err(win_to_app_err("GetService simple volume"))?;

    // Create event for event-driven rendering
    let render_event = CreateEventW(None, false, false, PCWSTR::null())
      .map_err(win_to_app_err("CreateEvent render"))?;
    if render_event.is_invalid() {
      return Err(AppError::Runtime("Не удалось создать событие для render".into()));
    }
    client
      .SetEventHandle(render_event)
      .map_err(win_to_app_err("Render SetEventHandle"))?;

    let buffer_size = client.GetBufferSize().map_err(win_to_app_err("GetBufferSize"))?;

  Ok(Some(RenderDevice {
      client,
      render_client,
      simple_volume,
      event: render_event,
      frame_size: source.nBlockAlign as u32,
      buffer_size,
    }))
  }

  unsafe fn get_device_friendly_name(device: &IMMDevice) -> Result<Option<String>, AppError> {
    let store = device
      .OpenPropertyStore(STGM_READ)
      .map_err(win_to_app_err("OpenPropertyStore"))?;
    let mut value = store
      .GetValue(&PKEY_Device_FriendlyName)
      .map_err(win_to_app_err("GetValue FriendlyName"))?;

    let friendly_result = PropVariantToStringAlloc(&value)
      .map_err(win_to_app_err("PropVariantToStringAlloc"))
      .and_then(|pwstr| {
        let ptr = pwstr.0;
        let text = match pwstr
          .to_string()
          .map_err(|err| AppError::Runtime(format!("Не удалось преобразовать имя устройства: {err}")))
        {
          Ok(s) => s,
          Err(err) => {
            CoTaskMemFree(Some(ptr as *const c_void));
            return Err(err);
          }
        };
        CoTaskMemFree(Some(ptr as *const c_void));
        Ok(text)
      });

    PropVariantClear(&mut value).map_err(win_to_app_err("PropVariantClear FriendlyName"))?;

    match friendly_result {
      Ok(text) => {
        let trimmed = text.trim();
        if trimmed.is_empty() {
          Ok(None)
        } else {
          Ok(Some(trimmed.to_string()))
        }
      }
      Err(err) => Err(err),
    }
  }

  unsafe fn get_device_id(device: &IMMDevice) -> Result<String, AppError> {
    let raw = device.GetId().map_err(win_to_app_err("GetId"))?;
    let id = raw
      .to_string()
      .map_err(|err| AppError::Runtime(format!("Некорректный ID устройства: {err}")))?;
    CoTaskMemFree(Some(raw.0 as *const c_void));
    Ok(id)
  }

  fn win_to_app_err(context: &'static str) -> impl Fn(windows::core::Error) -> AppError {
    move |err| AppError::Runtime(format!("{context} failed: {err}"))
  }
}

#[cfg(target_os = "windows")]
pub use platform::AudioMirrorService;

#[cfg(not(target_os = "windows"))]
mod platform {
  use super::{AppError, AudioDevice, MirrorStatus};

  #[derive(Default)]
  pub struct AudioMirrorService;

  impl AudioMirrorService {
    pub fn new() -> Self {
      Self
    }

    pub fn list_devices(&self) -> Result<Vec<AudioDevice>, AppError> {
      Ok(Vec::new())
    }

    pub fn set_targets(&self, _: Vec<String>) -> Result<(), AppError> {
      Err(AppError::Runtime(
        "Аудио-зеркалирование доступно только в Windows-сборке".into(),
      ))
    }

    pub fn stop(&self) {}

    pub fn status(&self) -> MirrorStatus {
      MirrorStatus {
        enabled: false,
        targets: Vec::new(),
      }
    }
  }
}

#[cfg(not(target_os = "windows"))]
pub use platform::AudioMirrorService;


















