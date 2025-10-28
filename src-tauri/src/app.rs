use crate::error::AppError;
use parking_lot::Mutex;
use pathdiff::diff_paths;
use serde::{Deserialize, Serialize};
use std::{
  collections::HashSet,
  fs,
  io::Write,
  path::PathBuf,
};

const CONFIG_DIR_NAME: &str = "config-files";
const DATA_FILE_NAME: &str = "data.json";
const MAIN_CONFIG_NAME: &str = "config.txt";

#[derive(Debug, Clone)]
pub struct AppPaths {
  pub base_dir: PathBuf,
  pub config_dir: PathBuf,
  pub data_file: PathBuf,
  pub main_config: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct PersistedState {
  allow_multiple: bool,
  active_configs: Vec<String>,
  profiles_dir: Option<String>,
}

impl Default for PersistedState {
  fn default() -> Self {
    Self {
      allow_multiple: true,
      active_configs: Vec::new(),
      profiles_dir: None,
    }
  }
}

#[derive(Debug)]
pub struct AppState {
  inner: Mutex<PersistedState>,
  pub paths: AppPaths,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatus {
  pub file_name: String,
  pub display_name: String,
  pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiStatus {
  pub allow_multiple: bool,
  pub configs: Vec<ConfigStatus>,
}

impl AppPaths {
  pub fn resolve() -> Result<Self, AppError> {
    // mesh-audio не требует наличия папки config-files.
    // Берём текущую директорию (или каталог исполняемого файла) и формируем пути без проверок.
    let base = std::env::current_dir().or_else(|_| std::env::current_exe().and_then(|mut p| { p.pop(); Ok(p) }))?;
    let config_dir = base.join(CONFIG_DIR_NAME);
    let main_config = base.join(MAIN_CONFIG_NAME);
    let data_file = base.join(DATA_FILE_NAME);
    Ok(AppPaths { base_dir: base, config_dir, data_file, main_config })
  }
}

impl AppState {
  pub fn initialize() -> Result<Self, AppError> {
    let paths = AppPaths::resolve()?;
    fs::create_dir_all(&paths.config_dir)?;

    let persisted = if paths.data_file.exists() {
      let raw = fs::read_to_string(&paths.data_file)?;
      if raw.trim().is_empty() {
        PersistedState::default()
      } else {
        serde_json::from_str(&raw)?
      }
    } else {
      PersistedState::default()
    };

    Ok(Self {
      inner: Mutex::new(persisted),
      paths,
    })
  }

  pub fn status(&self) -> Result<UiStatus, AppError> {
    let available = self.collect_config_files()?;
    let snapshot = {
      let data = self.inner.lock();
      data.clone()
    };
    Ok(Self::build_status(&available, &snapshot))
  }

  pub fn toggle_config(&self, file_name: &str) -> Result<UiStatus, AppError> {
    let available = self.collect_config_files()?;
    if !available.iter().any(|name| name == file_name) {
      return Err(AppError::InvalidConfig(format!(
        "Config `{}` was not found in {}`",
        file_name, CONFIG_DIR_NAME
      )));
    }

    let mut data = self.inner.lock();
    let mut changed = Self::prune_missing(&mut data, &available);

    if let Some(idx) = data.active_configs.iter().position(|name| name == file_name) {
      data.active_configs.remove(idx);
      changed = true;
    } else {
      if !data.allow_multiple {
        if !data.active_configs.is_empty() {
          data.active_configs.clear();
        }
      }
      data.active_configs.push(file_name.to_string());
      changed = true;
    }

    if changed {
      // ensure the file actually exists in the active directory before writing Include
      let active_dir = self
        .inner
        .lock()
        .profiles_dir
        .as_ref()
        .and_then(|p| {
          let pb = PathBuf::from(p);
          if pb.is_dir() { Some(pb) } else { None }
        })
        .unwrap_or_else(|| self.paths.config_dir.clone());
      for file in &data.active_configs {
        let candidate = active_dir.join(file);
        if !candidate.exists() {
          return Err(AppError::InvalidConfig(format!(
            "Selected profile does not exist: {}",
            candidate.display()
          )));
        }
      }
      Self::sort_configs(&mut data.active_configs);
      self.write_main_config(&data.active_configs)?;
      self.persist(&data)?;
    }

    let snapshot = data.clone();
    drop(data);
    Ok(Self::build_status(&available, &snapshot))
  }

  pub fn set_allow_multiple(&self, allow: bool) -> Result<UiStatus, AppError> {
    let mut data = self.inner.lock();
    if data.allow_multiple != allow {
      data.allow_multiple = allow;
      self.persist(&data)?;
    }
    let snapshot = data.clone();
    drop(data);
    let available = self.collect_config_files()?;
    Ok(Self::build_status(&available, &snapshot))
  }

  pub fn uncheck_all(&self) -> Result<UiStatus, AppError> {
    let mut data = self.inner.lock();
    if !data.active_configs.is_empty() {
      data.active_configs.clear();
      self.write_main_config(&data.active_configs)?;
      self.persist(&data)?;
    }
    let snapshot = data.clone();
    drop(data);
    let available = self.collect_config_files()?;
    Ok(Self::build_status(&available, &snapshot))
  }

  pub fn set_profiles_dir(&self, dir: String) -> Result<UiStatus, AppError> {
    let mut data = self.inner.lock();
    let normalized = dir.trim().to_string();
    if normalized.is_empty() {
      data.profiles_dir = None;
    } else {
      let pb = PathBuf::from(&normalized);
      if !pb.exists() || !pb.is_dir() {
        return Err(AppError::InvalidConfig(format!(
          "Selected folder is not a directory: {}",
          normalized
        )));
      }
      data.profiles_dir = Some(normalized);
    }
    self.persist(&data)?;
    let snapshot = data.clone();
    drop(data);
    let available = self.collect_config_files()?;
    Ok(Self::build_status(&available, &snapshot))
  }

  fn collect_config_files(&self) -> Result<Vec<String>, AppError> {
    let mut configs = Vec::new();
    let read_dir_path = self
      .inner
      .lock()
      .profiles_dir
      .as_ref()
      .and_then(|p| {
        let pb = PathBuf::from(p);
        if pb.is_dir() { Some(pb) } else { None }
      })
      .unwrap_or_else(|| self.paths.config_dir.clone());

    if !read_dir_path.exists() {
      return Ok(configs);
    }
    let it = match fs::read_dir(&read_dir_path) {
      Ok(it) => it,
      Err(_) => return Ok(configs),
    };
    for entry in it {
      let entry = entry?;
      let path = entry.path();
      if path.is_file() {
        if let Some(ext) = path.extension() {
          if ext.eq_ignore_ascii_case("txt") {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
              configs.push(file_name.to_string());
            }
          }
        }
      }
    }
    configs.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    Ok(configs)
  }

  fn build_status(configs: &[String], data: &PersistedState) -> UiStatus {
    let active_set: HashSet<&str> = data.active_configs.iter().map(|s| s.as_str()).collect();
    let config_statuses = configs
      .iter()
      .map(|file_name| ConfigStatus {
        display_name: file_name.trim_end_matches(".txt").to_string(),
        file_name: file_name.clone(),
        active: active_set.contains(file_name.as_str()),
      })
      .collect();

    UiStatus {
      allow_multiple: data.allow_multiple,
      configs: config_statuses,
    }
  }

  fn prune_missing(data: &mut PersistedState, available: &[String]) -> bool {
    let available: HashSet<&str> = available.iter().map(|s| s.as_str()).collect();
    let original_len = data.active_configs.len();
    data.active_configs.retain(|name| available.contains(name.as_str()));
    original_len != data.active_configs.len()
  }

  fn sort_configs(configs: &mut Vec<String>) {
    configs.sort();
    configs.dedup();
  }

  fn write_main_config(&self, configs: &[String]) -> Result<(), AppError> {
    let mut lines = Vec::new();
    for file in configs {
      let base_cfg_dir = self
        .inner
        .lock()
        .profiles_dir
        .as_ref()
        .and_then(|p| {
          let pb = PathBuf::from(p);
          if pb.is_dir() { Some(pb) } else { None }
        })
        .unwrap_or_else(|| self.paths.config_dir.clone());

      let absolute = base_cfg_dir.join(file);
      let base = self
        .paths
        .main_config
        .parent()
        .unwrap_or(&self.paths.base_dir);
      let relative = diff_paths(&absolute, base).unwrap_or_else(|| PathBuf::from(&absolute));
      lines.push(format!("Include: {}", relative.display()));
    }
    let content = lines.join("\n");
    let mut file = fs::File::create(&self.paths.main_config)?;
    file.write_all(content.as_bytes())?;
    Ok(())
  }

  fn persist(&self, data: &PersistedState) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(data)?;
    fs::write(&self.paths.data_file, json)?;
    Ok(())
  }
}
