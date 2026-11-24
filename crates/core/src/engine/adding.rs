//! Looking into what the user picked, and adding it as a game.

use std::path::Path;

use rust_i18n::t;

use super::*;
use crate::archive::{self, ArchiveError};
use crate::library::Game;
use crate::{detect, fix, proton, steam};

/// Games keep their programs near the top.
const LIST_DEPTH: usize = 8;

/// Where adding a game got to, reported back to the command loop.
pub(super) enum AddStep {
	Ready {
		id: String,
		name: String,
		exe: PathBuf,
		prefix: Option<PathBuf>,
	},
	Installed {
		id: String,
		name: String,
		folder: PathBuf,
		prefix: Option<PathBuf>,
	},
	Failed {
		error: String,
	},
}

/// Runs a FreeTP (Inno Setup) installer silently into `target`.
async fn run_installer(
	proton: &crate::proton::ProtonBuild,
	prefix: &Path,
	steam_root: Option<&Path>,
	exe: &Path,
	target: &Path,
) -> Result<(), String> {
	std::fs::create_dir_all(prefix).map_err(|e| e.to_string())?;
	std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
	let steam_root = steam_root.map(Path::to_path_buf).unwrap_or_else(|| {
		std::env::var_os("HOME")
			.map(PathBuf::from)
			.unwrap_or_default()
			.join(".steam/steam")
	});
	tracing::info!("installing {} into {}", exe.display(), target.display());
	let status = tokio::process::Command::new(proton.script())
		.arg("run")
		.arg(exe)
		.arg(format!("/DIR={}", windows_path(target)))
		.args(["/TASKS=", "/SILENT", "/SUPPRESSMSGBOXES"])
		.current_dir(exe.parent().unwrap_or(Path::new("/")))
		.env("STEAM_COMPAT_DATA_PATH", prefix)
		.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_root)
		.env("WINEDEBUG", "-all")
		.stdin(std::process::Stdio::null())
		.status()
		.await
		.map_err(|e| e.to_string())?;
	// Inno Setup can hand off to a second process and quit.
	let _ = tokio::process::Command::new(proton.wine_bin("wineserver"))
		.arg("-w")
		.env("WINEPREFIX", prefix.join("pfx"))
		.status()
		.await;
	if !status.success() {
		return Err(t!("err.installer_code", code = status.code().unwrap_or(-1)).into_owned());
	}
	Ok(())
}

/// Wine sees the Linux root as drive Z:.
fn windows_path(path: &Path) -> String {
	format!("Z:{}", path.display()).replace('/', "\\")
}

impl State {
	pub(super) fn inspect_source(&self, request: u64, path: PathBuf, password: Option<String>) {
		let sink = self.sink.clone();
		tokio::spawn(async move {
			let result = inspect(&path, password).await.map(Box::new);
			sink.emit(Event::SourceInspected { request, result });
		});
	}

	pub(super) fn add_new_game(&mut self, mut new: NewGame) {
		if new.unpack.is_none() && new.install_into.is_none() {
			self.add_game(new.id, new.name, new.exe, new.prefix);
			return;
		}
		let id = new.id.clone().unwrap_or_else(|| Game::new("", "").id);
		// An installer's archive is only needed until it ran.
		let scratch = self.paths.cache_dir().join("installers").join(&id);
		if new.install_into.is_some()
			&& let Some(unpack) = &mut new.unpack
		{
			unpack.into = scratch.clone();
			unpack.game_dir = scratch.clone();
		}
		let installer = new.install_into.as_ref().map(|target| {
			let root = steam::root();
			let libraries = root
				.as_deref()
				.map(steam::library_folders)
				.unwrap_or_default();
			let builds =
				proton::find_installed(&self.paths.protons_dir(), root.as_deref(), &libraries);
			let proton =
				proton::choose(&builds, None, self.settings.default_proton.as_deref()).cloned();
			let prefix = new
				.prefix
				.clone()
				.unwrap_or_else(|| self.paths.default_prefixes_dir().join(&id));
			(proton, prefix, root, target.clone())
		});
		let sink = self.sink.clone();
		let done = self.unpacked.clone();
		tokio::spawn(async move {
			let pending_id = id.clone();
			let pending = |stage: PendingStage| {
				sink.emit(Event::Pending {
					id: id.clone(),
					name: new.name.clone(),
					stage,
				})
			};
			let mut exe = new.exe.clone();
			if let Some(unpack) = &new.unpack {
				pending(PendingStage::Unpacking(0.));
				if let Err(e) =
					unpack_archive(unpack, |p| pending(PendingStage::Unpacking(p))).await
				{
					sink.emit(Event::PendingEnded(id.clone()));
					let _ = done.send(AddStep::Failed {
						error: t!("err.unpack", name = new.name, error = e).into_owned(),
					});
					return;
				}
				exe = unpack.into.join(&new.exe);
			}
			let step = match installer {
				None => AddStep::Ready {
					id,
					name: new.name,
					exe,
					prefix: new.prefix,
				},
				Some((proton, prefix, root, target)) => {
					pending(PendingStage::Installing);
					let result = match proton {
						Some(proton) => {
							run_installer(&proton, &prefix, root.as_deref(), &exe, &target).await
						}
						None => Err(crate::launch::LaunchError::ProtonMissing.to_string()),
					};
					let _ = std::fs::remove_dir_all(&scratch);
					match result {
						Ok(()) => AddStep::Installed {
							id: id.clone(),
							name: new.name,
							folder: target,
							prefix: new.prefix,
						},
						Err(e) => AddStep::Failed {
							error: t!("err.install", name = new.name, error = e).into_owned(),
						},
					}
				}
			};
			sink.emit(Event::PendingEnded(pending_id));
			let _ = done.send(step);
		});
	}

	pub(super) fn add_step(&mut self, step: AddStep) {
		match step {
			AddStep::Ready {
				id,
				name,
				exe,
				prefix,
			} => self.add_game(Some(id), name, exe, prefix),
			AddStep::Installed {
				id,
				name,
				folder,
				prefix,
			} => self.sink.emit(Event::Installed {
				id,
				name,
				folder,
				prefix,
			}),
			AddStep::Failed { error } => self.sink.emit(Event::Error(error)),
		}
	}

	pub(super) fn add_game(
		&mut self,
		id: Option<String>,
		name: String,
		exe: PathBuf,
		prefix: Option<PathBuf>,
	) {
		let info = fix::scan(&fix::game_root(&exe));
		let mut game = Game::new(name, exe);
		if let Some(id) = id {
			game.id = id;
		}
		game.prefix = prefix;
		game.fix_overrides = info.overrides_string();
		game.real_app_id = info.real_app_id;
		game.fake_app_id = info.fake_app_id;
		tracing::info!(
			"adding {}: {:?} fix, overrides {:?}",
			game.name,
			info.vendor,
			game.fix_overrides
		);
		let id = game.id.clone();
		self.library.upsert(game);
		if self.save_library() {
			self.emit_library();
			self.sink.emit(Event::GameAdded(id.clone()));
			self.fetch_art(&id);
		} else {
			self.library.remove(&id);
		}
	}
}

async fn inspect(path: &Path, password: Option<String>) -> Result<Source, InspectError> {
	if path.is_dir() {
		let files = detect::list_files(path, LIST_DEPTH);
		let candidates = sorted_candidates(&files, path);
		let installers = installers(&candidates, &files);
		let pick = detect::auto_pick(&files)
			.cloned()
			.or_else(|| (candidates.len() == 1).then(|| candidates[0].clone()));
		let name = path
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default();
		return Ok(Source {
			kind: SourceKind::Folder,
			path: path.to_path_buf(),
			password: None,
			candidates,
			pick,
			name,
			top_dir: None,
			installers,
			fix: Some(fix::scan(path)),
		});
	}
	if archive::is_archive(path) {
		let archive = archive::first_volume(path);
		let password = password.unwrap_or_else(|| archive::ONLINEFIX_PASSWORD.to_owned());
		let entries = archive::list(&archive, &password)
			.await
			.map_err(|e| match e {
				ArchiveError::Password => InspectError::NeedPassword,
				e => InspectError::Failed(e.to_string()),
			})?;
		let files: Vec<PathBuf> = entries
			.iter()
			.filter(|e| !e.dir)
			.map(|e| e.path.clone())
			.collect();
		let top_dir = single_top_dir(&entries);
		let candidates = sorted_candidates(&files, Path::new(""));
		let installers = installers(&candidates, &files);
		let pick = detect::auto_pick(&files)
			.cloned()
			.or_else(|| (candidates.len() == 1).then(|| candidates[0].clone()));
		let name = top_dir
			.as_ref()
			.and_then(|d| d.file_name())
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_else(|| archive_stem(&archive));
		return Ok(Source {
			kind: SourceKind::Archive,
			path: archive,
			password: Some(password),
			candidates,
			pick,
			name,
			top_dir,
			installers,
			fix: None,
		});
	}
	if path.is_file() {
		let root = fix::game_root(path);
		let siblings = detect::list_files(path.parent().unwrap_or(&root), 0);
		let installers = installers(&[path.to_path_buf()], &siblings);
		return Ok(Source {
			kind: SourceKind::Exe,
			path: path.to_path_buf(),
			password: None,
			candidates: vec![path.to_path_buf()],
			pick: Some(path.to_path_buf()),
			name: detect::game_name(path),
			top_dir: None,
			installers,
			fix: Some(fix::scan(&root)),
		});
	}
	Err(InspectError::Failed(t!("err.not_a_source").into_owned()))
}

fn installers(candidates: &[PathBuf], files: &[PathBuf]) -> Vec<PathBuf> {
	candidates
		.iter()
		.filter(|c| detect::is_freetp_installer(c, files))
		.cloned()
		.collect()
}

/// Shallow programs first: the game's exe is rarely deep inside.
fn sorted_candidates(files: &[PathBuf], root: &Path) -> Vec<PathBuf> {
	let mut candidates: Vec<PathBuf> = files
		.iter()
		.filter(|f| detect::is_candidate(f))
		.cloned()
		.collect();
	candidates.sort_by_key(|f| {
		let rel = f.strip_prefix(root).unwrap_or(f);
		(
			rel.components().count(),
			rel.to_string_lossy().to_lowercase(),
		)
	});
	candidates
}

fn single_top_dir(entries: &[archive::Entry]) -> Option<PathBuf> {
	let mut tops = entries.iter().filter_map(|e| e.path.components().next());
	let first = tops.next()?;
	let all_same = tops.all(|c| c == first);
	let first = PathBuf::from(first.as_os_str());
	let is_dir = entries.iter().any(|e| e.path == first && e.dir)
		|| entries
			.iter()
			.any(|e| e.path.starts_with(&first) && e.path != first);
	(all_same && is_dir).then_some(first)
}

/// `Game.part1.rar` and `Game.rar` are both `Game`.
fn archive_stem(archive: &Path) -> String {
	let stem = archive
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	match stem.to_ascii_lowercase().rfind(".part") {
		Some(at) => stem[..at].to_owned(),
		None => stem,
	}
}

async fn unpack_archive(unpack: &Unpack, report: impl Fn(f32)) -> Result<(), String> {
	if let Ok(mut entries) = std::fs::read_dir(&unpack.game_dir)
		&& entries.next().is_some()
	{
		return Err(t!("err.folder_not_empty", path = unpack.game_dir.display()).into_owned());
	}
	archive::extract(&unpack.archive, &unpack.into, &unpack.password, report)
		.await
		.map_err(|e| e.to_string())?;
	if unpack.delete_archive {
		for volume in volumes(&unpack.archive) {
			if let Err(e) = std::fs::remove_file(&volume) {
				tracing::warn!("could not delete {}: {e}", volume.display());
			}
		}
	}
	Ok(())
}

/// The archive and its other parts.
fn volumes(first: &Path) -> Vec<PathBuf> {
	let stem = archive_stem(first);
	let Some(dir) = first.parent() else {
		return vec![first.to_path_buf()];
	};
	let multi = first
		.file_name()
		.and_then(|n| n.to_str())
		.is_some_and(|n| n.to_ascii_lowercase().contains(".part"));
	if !multi {
		return vec![first.to_path_buf()];
	}
	let prefix = format!("{}.part", stem.to_ascii_lowercase());
	std::fs::read_dir(dir)
		.map(|entries| {
			entries
				.flatten()
				.map(|e| e.path())
				.filter(|p| {
					p.file_name()
						.and_then(|n| n.to_str())
						.map(str::to_ascii_lowercase)
						.is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".rar"))
				})
				.collect()
		})
		.unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::archive::Entry;

	fn entry(path: &str, dir: bool) -> Entry {
		Entry {
			path: path.into(),
			dir,
		}
	}

	#[test]
	fn top_dir_of_archives() {
		let one = [entry("Game", true), entry("Game/a.exe", false)];
		assert_eq!(single_top_dir(&one), Some(PathBuf::from("Game")));
		let implied = [entry("Game/a.exe", false), entry("Game/b/c.dll", false)];
		assert_eq!(single_top_dir(&implied), Some(PathBuf::from("Game")));
		let flat = [entry("a.exe", false), entry("b.dll", false)];
		assert_eq!(single_top_dir(&flat), None);
		let two = [entry("Game/a.exe", false), entry("Fix/b.dll", false)];
		assert_eq!(single_top_dir(&two), None);
	}

	#[test]
	fn stems_and_volumes() {
		assert_eq!(
			archive_stem(Path::new("/d/Cool Game.part01.rar")),
			"Cool Game"
		);
		assert_eq!(archive_stem(Path::new("/d/Cool Game.rar")), "Cool Game");
		let dir = tempfile::tempdir().unwrap();
		for name in ["G.part1.rar", "G.part2.rar", "Other.part1.rar"] {
			std::fs::write(dir.path().join(name), "").unwrap();
		}
		let mut found = volumes(&dir.path().join("G.part1.rar"));
		found.sort();
		assert_eq!(
			found,
			[
				dir.path().join("G.part1.rar"),
				dir.path().join("G.part2.rar")
			]
		);
	}

	#[test]
	fn installer_paths() {
		assert_eq!(
			windows_path(Path::new("/mnt/games/My Game")),
			"Z:\\mnt\\games\\My Game"
		);
	}

	#[tokio::test]
	async fn finds_freetp_installers() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("setup.exe"), "").unwrap();
		std::fs::write(dir.path().join("data.ftp"), "").unwrap();
		let source = inspect(dir.path(), None).await.unwrap();
		assert_eq!(source.installers, [dir.path().join("setup.exe")]);
		let exe = inspect(&dir.path().join("setup.exe"), None).await.unwrap();
		assert_eq!(exe.installers, [dir.path().join("setup.exe")]);
	}

	#[tokio::test]
	async fn inspects_a_folder() {
		let dir = tempfile::tempdir().unwrap();
		let game = dir.path().join("Cool Game");
		std::fs::create_dir_all(game.join("bin")).unwrap();
		std::fs::write(game.join("bin/Cool.exe"), "").unwrap();
		std::fs::write(game.join("unins000.exe"), "").unwrap();
		std::fs::write(game.join("bin/OnlineFix64.dll"), "").unwrap();
		let source = inspect(&game, None).await.unwrap();
		assert_eq!(source.kind, SourceKind::Folder);
		assert_eq!(source.name, "Cool Game");
		assert_eq!(source.candidates, [game.join("bin/Cool.exe")]);
		assert_eq!(source.pick, Some(game.join("bin/Cool.exe")));
		assert!(source.installers.is_empty());
		assert_eq!(
			source.fix.unwrap().vendor,
			Some(crate::fix::Vendor::OnlineFix)
		);
	}
}
