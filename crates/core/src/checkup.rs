//! What the launcher needs from the system, checked in one go.

use rust_i18n::t;

use crate::paths::AppPaths;
use crate::settings::Settings;
use crate::{archive, proton, steam};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
	Ok,
	Warning,
	Problem,
	/// Not set up, and that's fine.
	Skipped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
	pub level: Level,
	pub title: String,
	pub detail: String,
	/// A link that fixes it, like `steam://install/<id>`.
	pub link: Option<String>,
}

impl Check {
	fn new(level: Level, title: impl Into<String>, detail: impl Into<String>) -> Self {
		Self {
			level,
			title: title.into(),
			detail: detail.into(),
			link: None,
		}
	}
}

pub fn run(paths: &AppPaths, settings: &Settings) -> Vec<Check> {
	let mut checks = Vec::new();
	let root = steam::root();
	let libraries = root
		.as_deref()
		.map(steam::library_folders)
		.unwrap_or_default();
	checks.push(match &root {
		Some(root) => Check::new(Level::Ok, t!("chk.steam_ok"), root.display().to_string()),
		None => Check::new(
			Level::Problem,
			t!("chk.steam_missing"),
			t!("chk.steam_missing_detail"),
		),
	});

	let builds = proton::find_installed(&paths.protons_dir(), root.as_deref(), &libraries);
	let chosen = proton::choose(&builds, None, settings.default_proton.as_deref());
	checks.push(match chosen {
		Some(build) => Check::new(
			Level::Ok,
			t!("chk.proton_ok", n = builds.len()),
			t!("chk.proton_default", name = build.name),
		),
		None => Check::new(
			Level::Problem,
			t!("chk.proton_missing"),
			t!("chk.proton_missing_detail"),
		),
	});
	if let Some(dir) = &settings.default_proton
		&& !builds.iter().any(|b| &b.dir == dir)
	{
		checks.push(Check::new(
			Level::Warning,
			t!("chk.proton_default_gone"),
			dir.display().to_string(),
		));
	}

	let mut runtimes: Vec<u32> = builds.iter().filter_map(|b| b.runtime_app_id).collect();
	runtimes.sort_unstable();
	runtimes.dedup();
	let missing: Vec<u32> = runtimes
		.into_iter()
		.filter(|id| steam::runtime(&libraries, *id).is_none())
		.collect();
	if missing.is_empty() {
		checks.push(Check::new(Level::Ok, t!("chk.runtime_ok"), ""));
	}
	for id in missing {
		checks.push(Check {
			link: Some(format!("steam://install/{id}")),
			..Check::new(
				Level::Warning,
				t!("chk.runtime_missing", id = id),
				t!("chk.runtime_missing_detail"),
			)
		});
	}

	let seven = archive::which("7z").or_else(|| archive::which("7zz"));
	let unrar = archive::which("unrar");
	checks.push(match (seven, unrar) {
		(Some(_), _) => Check::new(Level::Ok, t!("chk.archiver_ok"), ""),
		(None, Some(_)) => Check::new(
			Level::Warning,
			t!("chk.archiver_rar_only"),
			t!("chk.archiver_detail"),
		),
		(None, None) => Check::new(
			Level::Warning,
			t!("chk.archiver_missing"),
			t!("chk.archiver_detail"),
		),
	});

	checks.push(match archive::which("winetricks") {
		Some(_) => Check::new(Level::Ok, t!("chk.winetricks_ok"), ""),
		None => Check::new(
			Level::Skipped,
			t!("chk.winetricks_missing"),
			t!("chk.winetricks_detail"),
		),
	});

	let games_dir = settings.games_dir();
	let writable = std::fs::create_dir_all(&games_dir).is_ok() && tempfile_in(&games_dir).is_some();
	checks.push(if writable {
		Check::new(
			Level::Ok,
			t!("chk.games_dir_ok"),
			games_dir.display().to_string(),
		)
	} else {
		Check::new(
			Level::Problem,
			t!("chk.games_dir_bad"),
			games_dir.display().to_string(),
		)
	});

	checks.push(if settings.steamgriddb_key.is_empty() {
		Check::new(
			Level::Skipped,
			t!("chk.sgdb_none"),
			t!("chk.sgdb_none_detail"),
		)
	} else {
		Check::new(Level::Ok, t!("chk.sgdb_ok"), "")
	});
	checks
}

/// Whether a file can be made in `dir`; it's removed right away.
fn tempfile_in(dir: &std::path::Path) -> Option<()> {
	let probe = dir.join(".onlinefix-launcher-probe");
	std::fs::write(&probe, b"").ok()?;
	std::fs::remove_file(&probe).ok()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unwritable_games_dir_is_a_problem() {
		let dir = tempfile::tempdir().unwrap();
		let paths = AppPaths::under(dir.path());
		let settings = Settings {
			games_dir: Some("/proc/no-games-here".into()),
			..Settings::default()
		};
		let checks = run(&paths, &settings);
		let games = checks
			.iter()
			.find(|c| c.detail == "/proc/no-games-here")
			.unwrap();
		assert_eq!(games.level, Level::Problem);
		let ok = Settings {
			games_dir: Some(dir.path().join("games")),
			..Settings::default()
		};
		assert!(
			run(&paths, &ok).iter().any(|c| c.level == Level::Ok
				&& c.detail == dir.path().join("games").display().to_string())
		);
	}
}
