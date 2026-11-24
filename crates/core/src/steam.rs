//! The local Steam install: library folders, Steam Linux Runtime, overlay,
//! and whether the client is up (fixes talk to it).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::vdf;

/// The Steam folder: `~/.steam/root` and friends point at it.
pub fn root() -> Option<PathBuf> {
	let home = std::env::var_os("HOME").map(PathBuf::from)?;
	[
		home.join(".steam/root"),
		home.join(".steam/steam"),
		home.join(".local/share/Steam"),
	]
	.into_iter()
	.filter_map(|p| p.canonicalize().ok())
	.find(|p| p.join("steamapps").is_dir())
}

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryFolder {
	pub path: PathBuf,
	pub apps: Vec<u32>,
}

/// Every library Steam knows; the root alone if the list can't be read.
pub fn library_folders(root: &Path) -> Vec<LibraryFolder> {
	let file = root.join("steamapps/libraryfolders.vdf");
	let parsed = std::fs::read_to_string(&file)
		.ok()
		.and_then(|text| vdf::parse(&text));
	let folders: Vec<LibraryFolder> = parsed
		.as_ref()
		.and_then(|root| root.obj("libraryfolders"))
		.map(|list| {
			list.iter()
				.filter_map(|(_, value)| match value {
					vdf::Value::Obj(folder) => Some(LibraryFolder {
						path: PathBuf::from(folder.str("path")?),
						apps: folder
							.obj("apps")
							.map(|apps| apps.iter().filter_map(|(id, _)| id.parse().ok()).collect())
							.unwrap_or_default(),
					}),
					vdf::Value::Str(_) => None,
				})
				.collect()
		})
		.unwrap_or_default();
	if folders.is_empty() {
		vec![LibraryFolder {
			path: root.to_path_buf(),
			apps: Vec::new(),
		}]
	} else {
		folders
	}
}

/// Where an installed app lives, from its `appmanifest_<id>.acf`.
pub fn app_dir(libraries: &[LibraryFolder], app_id: u32) -> Option<PathBuf> {
	libraries.iter().find_map(|lib| {
		let steamapps = lib.path.join("steamapps");
		let manifest =
			std::fs::read_to_string(steamapps.join(format!("appmanifest_{app_id}.acf"))).ok()?;
		let parsed = vdf::parse(&manifest)?;
		let install = parsed.obj("AppState")?.str("installdir")?;
		let dir = steamapps.join("common").join(install);
		dir.is_dir().then_some(dir)
	})
}

/// Steam Linux Runtime app ids, for the install hint when one is missing.
pub const RUNTIME_SOLDIER: u32 = 1_391_110;
pub const RUNTIME_SNIPER: u32 = 1_628_350;
pub const RUNTIME_4: u32 = 4_183_110;

/// The `run` script of the Steam Linux Runtime with this app id.
pub fn runtime(libraries: &[LibraryFolder], app_id: u32) -> Option<PathBuf> {
	let run = app_dir(libraries, app_id)?.join("run");
	run.is_file().then_some(run)
}

/// `LD_PRELOAD` entries that draw the Steam overlay over a game.
pub fn overlay_preload(root: &Path) -> Vec<PathBuf> {
	["ubuntu12_32", "ubuntu12_64"]
		.iter()
		.map(|dir| root.join(dir).join("gameoverlayrenderer.so"))
		.collect()
}

/// Whether the Steam client runs, going by its pid file and `/proc`.
pub fn is_running() -> bool {
	let home = std::env::var_os("HOME").map(PathBuf::from);
	let from_pid_file = home
		.and_then(|h| std::fs::read_to_string(h.join(".steam/steam.pid")).ok())
		.and_then(|pid| pid.trim().parse::<u32>().ok())
		.is_some_and(|pid| process_name(pid).as_deref() == Some("steam"));
	from_pid_file
		|| std::fs::read_dir("/proc").is_ok_and(|entries| {
			entries.flatten().any(|e| {
				e.file_name()
					.to_str()
					.and_then(|n| n.parse::<u32>().ok())
					.is_some_and(|pid| process_name(pid).as_deref() == Some("steam"))
			})
		})
}

fn process_name(pid: u32) -> Option<String> {
	std::fs::read_to_string(format!("/proc/{pid}/comm"))
		.ok()
		.map(|s| s.trim().to_owned())
}

/// Starts Steam in the tray and waits until a user is signed in, which is
/// when Steam rewrites `loginusers.vdf`. False if that doesn't happen in time.
pub async fn start_and_wait(root: &Path, timeout: Duration) -> bool {
	let users = root.join("config/loginusers.vdf");
	let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
	let before: Option<SystemTime> = modified(&users);
	let spawned = tokio::process::Command::new("steam")
		.arg("-silent")
		.stdin(std::process::Stdio::null())
		.stdout(std::process::Stdio::null())
		.stderr(std::process::Stdio::null())
		.spawn();
	if let Err(e) = spawned {
		tracing::warn!("could not start steam: {e}");
		return false;
	}
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		tokio::time::sleep(Duration::from_secs(1)).await;
		let now = modified(&users);
		if now.is_some() && now != before {
			return true;
		}
	}
	false
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_runtime_through_manifests() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().join("Steam");
		let other = dir.path().join("Lib");
		std::fs::create_dir_all(root.join("steamapps")).unwrap();
		std::fs::write(
			root.join("steamapps/libraryfolders.vdf"),
			format!(
				"\"libraryfolders\"\n{{\n\"0\" {{ \"path\" \"{}\" \"apps\" {{ \"10\" \"1\" }} }}\n\"1\" {{ \"path\" \"{}\" \"apps\" {{ \"1628350\" \"1\" }} }}\n}}\n",
				root.display(),
				other.display()
			),
		)
		.unwrap();
		let sniper = other.join("steamapps/common/SteamLinuxRuntime_sniper");
		std::fs::create_dir_all(&sniper).unwrap();
		std::fs::write(sniper.join("run"), "#!/bin/sh\n").unwrap();
		std::fs::write(
			other.join("steamapps/appmanifest_1628350.acf"),
			"\"AppState\"\n{\n\t\"appid\"\t\"1628350\"\n\t\"installdir\"\t\"SteamLinuxRuntime_sniper\"\n}\n",
		)
		.unwrap();

		let libraries = library_folders(&root);
		assert_eq!(libraries.len(), 2);
		assert_eq!(libraries[1].apps, [RUNTIME_SNIPER]);
		assert_eq!(
			runtime(&libraries, RUNTIME_SNIPER),
			Some(sniper.join("run"))
		);
		assert_eq!(runtime(&libraries, RUNTIME_4), None);
	}

	#[test]
	fn unreadable_list_falls_back_to_root() {
		let dir = tempfile::tempdir().unwrap();
		let libraries = library_folders(dir.path());
		assert_eq!(libraries[0].path, dir.path());
	}
}
