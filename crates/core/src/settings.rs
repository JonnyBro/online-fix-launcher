use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::paths::AppPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
	#[default]
	System,
	Dark,
	Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
	#[default]
	System,
	English,
	Russian,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
	pub theme: Theme,
	/// Color scheme names the UI shows for the dark and light theme; empty
	/// means its default one. The core doesn't know the schemes.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub dark_scheme: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	pub light_scheme: String,
	pub language: Language,
	/// Desktop notifications for what happens while the window isn't focused.
	pub notifications: bool,
	/// Look for a newer release of the app on GitHub at start.
	pub check_app_updates: bool,
	/// A release the user chose not to be asked about again.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub skipped_version: String,
	/// The Proton build for games that don't pick one; `None` means the newest.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub default_proton: Option<PathBuf>,
	/// Start Steam before a game when it isn't running; fixes talk to it.
	pub start_steam: bool,
	/// Where archives unpack; empty means `~/Games`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub games_dir: Option<PathBuf>,
	/// For pictures Steam lacks and for the gallery; empty turns SteamGridDB off.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub steamgriddb_key: String,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			theme: Theme::default(),
			dark_scheme: String::new(),
			light_scheme: String::new(),
			language: Language::default(),
			notifications: true,
			check_app_updates: true,
			skipped_version: String::new(),
			default_proton: None,
			start_steam: true,
			games_dir: None,
			steamgriddb_key: String::new(),
		}
	}
}

impl Settings {
	pub fn games_dir(&self) -> PathBuf {
		self.games_dir.clone().unwrap_or_else(|| {
			std::env::var_os("HOME")
				.map(PathBuf::from)
				.unwrap_or_default()
				.join("Games")
		})
	}

	/// A missing file yields defaults; a broken one is an error so the user's
	/// settings are never silently replaced.
	pub fn load(paths: &AppPaths) -> Result<Self> {
		let path = paths.settings_file();
		let text = match std::fs::read_to_string(&path) {
			Ok(text) => text,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
			Err(e) => return Err(e).at(path),
		};
		toml::from_str(&text).map_err(|source| Error::SettingsParse { path, source })
	}

	pub fn save(&self, paths: &AppPaths) -> Result<()> {
		write_atomically(
			&paths.settings_file(),
			toml::to_string_pretty(self)?.as_bytes(),
		)
	}
}

pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).at(parent)?;
	}
	let tmp = path.with_extension("tmp");
	// The SteamGridDB key is in here.
	write_private(&tmp, bytes).at(&tmp)?;
	std::fs::rename(&tmp, path).at(path)
}

/// Writes a file only this user can read, for credentials.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	use std::os::unix::fs::OpenOptionsExt;
	let mut file = std::fs::OpenOptions::new()
		.write(true)
		.create(true)
		.truncate(true)
		.mode(0o600)
		.open(path)?;
	std::io::Write::write_all(&mut file, bytes)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn missing_file_gives_defaults() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		assert_eq!(Settings::load(&paths).unwrap(), Settings::default());
	}

	#[test]
	fn roundtrip_is_private() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		let settings = Settings {
			theme: Theme::Dark,
			dark_scheme: "Nord".into(),
			light_scheme: String::new(),
			language: Language::Russian,
			notifications: false,
			check_app_updates: false,
			skipped_version: "0.2.0".into(),
			default_proton: Some("/p/GE-Proton10-1".into()),
			start_steam: false,
			games_dir: Some("/mnt/games".into()),
			steamgriddb_key: "secret".into(),
		};
		settings.save(&paths).unwrap();
		assert_eq!(Settings::load(&paths).unwrap(), settings);
		use std::os::unix::fs::PermissionsExt;
		let mode = std::fs::metadata(paths.settings_file())
			.unwrap()
			.permissions()
			.mode();
		assert_eq!(mode & 0o777, 0o600);
	}

	#[test]
	fn partial_file_fills_defaults() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		std::fs::create_dir_all(paths.settings_file().parent().unwrap()).unwrap();
		std::fs::write(paths.settings_file(), "language = \"russian\"\n").unwrap();
		let loaded = Settings::load(&paths).unwrap();
		assert_eq!(loaded.language, Language::Russian);
		assert!(loaded.check_app_updates);
	}

	#[test]
	fn broken_file_is_an_error() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		std::fs::create_dir_all(paths.settings_file().parent().unwrap()).unwrap();
		std::fs::write(paths.settings_file(), "theme = [").unwrap();
		assert!(matches!(
			Settings::load(&paths),
			Err(Error::SettingsParse { .. })
		));
	}
}
