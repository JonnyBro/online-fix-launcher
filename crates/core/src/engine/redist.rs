//! Installing a game's bundled runtimes into its prefix.

use std::collections::HashSet;
use std::path::Path;

use rust_i18n::t;

use super::*;
use crate::redist::Redist;
use crate::{proton, steam};

/// What an install task reports back to the command loop.
pub(super) struct RedistsDone {
	pub id: String,
	/// Keys of the ones that went fine.
	pub done: Vec<String>,
	/// File names of the ones that didn't, with why.
	pub failed: Vec<(String, String)>,
}

impl State {
	pub(super) fn list_redists(&self, id: &str) {
		let Some(game) = self.library.get(id) else {
			return;
		};
		let exe = game.exe.clone();
		let (sink, id) = (self.sink.clone(), id.to_owned());
		tokio::task::spawn_blocking(move || {
			let found = crate::redist::find(&exe);
			sink.emit(Event::Redists { id, found });
		});
	}

	pub(super) fn install_redists(&mut self, id: &str, paths: Vec<PathBuf>) {
		if self.running.contains_key(id) {
			self.sink
				.emit(Event::Error(t!("err.game_running").into_owned()));
			return;
		}
		if paths.is_empty() || !self.redists_busy.insert(id.to_owned()) {
			return;
		}
		let Some(game) = self.library.get(id).cloned() else {
			self.redists_busy.remove(id);
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
			self.redists_busy.remove(id);
			self.sink.emit(Event::Error(
				crate::launch::LaunchError::ProtonMissing.to_string(),
			));
			return;
		};
		// Only what's still there and was found for this game, so a stale
		// list can't run some other file.
		let found = crate::redist::find(&game.exe);
		let wanted: HashSet<&PathBuf> = paths.iter().collect();
		let redists: Vec<Redist> = found
			.into_iter()
			.filter(|r| wanted.contains(&r.path))
			.collect();
		let steam_root = root.unwrap_or_else(|| {
			std::env::var_os("HOME")
				.map(PathBuf::from)
				.unwrap_or_default()
				.join(".steam/steam")
		});
		let prefix = game.prefix_dir(&self.paths);
		let (sink, done_tx, id) = (self.sink.clone(), self.redists_done.clone(), id.to_owned());
		tokio::spawn(async move {
			let total = redists.len();
			let mut done = Vec::new();
			let mut failed = Vec::new();
			for (ix, redist) in redists.iter().enumerate() {
				sink.emit(Event::RedistInstalling {
					id: id.clone(),
					step: Some((ix, total)),
				});
				let name = redist
					.path
					.file_name()
					.map(|n| n.to_string_lossy().into_owned())
					.unwrap_or_default();
				match install_one(&proton, &prefix, &steam_root, redist).await {
					Ok(()) => done.push(redist.key.clone()),
					Err(e) => {
						tracing::warn!("{}: {e}", redist.path.display());
						failed.push((name, e));
					}
				}
			}
			let _ = done_tx.send(RedistsDone { id, done, failed });
		});
	}

	pub(super) fn redists_done(&mut self, result: RedistsDone) {
		let RedistsDone { id, done, failed } = result;
		self.redists_busy.remove(&id);
		self.sink.emit(Event::RedistInstalling {
			id: id.clone(),
			step: None,
		});
		let Some(mut game) = self.library.get(&id).cloned() else {
			return;
		};
		if !done.is_empty() {
			game.redists_done.extend(done.iter().cloned());
			self.update_game(game.clone());
		}
		if failed.is_empty() {
			self.sink.emit(Event::Notice(
				t!("redist.installed", name = game.name, n = done.len()).into_owned(),
			));
		} else {
			let list = failed
				.iter()
				.map(|(name, e)| format!("{name}: {e}"))
				.collect::<Vec<_>>()
				.join("; ");
			self.sink.emit(Event::Error(
				t!("err.redist", name = game.name, list = list).into_owned(),
			));
		}
		self.list_redists(&id);
	}
}

async fn install_one(
	proton: &crate::proton::ProtonBuild,
	prefix: &Path,
	steam_root: &Path,
	redist: &Redist,
) -> std::result::Result<(), String> {
	std::fs::create_dir_all(prefix).map_err(|e| e.to_string())?;
	let command = redist.command();
	tracing::info!(
		"installing {} into {}",
		redist.path.display(),
		prefix.display()
	);
	let status = tokio::process::Command::new(proton.script())
		.arg("run")
		.args(&command)
		.current_dir(redist.path.parent().unwrap_or(Path::new("/")))
		.env("STEAM_COMPAT_DATA_PATH", prefix)
		.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_root)
		.env("WINEDEBUG", "-all")
		.stdin(std::process::Stdio::null())
		.status()
		.await
		.map_err(|e| e.to_string())?;
	// Installers hand off to child processes; the next one waits for them.
	let _ = tokio::process::Command::new(proton.wine_bin("wineserver"))
		.arg("-w")
		.env("WINEPREFIX", prefix.join("pfx"))
		.status()
		.await;
	if Redist::succeeded(status.code()) {
		Ok(())
	} else {
		Err(t!("err.installer_code", code = status.code().unwrap_or(-1)).into_owned())
	}
}
