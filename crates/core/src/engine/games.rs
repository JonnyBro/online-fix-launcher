//! Adding, editing, launching and stopping games.

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rust_i18n::t;

use super::*;
use crate::launch::{self, Stream};
use crate::library::Game;
use crate::proton::ProtonBuild;
use crate::{fix, proton, steam};

/// Signing in can take a while on a slow connection.
const STEAM_TIMEOUT: Duration = Duration::from_secs(420);
/// A game that ends this fast with an error most likely didn't start.
const QUICK_EXIT: Duration = Duration::from_secs(15);

pub(super) struct Running {
	proton: ProtonBuild,
	prefix: PathBuf,
}

/// What a launch task reports back to the command loop.
pub(super) enum Finished {
	Ran {
		id: String,
		ran_for: Duration,
		code: Option<i32>,
	},
	Failed {
		id: String,
		error: String,
	},
}

impl State {
	pub(super) fn emit_library(&self) {
		self.sink.emit(Event::Library(self.library.games.clone()));
	}

	pub(super) fn save_library(&mut self) -> bool {
		if self.library_broken {
			self.sink
				.emit(Event::Error(t!("err.library_read_only").into_owned()));
			return false;
		}
		match self.library.save(&self.paths) {
			Ok(()) => true,
			Err(e) => {
				self.sink.emit(Event::Error(e.to_string()));
				false
			}
		}
	}

	pub(super) fn protons(&self) -> Vec<ProtonBuild> {
		let root = steam::root();
		let libraries = root
			.as_deref()
			.map(steam::library_folders)
			.unwrap_or_default();
		proton::find_installed(&self.paths.protons_dir(), root.as_deref(), &libraries)
	}

	pub(super) fn refresh_protons(&self) {
		let builds = self.protons();
		let libraries = steam::root()
			.as_deref()
			.map(steam::library_folders)
			.unwrap_or_default();
		let mut missing: Vec<u32> = builds
			.iter()
			.filter_map(|b| b.runtime_app_id)
			.filter(|id| steam::runtime(&libraries, *id).is_none())
			.collect();
		missing.sort_unstable();
		missing.dedup();
		self.sink.emit(Event::Protons(builds));
		self.sink.emit(Event::MissingRuntimes(missing));
	}

	pub(super) fn update_game(&mut self, game: Game) {
		let old = self.library.get(&game.id).cloned();
		self.library.upsert(game);
		if !self.save_library()
			&& let Some(old) = old
		{
			self.library.upsert(old);
		}
		self.emit_library();
	}

	pub(super) fn remove_game(&mut self, id: &str, delete_game_dir: bool, delete_prefix: bool) {
		if self.running.contains_key(id) {
			self.sink
				.emit(Event::Error(t!("err.game_running").into_owned()));
			return;
		}
		let Some(game) = self.library.remove(id) else {
			return;
		};
		if !self.save_library() {
			self.library.upsert(game);
			return;
		}
		let _ = std::fs::remove_file(self.paths.game_log(id));
		self.art.remove_all(id);
		self.emit_library();

		let mut doomed = Vec::new();
		if delete_prefix {
			doomed.push(game.prefix_dir(&self.paths));
		}
		if delete_game_dir {
			doomed.push(fix::game_root(&game.exe));
		}
		let protected = protected_dirs(&self.paths);
		let sink = self.sink.clone();
		let name = game.name;
		tokio::task::spawn_blocking(move || {
			let mut failed = false;
			for dir in doomed {
				if !deletable(&dir, &protected) {
					tracing::warn!("refusing to delete {}", dir.display());
					sink.emit(Event::Error(
						t!("err.not_deleting", path = dir.display()).into_owned(),
					));
					failed = true;
					continue;
				}
				tracing::info!("deleting {}", dir.display());
				if let Err(e) = std::fs::remove_dir_all(&dir)
					&& e.kind() != std::io::ErrorKind::NotFound
				{
					sink.emit(Event::Error(
						t!("err.delete_failed", path = dir.display(), error = e).into_owned(),
					));
					failed = true;
				}
			}
			if !failed {
				sink.emit(Event::Notice(t!("game.removed", name = name).into_owned()));
			}
		});
	}

	pub(super) fn launch(&mut self, id: &str) {
		if self.running.contains_key(id) {
			return;
		}
		let Some(game) = self.library.get(id).cloned() else {
			return;
		};
		let fail = |error: String| {
			self.sink.emit(Event::Error(
				t!("err.launch", name = game.name, error = error).into_owned(),
			));
		};
		let root = steam::root();
		let libraries = root
			.as_deref()
			.map(steam::library_folders)
			.unwrap_or_default();
		let builds = proton::find_installed(&self.paths.protons_dir(), root.as_deref(), &libraries);
		let Some(proton) = proton::choose(
			&builds,
			game.proton.as_deref(),
			self.settings.default_proton.as_deref(),
		)
		.cloned() else {
			fail(launch::LaunchError::ProtonMissing.to_string());
			return;
		};
		let runtime = match (game.steam_runtime, proton.runtime_app_id) {
			(true, Some(app_id)) => {
				let run = steam::runtime(&libraries, app_id);
				if run.is_none() {
					self.sink.emit(Event::Error(
						t!("err.runtime_missing", app_id = app_id).into_owned(),
					));
				}
				run
			}
			_ => None,
		};
		let prefix = game.prefix_dir(&self.paths);
		let ctx = launch::Context {
			proton: &proton,
			prefix: prefix.clone(),
			steam_root: root.clone(),
			runtime,
			wine_debug: None,
			ld_preload: std::env::var("LD_PRELOAD").ok(),
		};
		let plan = match launch::plan(&game, &ctx) {
			Ok(plan) => plan,
			Err(e) => {
				fail(e.to_string());
				return;
			}
		};
		tracing::info!("launching {}: {}", game.name, plan.display());

		let fix_root = fix::game_root(&game.exe);
		let fix_info = fix::scan(&fix_root);
		for change in crate::patch::patch_ini(&fix_info) {
			tracing::info!("{change}");
		}
		let cache = self.paths.cache_dir().to_path_buf();
		let wait_for_steam = self.settings.start_steam && root.is_some() && !steam::is_running();
		let sink = self.sink.clone();
		let done = self.finished.clone();
		let log_path = self.paths.game_log(id);
		let id = id.to_owned();
		let wineserver = proton.wine_bin("wineserver");
		let wine_prefix = prefix.join("pfx");
		self.running.insert(id.clone(), Running { proton, prefix });
		tokio::spawn(async move {
			match crate::patch::patch_photon(&fix_info, &fix_root, &cache).await {
				Ok(changes) => changes.iter().for_each(|c| tracing::info!("{c}")),
				Err(e) => {
					let _ = done.send(Finished::Failed {
						id,
						error: t!("err.patch", error = e).into_owned(),
					});
					return;
				}
			}
			if wait_for_steam {
				sink.emit(Event::Run {
					id: id.clone(),
					state: RunState::WaitingForSteam,
				});
				let root = root.unwrap_or_default();
				if !steam::start_and_wait(&root, STEAM_TIMEOUT).await {
					let _ = done.send(Finished::Failed {
						id,
						error: t!("err.steam_not_started").into_owned(),
					});
					return;
				}
			}
			sink.emit(Event::Run {
				id: id.clone(),
				state: RunState::Running,
			});
			let log = open_log(&log_path, &plan.display());
			let result = launch::run(&plan, move |stream, line| {
				if let Some(log) = &log
					&& let Ok(mut file) = log.lock()
				{
					let tag = match stream {
						Stream::Stdout => "out",
						Stream::Stderr => "err",
					};
					let _ = writeln!(file, "[{tag}] {line}");
				}
			})
			.await;
			// Launchers start the game and quit; the game is done when
			// every Wine process of the prefix is.
			wait_for_wine(&wineserver, &wine_prefix).await;
			let _ = done.send(match result {
				Ok(outcome) => Finished::Ran {
					id,
					ran_for: outcome.ran_for,
					code: outcome.code,
				},
				Err(e) => Finished::Failed {
					id,
					error: e.to_string(),
				},
			});
		});
	}

	pub(super) fn run_tool(&self, id: &str, tool: crate::tools::WineTool) {
		let Some(game) = self.library.get(id).cloned() else {
			return;
		};
		let root = steam::root();
		let libraries = root
			.as_deref()
			.map(steam::library_folders)
			.unwrap_or_default();
		let builds = proton::find_installed(&self.paths.protons_dir(), root.as_deref(), &libraries);
		let Some(proton) = proton::choose(
			&builds,
			game.proton.as_deref(),
			self.settings.default_proton.as_deref(),
		)
		.cloned() else {
			self.sink
				.emit(Event::Error(launch::LaunchError::ProtonMissing.to_string()));
			return;
		};
		let prefix = game.prefix_dir(&self.paths);
		let steam_root = root.unwrap_or_else(|| {
			std::env::var_os("HOME")
				.map(PathBuf::from)
				.unwrap_or_default()
				.join(".steam/steam")
		});
		let data_dir = self.paths.data_dir().to_path_buf();
		let sink = self.sink.clone();
		tokio::spawn(async move {
			let target = crate::tools::Target {
				proton: &proton,
				prefix: &prefix,
				steam_root,
			};
			if let Err(e) = crate::tools::run(tool, &target, &data_dir).await {
				sink.emit(Event::Error(t!("err.tool", error = e).into_owned()));
			}
		});
	}

	pub(super) fn stop(&mut self, id: &str) {
		let Some(running) = self.running.get(id) else {
			return;
		};
		self.sink.emit(Event::Run {
			id: id.to_owned(),
			state: RunState::Stopping,
		});
		let proton = running.proton.clone();
		let prefix = running.prefix.clone();
		let sink = self.sink.clone();
		tokio::spawn(async move {
			if let Err(e) = launch::stop(&proton, &prefix).await {
				sink.emit(Event::Error(e.to_string()));
			}
		});
	}

	pub(super) fn finished(&mut self, finished: Finished) {
		let (id, outcome) = match finished {
			Finished::Ran { id, ran_for, code } => (id, Ok((ran_for, code))),
			Finished::Failed { id, error } => (id, Err(error)),
		};
		self.running.remove(&id);
		self.sink.emit(Event::Run {
			id: id.clone(),
			state: RunState::Stopped,
		});
		let Some(mut game) = self.library.get(&id).cloned() else {
			return;
		};
		match outcome {
			Ok((ran_for, code)) => {
				tracing::info!("{} exited with {code:?} after {ran_for:?}", game.name);
				game.play_seconds += ran_for.as_secs();
				game.last_played = Some(crate::library::now());
				self.update_game(game.clone());
				if code != Some(0) && ran_for < QUICK_EXIT {
					let code = code.map_or_else(|| "?".to_owned(), |c| c.to_string());
					self.sink.emit(Event::Error(
						t!("err.game_quit", name = game.name, code = code).into_owned(),
					));
				}
			}
			Err(error) => self.sink.emit(Event::Error(
				t!("err.launch", name = game.name, error = error).into_owned(),
			)),
		}
	}
}

/// Folders a removal must never take, nor anything holding them.
fn protected_dirs(paths: &AppPaths) -> Vec<PathBuf> {
	let mut dirs = vec![
		paths
			.games_file()
			.parent()
			.map(Path::to_path_buf)
			.unwrap_or_default(),
		paths.default_prefixes_dir(),
		paths.protons_dir(),
	];
	if let Some(home) = std::env::var_os("HOME") {
		dirs.push(PathBuf::from(home));
	}
	if let Some(root) = steam::root() {
		dirs.push(root);
	}
	dirs.retain(|d| !d.as_os_str().is_empty());
	dirs
}

/// A guard against a broken path wiping something big: the folder must be
/// absolute, at least three levels deep, and not hold a protected folder.
pub(super) fn deletable(dir: &Path, protected: &[PathBuf]) -> bool {
	use std::path::Component;
	let deep = dir
		.components()
		.filter(|c| matches!(c, Component::Normal(_)))
		.count()
		>= 3;
	let tricky = dir
		.components()
		.any(|c| matches!(c, Component::ParentDir | Component::CurDir));
	dir.is_absolute() && deep && !tricky && !protected.iter().any(|p| p.starts_with(dir))
}

fn open_log(path: &Path, header: &str) -> Option<Mutex<std::fs::File>> {
	let open = || -> std::io::Result<std::fs::File> {
		if let Some(dir) = path.parent() {
			std::fs::create_dir_all(dir)?;
		}
		let mut file = std::fs::File::create(path)?;
		writeln!(file, "{header}\n")?;
		Ok(file)
	};
	match open() {
		Ok(file) => Some(Mutex::new(file)),
		Err(e) => {
			tracing::warn!("game log {}: {e}", path.display());
			None
		}
	}
}

async fn wait_for_wine(wineserver: &Path, prefix: &Path) {
	let status = tokio::process::Command::new(wineserver)
		.arg("-w")
		.env("WINEPREFIX", prefix)
		.stdin(std::process::Stdio::null())
		.stdout(std::process::Stdio::null())
		.stderr(std::process::Stdio::null())
		.status()
		.await;
	if let Err(e) = status {
		tracing::debug!("wineserver -w: {e}");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn guards_deletion() {
		let protected = [
			PathBuf::from("/home/u"),
			PathBuf::from("/home/u/.local/share/onlinefix-launcher/prefixes"),
		];
		assert!(deletable(Path::new("/mnt/hdd/Bodycam"), &protected));
		assert!(deletable(
			Path::new("/home/u/.local/share/onlinefix-launcher/prefixes/abc"),
			&protected
		));
		assert!(deletable(Path::new("/home/u/Games/X"), &protected));
		for bad in [
			"/",
			"/mnt/hdd",
			"/home/u",
			"/home",
			"/home/u/.local/share/onlinefix-launcher",
			"relative/path/here",
			"/mnt/hdd/Game/../..",
		] {
			assert!(!deletable(Path::new(bad), &protected), "{bad}");
		}
	}
}
