use std::{io, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
  #[error("Could not locate the `config-files` directory. Checked: {0}")]
  ConfigDirectoryMissing(String),
  #[error("{0}")]
  InvalidConfig(String),
  #[error("{0}")]
  Runtime(String),
  #[error("I/O error: {0}")]
  Io(#[from] io::Error),
  #[error("Failed to parse data: {0}")]
  Serde(#[from] serde_json::Error),
}

impl AppError {
  pub fn wrap_paths<T: IntoIterator<Item = PathBuf>>(paths: T) -> String {
    let joined = paths
      .into_iter()
      .map(|p| p.to_string_lossy().to_string())
      .collect::<Vec<_>>()
      .join(", ");
    joined
  }
}

impl From<AppError> for String {
  fn from(err: AppError) -> Self {
    err.to_string()
  }
}
