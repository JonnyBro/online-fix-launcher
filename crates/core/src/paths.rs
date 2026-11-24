use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{Error, IoContext, Result};

pub const APP_ID: &str = "onlinefix-launcher";

#[derive(Debug, Clone)]
pub struct AppPaths {
	config_dir: PathBuf,
	data_dir: PathBuf,
	cache_dir: PathBuf,
}

impl AppPaths {
	/// `~/.config`, `~/.local/share` and `~/.cache`, or their XDG overrides.
	pub fn detect() -> Result<Self> {
		let dirs = ProjectDirs::from("", "", APP_ID).ok_or(Error::NoPlatformDirs)?;
		Ok(Self {
			config_dir: dirs.config_dir().to_path_buf(),
			data_dir: dirs.data_dir().to_path_buf(),
			cache_dir: dirs.cache_dir().to_path_buf(),
		})
	}

	/// Puts everything under one root; used by tests.
	pub fn under(root: impl AsRef<Path>) -> Self {
		let root = root.as_ref();
		Self {
			config_dir: root.join("config"),
			data_dir: root.join("data"),
			cache_dir: root.join("cache"),
		}
	}

	pub fn ensure_exist(&self) -> Result<()> {
		for dir in [&self.config_dir, &self.cache_dir, &self.logs_dir()] {
			std::fs::create_dir_all(dir).at(dir)?;
		}
		Ok(())
	}

	pub fn settings_file(&self) -> PathBuf {
		self.config_dir.join("settings.toml")
	}

	pub fn games_file(&self) -> PathBuf {
		self.config_dir.join("games.toml")
	}

	/// Window size and view choices kept by the GUI between runs.
	pub fn ui_state_file(&self) -> PathBuf {
		self.config_dir.join("ui.json")
	}

	pub fn logs_dir(&self) -> PathBuf {
		self.data_dir.join("logs")
	}

	/// Wine prefixes of games that don't set their own.
	pub fn default_prefixes_dir(&self) -> PathBuf {
		self.data_dir.join("prefixes")
	}

	/// GE-Proton builds the app downloaded.
	pub fn protons_dir(&self) -> PathBuf {
		self.data_dir.join("protons")
	}

	pub fn cache_dir(&self) -> &Path {
		&self.cache_dir
	}
}
