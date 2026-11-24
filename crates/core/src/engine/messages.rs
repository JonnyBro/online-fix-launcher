use std::path::PathBuf;

use crate::app_update::AppRelease;
use crate::art::{ArtKind, Choice};
use crate::fix::FixInfo;
use crate::library::Game;
use crate::proton::ProtonBuild;
use crate::proton_releases::{Provider, Release};
use crate::settings::Settings;
use crate::tools::WineTool;

#[derive(Debug)]
pub enum Command {
	SaveSettings(Settings),
	CheckAppUpdate,
	/// Looks into a folder, archive or exe the user wants to add.
	InspectSource {
		request: u64,
		path: PathBuf,
		password: Option<String>,
	},
	AddGame(NewGame),
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
	ListProtonReleases(Provider),
	InstallProton(Release),
	/// Only builds in the launcher's own folder.
	RemoveProton(PathBuf),
	/// Looks for pictures again, replacing the grid and hero.
	FetchArt(String),
	ArtChoices {
		request: u64,
		id: String,
		kind: ArtKind,
	},
	SetArt {
		id: String,
		kind: ArtKind,
		from: ArtSource,
	},
	CheckSteamGridDbKey(String),
	/// `show` opens the results; otherwise only problems are announced.
	Checkup {
		show: bool,
	},
	RunTool {
		id: String,
		tool: WineTool,
	},
}

#[derive(Debug, Clone)]
pub enum ArtSource {
	Url(String),
	File(PathBuf),
}

/// A SteamGridDB picture with its preview downloaded.
#[derive(Debug, Clone)]
pub struct ArtOption {
	pub choice: Choice,
	pub thumb: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
	Exe,
	Folder,
	Archive,
}

/// What a picked folder, archive or exe holds.
#[derive(Debug, Clone)]
pub struct Source {
	pub kind: SourceKind,
	/// For an archive, its first volume.
	pub path: PathBuf,
	/// The password the archive opened with.
	pub password: Option<String>,
	/// Programs that may start the game: absolute paths, or paths inside the archive.
	pub candidates: Vec<PathBuf>,
	/// The one that surely starts it.
	pub pick: Option<PathBuf>,
	pub name: String,
	/// The archive's only top folder, which unpacks as the game folder.
	pub top_dir: Option<PathBuf>,
	/// Candidates that are FreeTP installers (a `.ftp` file beside them).
	pub installers: Vec<PathBuf>,
	/// Not known for archives until they're unpacked.
	pub fix: Option<FixInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InspectError {
	NeedPassword,
	Failed(String),
}

#[derive(Debug, Clone)]
pub struct NewGame {
	/// Set when an installer already made the prefix for this id.
	pub id: Option<String>,
	pub name: String,
	/// Absolute, or inside the archive when unpacking.
	pub exe: PathBuf,
	/// `None` for the launcher's own folder.
	pub prefix: Option<PathBuf>,
	pub unpack: Option<Unpack>,
	/// `exe` is a FreeTP installer that puts the game here; the add dialog
	/// opens again on this folder afterwards.
	pub install_into: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PendingStage {
	Unpacking(f32),
	Installing,
}

#[derive(Debug, Clone)]
pub struct Unpack {
	pub archive: PathBuf,
	pub password: String,
	/// Where the archive's contents go.
	pub into: PathBuf,
	/// The game's folder after unpacking; it must not exist or be empty.
	pub game_dir: PathBuf,
	pub delete_archive: bool,
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
	/// Steam Linux Runtime app ids some installed build needs but Steam lacks.
	MissingRuntimes(Vec<u32>),
	ProtonReleases {
		provider: Provider,
		result: Result<Vec<Release>, String>,
	},
	/// `None` once the download is done and it unpacks.
	ProtonInstalling {
		tag: String,
		progress: Option<f32>,
	},
	ProtonInstallEnded(String),
	Run {
		id: String,
		state: RunState,
	},
	SourceInspected {
		request: u64,
		result: Result<Box<Source>, InspectError>,
	},
	/// A game that isn't in the list yet, being unpacked or installed.
	Pending {
		id: String,
		name: String,
		stage: PendingStage,
	},
	PendingEnded(String),
	/// A FreeTP installer finished; the game is to be added from `folder`.
	Installed {
		id: String,
		name: String,
		folder: PathBuf,
		prefix: Option<PathBuf>,
	},
	ArtChanged(String),
	ArtChoices {
		request: u64,
		result: Result<Vec<ArtOption>, String>,
	},
	SteamGridDbKey(Result<(), String>),
	Checkup {
		checks: Vec<crate::checkup::Check>,
		show: bool,
	},
	/// A short success message, shown as a toast.
	Notice(String),
	Error(String),
}
