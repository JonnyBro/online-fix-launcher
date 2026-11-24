//! Looking into what the user picked, and adding it as a game.

use std::path::Path;

use rust_i18n::t;

use super::*;
use crate::archive::{self, ArchiveError};
use crate::library::Game;
use crate::{detect, fix};

/// Games keep their programs near the top.
const LIST_DEPTH: usize = 8;

impl State {
	pub(super) fn inspect_source(&self, request: u64, path: PathBuf, password: Option<String>) {
		let sink = self.sink.clone();
		tokio::spawn(async move {
			let result = inspect(&path, password).await.map(Box::new);
			sink.emit(Event::SourceInspected { request, result });
		});
	}

	pub(super) fn add_new_game(&mut self, new: NewGame) {
		let Some(unpack) = new.unpack.clone() else {
			self.add_game(new.name, new.exe, new.prefix);
			return;
		};
		let id = Game::new("", "").id;
		let sink = self.sink.clone();
		let done = self.unpacked.clone();
		let name = new.name.clone();
		tokio::spawn(async move {
			let report = |progress: f32| {
				sink.emit(Event::Unpacking {
					id: id.clone(),
					name: name.clone(),
					progress,
				})
			};
			report(0.);
			let result = unpack_archive(&unpack, report).await;
			sink.emit(Event::UnpackEnded(id.clone()));
			let _ = done.send((new, result));
		});
	}

	pub(super) fn unpacked(&mut self, new: NewGame, result: Result<(), String>) {
		match result {
			Ok(()) => {
				let into = new
					.unpack
					.as_ref()
					.map(|u| u.into.clone())
					.unwrap_or_default();
				self.add_game(new.name, into.join(&new.exe), new.prefix);
			}
			Err(e) => self.sink.emit(Event::Error(
				t!("err.unpack", name = new.name, error = e).into_owned(),
			)),
		}
	}

	pub(super) fn add_game(&mut self, name: String, exe: PathBuf, prefix: Option<PathBuf>) {
		let info = fix::scan(&fix::game_root(&exe));
		let mut game = Game::new(name, exe);
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
			fix: None,
		});
	}
	if path.is_file() {
		let root = fix::game_root(path);
		return Ok(Source {
			kind: SourceKind::Exe,
			path: path.to_path_buf(),
			password: None,
			candidates: vec![path.to_path_buf()],
			pick: Some(path.to_path_buf()),
			name: detect::game_name(path),
			top_dir: None,
			fix: Some(fix::scan(&root)),
		});
	}
	Err(InspectError::Failed(t!("err.not_a_source").into_owned()))
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
		assert_eq!(
			source.fix.unwrap().vendor,
			Some(crate::fix::Vendor::OnlineFix)
		);
	}
}
