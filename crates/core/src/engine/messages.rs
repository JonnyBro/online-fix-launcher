use std::path::PathBuf;

use crate::app_update::AppRelease;
use crate::library::Game;
use crate::proton::ProtonBuild;
use crate::settings::Settings;

#[derive(Debug)]
pub enum Command {
	SaveSettings(Settings),
	CheckAppUpdate,
	/// Adds the game this exe starts, with what its fix needs.
	AddGame {
		exe: PathBuf,
	},
	UpdateGame(Box<Game>),
	/// Takes the game off the list, and its folders off the disk if asked.
	RemoveGame {
		id: String,
		delete_game_dir: bool,
		delete_prefix: bool,
	},
	Launch(String),
	Stop(String),
	RefreshProtons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
	WaitingForSteam,
	Running,
	/// Asked to stop; Wine is closing it.
	Stopping,
	Stopped,
}

#[derive(Debug, Clone)]
pub enum Event {
	SettingsSaved(Settings),
	SettingsSaveFailed(String),
	AppUpdate(AppRelease),
	/// Every game, after any change.
	Library(Vec<Game>),
	GameAdded(String),
	Protons(Vec<ProtonBuild>),
	Run {
		id: String,
		state: RunState,
	},
	/// A short success message for the status area.
	Notice(String),
	Error(String),
}
