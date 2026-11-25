//! The games the user added, kept in `games.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::paths::AppPaths;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Game {
	pub id: String,
	pub name: String,
	pub exe: PathBuf,
	/// `None` means `<prefixes>/<id>`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub prefix: Option<PathBuf>,
	/// The Proton build folder; `None` means the default one.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub proton: Option<PathBuf>,
	/// DLL overrides found in the fix, like `winmm=n,b;steam_api64=n`.
	pub fix_overrides: String,
	/// Added by the user after the fix's own.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub extra_overrides: String,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	pub env: BTreeMap<String, String>,
	/// Shell words put before the whole command, like `gamemoderun`.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub args_before: String,
	/// Shell words passed to the game.
	#[serde(skip_serializing_if = "String::is_empty")]
	pub args_after: String,
	pub wined3d: bool,
	pub wayland: bool,
	pub steam_overlay: bool,
	pub steam_runtime: bool,
	/// The overlay, as a Vulkan layer (`MANGOHUD=1`).
	#[serde(skip_serializing_if = "is_false")]
	pub mangohud: bool,
	/// Run through `gamemoderun`.
	#[serde(skip_serializing_if = "is_false")]
	pub gamemode: bool,
	/// DLSS and Reflex on NVIDIA cards (`PROTON_ENABLE_NVAPI=1`).
	#[serde(skip_serializing_if = "is_false")]
	pub nvapi: bool,
	/// AMD FSR 1 upscaling: gamescope's when it's on, Proton's otherwise.
	#[serde(skip_serializing_if = "is_false")]
	pub fsr: bool,
	#[serde(skip_serializing_if = "Gamescope::is_default")]
	pub gamescope: Gamescope,
	/// The game's own Steam app id, for covers.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub real_app_id: Option<u32>,
	/// The id the fix pretends to be (often 480, Spacewar).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub fake_app_id: Option<u32>,
	pub play_seconds: u64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub last_played: Option<u64>,
	pub added: u64,
	#[serde(skip_serializing_if = "is_false")]
	pub favorite: bool,
	/// Run with Wine's debug channels on, so the log says more.
	#[serde(skip_serializing_if = "is_false")]
	pub debug: bool,
	/// The channels for `debug`; empty means [`DEBUG_CHANNELS`].
	#[serde(skip_serializing_if = "String::is_empty")]
	pub debug_channels: String,
	/// Left out of the library unless hidden games are shown.
	#[serde(skip_serializing_if = "is_false")]
	pub hidden: bool,
}

/// Sizes kept while it's off, so turning it back on needs no retyping.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gamescope {
	pub enabled: bool,
	/// The size of gamescope's window or screen, `-W`/`-H`; 0 leaves it to gamescope.
	pub width: u32,
	pub height: u32,
	/// The size the game renders at, `-w`/`-h`; smaller than the output upscales.
	pub game_width: u32,
	pub game_height: u32,
	/// `-r`; 0 leaves it to gamescope.
	pub refresh: u32,
	pub fullscreen: bool,
}

impl Gamescope {
	fn is_default(&self) -> bool {
		*self == Self::default()
	}

	/// `gamescope` and its options, up to the `--` before the game.
	pub fn command(&self, fsr: bool) -> Vec<String> {
		let mut args = vec!["gamescope".to_owned()];
		let mut pair = |flag: &str, value: u32| {
			if value > 0 {
				args.push(flag.to_owned());
				args.push(value.to_string());
			}
		};
		pair("-W", self.width);
		pair("-H", self.height);
		pair("-w", self.game_width);
		pair("-h", self.game_height);
		pair("-r", self.refresh);
		if self.fullscreen {
			args.push("-f".into());
		}
		if fsr {
			args.extend(["-F".into(), "fsr".into()]);
		}
		args.push("--".into());
		args
	}
}

/// What Proton itself turns on with `PROTON_LOG=1`.
pub const DEBUG_CHANNELS: &str = "+timestamp,+pid,+tid,+seh,+debugstr,+loaddll,+mscoree";

fn is_false(value: &bool) -> bool {
	!value
}

impl Default for Game {
	fn default() -> Self {
		Self {
			id: String::new(),
			name: String::new(),
			exe: PathBuf::new(),
			prefix: None,
			proton: None,
			fix_overrides: String::new(),
			extra_overrides: String::new(),
			env: BTreeMap::new(),
			args_before: String::new(),
			args_after: String::new(),
			wined3d: false,
			wayland: false,
			steam_overlay: false,
			steam_runtime: true,
			mangohud: false,
			gamemode: false,
			nvapi: false,
			fsr: false,
			gamescope: Gamescope::default(),
			real_app_id: None,
			fake_app_id: None,
			play_seconds: 0,
			last_played: None,
			added: 0,
			favorite: false,
			debug: false,
			debug_channels: String::new(),
			hidden: false,
		}
	}
}

impl Game {
	pub fn new(name: impl Into<String>, exe: impl Into<PathBuf>) -> Self {
		Self {
			id: new_id(),
			name: name.into(),
			exe: exe.into(),
			added: now(),
			..Self::default()
		}
	}

	pub fn prefix_dir(&self, paths: &AppPaths) -> PathBuf {
		self.prefix
			.clone()
			.unwrap_or_else(|| paths.default_prefixes_dir().join(&self.id))
	}

	/// `WINEDEBUG` for the next run.
	pub fn wine_debug(&self) -> &str {
		match self.debug_channels.trim() {
			_ if !self.debug => "-all",
			"" => DEBUG_CHANNELS,
			channels => channels,
		}
	}

	pub fn work_dir(&self) -> &Path {
		self.exe.parent().unwrap_or(Path::new("/"))
	}
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
	#[serde(default)]
	pub games: Vec<Game>,
}

impl Library {
	/// A missing file is an empty library; a broken one is an error so the
	/// list is never silently replaced.
	pub fn load(paths: &AppPaths) -> Result<Self> {
		let path = paths.games_file();
		let text = match std::fs::read_to_string(&path) {
			Ok(text) => text,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
			Err(e) => return Err(e).at(path),
		};
		toml::from_str(&text).map_err(|source| Error::LibraryParse { path, source })
	}

	pub fn save(&self, paths: &AppPaths) -> Result<()> {
		let path = paths.games_file();
		crate::settings::write_atomically(&path, toml::to_string_pretty(self)?.as_bytes())
	}

	pub fn get(&self, id: &str) -> Option<&Game> {
		self.games.iter().find(|g| g.id == id)
	}

	/// Adds the game, or replaces the one with its id.
	pub fn upsert(&mut self, game: Game) {
		match self.games.iter_mut().find(|g| g.id == game.id) {
			Some(slot) => *slot = game,
			None => self.games.push(game),
		}
	}

	pub fn remove(&mut self, id: &str) -> Option<Game> {
		let at = self.games.iter().position(|g| g.id == id)?;
		Some(self.games.remove(at))
	}
}

/// The environment as `KEY=value` lines, for editing as text.
pub fn env_to_text(env: &BTreeMap<String, String>) -> String {
	env.iter()
		.map(|(k, v)| format!("{k}={v}"))
		.collect::<Vec<_>>()
		.join("\n")
}

/// Reads `KEY=value` lines back; blank lines, `#` comments and lines
/// without a valid name are skipped.
pub fn env_from_text(text: &str) -> BTreeMap<String, String> {
	text.lines()
		.filter_map(|line| {
			let line = line.trim();
			if line.starts_with('#') {
				return None;
			}
			let (key, value) = line.split_once('=')?;
			let key = key.trim();
			let valid = !key.is_empty()
				&& !key.starts_with(|c: char| c.is_ascii_digit())
				&& key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
			valid.then(|| (key.to_owned(), value.trim().to_owned()))
		})
		.collect()
}

pub(crate) fn now() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |d| d.as_secs())
}

/// Unique enough for one user's library, and safe as a folder name.
fn new_id() -> String {
	use std::sync::atomic::{AtomicU32, Ordering};
	static COUNTER: AtomicU32 = AtomicU32::new(0);
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |d| d.as_nanos());
	let n = COUNTER.fetch_add(1, Ordering::Relaxed);
	format!("{:x}{:x}", nanos as u64, n ^ std::process::id())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roundtrip_and_edit() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		assert!(Library::load(&paths).unwrap().games.is_empty());

		let mut game = Game::new("Test Game", "/games/test/Game.exe");
		game.fix_overrides = "winmm=n,b;steam_api64=n".into();
		game.env.insert("DXVK_HUD".into(), "fps".into());
		game.real_app_id = Some(1_234_560);
		let mut other = Game::new("Other", "/games/other/Other.exe");
		other.steam_overlay = true;
		other.favorite = true;
		other.hidden = true;
		assert_ne!(game.id, other.id);

		let mut library = Library::default();
		library.upsert(game.clone());
		library.upsert(other.clone());
		library.save(&paths).unwrap();
		let loaded = Library::load(&paths).unwrap();
		assert_eq!(loaded, library);

		game.play_seconds = 60;
		library.upsert(game.clone());
		assert_eq!(library.games.len(), 2);
		assert_eq!(library.get(&game.id).unwrap().play_seconds, 60);
		assert_eq!(library.remove(&other.id).unwrap().name, "Other");
		assert_eq!(
			game.prefix_dir(&paths),
			paths.default_prefixes_dir().join(&game.id)
		);
		assert_eq!(game.work_dir(), Path::new("/games/test"));
	}

	#[test]
	fn env_text_roundtrip() {
		let env = env_from_text("DXVK_HUD = fps\n# note\n\nbad line\n1X=2\nPROTON_LOG=1\nA-B=c\n");
		assert_eq!(env.len(), 2);
		assert_eq!(env["DXVK_HUD"], "fps");
		assert_eq!(env_to_text(&env), "DXVK_HUD=fps\nPROTON_LOG=1");
		assert_eq!(env_from_text(&env_to_text(&env)), env);
	}

	#[test]
	fn broken_file_is_an_error() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		std::fs::create_dir_all(paths.games_file().parent().unwrap()).unwrap();
		std::fs::write(paths.games_file(), "[[games]\n").unwrap();
		assert!(matches!(
			Library::load(&paths),
			Err(Error::LibraryParse { .. })
		));
	}
}
