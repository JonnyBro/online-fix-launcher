//! Backups of a prefix's user folders, where games and OnlineFix keep saves.
//! An archive holds paths under `drive_c/users`, so it restores into any
//! prefix, the same game's new one included.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Folders under `drive_c/users` that hold saves.
const SAVE_DIRS: &[&str] = &[
	"steamuser/AppData/Roaming",
	"steamuser/AppData/Local",
	"steamuser/AppData/LocalLow",
	"steamuser/Documents",
	"steamuser/Saved Games",
	"Public/Documents",
];

/// Caches and Windows' own files, big and useless in a backup.
const SKIPPED: &[&str] = &[
	"steamuser/AppData/Local/Temp",
	"steamuser/AppData/Local/Microsoft",
	"steamuser/AppData/Local/D3DSCache",
	"steamuser/AppData/Roaming/Microsoft",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Backup {
	pub path: PathBuf,
	/// Unix time.
	pub created: u64,
	pub size: u64,
}

/// `drive_c/users` of a Proton prefix (`STEAM_COMPAT_DATA_PATH`).
fn users_dir(prefix: &Path) -> PathBuf {
	prefix.join("pfx/drive_c/users")
}

/// Backups are kept by game name, so a game added again finds them.
pub fn backups_dir(root: &Path, game_name: &str) -> PathBuf {
	let folder: String = game_name
		.chars()
		.map(|c| if c == '/' || c == '\0' { '_' } else { c })
		.collect();
	root.join(folder.trim().trim_matches('.'))
}

/// Newest first.
pub fn list(dir: &Path) -> Vec<Backup> {
	let mut backups: Vec<Backup> = std::fs::read_dir(dir)
		.map(|entries| {
			entries
				.flatten()
				.filter(|e| e.file_name().to_string_lossy().ends_with(".tar.gz"))
				.filter_map(|e| {
					let meta = e.metadata().ok()?;
					Some(Backup {
						path: e.path(),
						created: meta
							.modified()
							.ok()?
							.duration_since(UNIX_EPOCH)
							.ok()?
							.as_secs(),
						size: meta.len(),
					})
				})
				.collect()
		})
		.unwrap_or_default();
	backups.sort_by(|a, b| b.created.cmp(&a.created).then(b.path.cmp(&a.path)));
	backups
}

/// Packs the prefix's save folders into `dir`; `None` when there's nothing
/// to save (the game never ran, say).
pub fn back_up(prefix: &Path, dir: &Path, stamp: &str) -> Result<Option<Backup>, String> {
	let users = users_dir(prefix);
	let present: Vec<&str> = SAVE_DIRS
		.iter()
		.copied()
		.filter(|d| has_files(&users.join(d), &users))
		.collect();
	if present.is_empty() {
		return Ok(None);
	}
	std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
	let path = dir.join(format!("{stamp}.tar.gz"));
	let partial = path.with_extension("partial");
	let result = (|| -> std::io::Result<()> {
		let file = std::fs::File::create(&partial)?;
		let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
		let mut builder = tar::Builder::new(gz);
		// Links stay links: one pointing at the home folder must not pull it in.
		builder.follow_symlinks(false);
		for dir in present {
			add_dir(&mut builder, &users, &users.join(dir))?;
		}
		builder.into_inner()?.finish()?;
		Ok(())
	})();
	if let Err(e) = result {
		let _ = std::fs::remove_file(&partial);
		return Err(e.to_string());
	}
	std::fs::rename(&partial, &path).map_err(|e| e.to_string())?;
	Ok(list(dir).into_iter().find(|b| b.path == path))
}

fn skipped(users: &Path, path: &Path) -> bool {
	path.strip_prefix(users)
		.is_ok_and(|rel| SKIPPED.iter().any(|s| rel.starts_with(s)))
}

/// Plain Wine links `Documents` and the like to the home folder; those
/// aren't the prefix's to back up.
fn has_files(dir: &Path, users: &Path) -> bool {
	let linked = std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink());
	if linked || skipped(users, dir) {
		return false;
	}
	std::fs::read_dir(dir).is_ok_and(|entries| {
		entries.flatten().any(|e| match e.file_type() {
			Ok(t) if t.is_dir() => has_files(&e.path(), users),
			Ok(t) => t.is_file(),
			Err(_) => false,
		})
	})
}

fn add_dir<W: std::io::Write>(
	builder: &mut tar::Builder<W>,
	users: &Path,
	dir: &Path,
) -> std::io::Result<()> {
	if skipped(users, dir) {
		return Ok(());
	}
	for entry in std::fs::read_dir(dir)?.flatten() {
		let path = entry.path();
		let Ok(rel) = path.strip_prefix(users) else {
			continue;
		};
		let kind = entry.file_type()?;
		if kind.is_dir() {
			add_dir(builder, users, &path)?;
		} else if kind.is_file() || kind.is_symlink() {
			builder.append_path_with_name(&path, rel)?;
		}
	}
	Ok(())
}

/// Unpacks a backup over the prefix's user folders; files with the same
/// names are replaced, others stay.
pub fn restore(backup: &Path, prefix: &Path) -> Result<(), String> {
	let users = users_dir(prefix);
	std::fs::create_dir_all(&users).map_err(|e| e.to_string())?;
	let file = std::fs::File::open(backup).map_err(|e| e.to_string())?;
	let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
	archive.set_overwrite(true);
	archive.unpack(&users).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn write(path: &Path, text: &str) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, text).unwrap();
	}

	#[test]
	fn backs_up_and_restores_saves_only() {
		let dir = tempfile::tempdir().unwrap();
		let prefix = dir.path().join("prefix");
		let users = users_dir(&prefix);
		write(
			&users.join("steamuser/AppData/LocalLow/Studio/Game/save.dat"),
			"level 3",
		);
		write(
			&users.join("Public/Documents/OnlineFix/480/stats.json"),
			"{}",
		);
		write(&users.join("steamuser/AppData/Local/Temp/huge.tmp"), "junk");
		std::os::unix::fs::symlink(dir.path(), users.join("steamuser/Documents")).unwrap();

		let backups = backups_dir(&dir.path().join("backups"), "Cool/Game");
		assert!(backups.ends_with("Cool_Game"));
		let made = back_up(&prefix, &backups, "1").unwrap().unwrap();
		assert_eq!(list(&backups), std::slice::from_ref(&made));

		let fresh = dir.path().join("fresh");
		restore(&made.path, &fresh).unwrap();
		let restored = users_dir(&fresh);
		assert_eq!(
			std::fs::read_to_string(
				restored.join("steamuser/AppData/LocalLow/Studio/Game/save.dat")
			)
			.unwrap(),
			"level 3"
		);
		assert!(
			restored
				.join("Public/Documents/OnlineFix/480/stats.json")
				.is_file()
		);
		assert!(!restored.join("steamuser/AppData/Local/Temp").exists());
		// A save folder that links out of the prefix isn't followed.
		assert!(!restored.join("steamuser/Documents").exists());
	}

	#[test]
	fn nothing_to_back_up() {
		let dir = tempfile::tempdir().unwrap();
		let prefix = dir.path().join("prefix");
		write(
			&users_dir(&prefix).join("steamuser/AppData/Local/Temp/x"),
			"junk",
		);
		assert_eq!(back_up(&prefix, &dir.path().join("b"), "1").unwrap(), None);
	}
}
