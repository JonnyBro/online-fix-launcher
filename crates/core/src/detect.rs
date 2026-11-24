//! Which file in a game folder (or archive listing) starts the game.

use std::path::{Path, PathBuf};

/// Programs that come with games but never start them.
fn is_helper(lower: &str) -> bool {
	const PREFIXES: &[&str] = &[
		"vcredist",
		"vc_redist",
		"dxwebsetup",
		"install",
		"uninstall",
		"updater",
		"patch",
		"origin",
		"uplay",
		"epicgames",
		"gog",
		"unitycrashhandler",
		"unitybugreporter",
		"winpixeventruntime",
		"monodistribution",
		"crashreportclient",
		"bugreporter",
		"sandbox",
		"editor",
		"config",
		"settings",
		"options",
		"benchmark",
		"profiler",
		"profiling",
	];
	let Some(stem) = lower.strip_suffix(".exe") else {
		return false;
	};
	if PREFIXES.iter().any(|p| stem.starts_with(p))
		|| matches!(stem, "steam" | "dxsetup" | "crashpad_handler")
	{
		return true;
	}
	if stem.starts_with("unins") && stem[5..].bytes().all(|b| b.is_ascii_digit()) && stem.len() > 5
	{
		return true;
	}
	if stem.starts_with("directx") && stem.contains("setup")
		|| stem.starts_with("dotnet") && stem.contains("setup")
		|| stem.starts_with("ue4") && stem.contains("prereq")
	{
		return true;
	}
	// .NET Framework offline installers, like NDP472-KB4054530-x86-x64-AllOS-ENU.exe.
	stem.starts_with("ndp")
		&& stem.contains("-kb")
		&& (stem.contains("-x86") || stem.contains("-x64"))
		&& stem.contains("-allos")
		&& stem.contains("-enu")
}

/// Files worth offering: programs and scripts that may start the game.
pub fn is_candidate(path: &Path) -> bool {
	let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
		return false;
	};
	let lower = name.to_ascii_lowercase();
	let runnable = [".exe", ".bat", ".vbs"]
		.iter()
		.any(|ext| lower.ends_with(ext));
	runnable && !is_helper(&lower)
}

/// The file that is surely the one to start: the EOS auth launcher, or a
/// Photon launcher (`Launcher.exe` next to `OnlineFix.json`). `files` is the
/// whole listing, so archives work without unpacking.
pub fn auto_pick(files: &[PathBuf]) -> Option<&PathBuf> {
	let named = |path: &Path, name: &str| {
		path.file_name()
			.and_then(|n| n.to_str())
			.is_some_and(|n| n.eq_ignore_ascii_case(name))
	};
	files
		.iter()
		.find(|f| named(f, "eosauthlauncher.exe"))
		.or_else(|| {
			files.iter().find(|f| {
				named(f, "launcher.exe")
					&& files
						.iter()
						.any(|o| o.parent() == f.parent() && named(o, "onlinefix.json"))
			})
		})
}

/// FreeTP installers come with a `.ftp` file beside them.
pub fn is_freetp_installer(exe: &Path, files: &[PathBuf]) -> bool {
	files.iter().any(|f| {
		f.parent() == exe.parent()
			&& f.extension()
				.and_then(|e| e.to_str())
				.is_some_and(|e| e.eq_ignore_ascii_case("ftp"))
	})
}

/// The shallowest folder among `files`: where an archive's game folder starts.
pub fn base_dir(files: &[PathBuf]) -> Option<&Path> {
	files
		.iter()
		.filter_map(|f| f.parent())
		.min_by_key(|p| p.components().count())
}

/// The exe's name, or its folder's name when the exe is a launcher.
pub fn game_name(exe: &Path) -> String {
	let stem = exe
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	let folder = exe
		.parent()
		.and_then(|p| p.file_name())
		.map(|s| s.to_string_lossy().into_owned());
	match folder {
		Some(folder) if stem.to_ascii_lowercase().contains("launcher") => folder,
		_ => stem,
	}
}

/// Every file under `root` as a full path, for [`auto_pick`] and the picker.
pub fn list_files(root: &Path, max_depth: usize) -> Vec<PathBuf> {
	fn walk(dir: &Path, depth: usize, max: usize, out: &mut Vec<PathBuf>) {
		let Ok(entries) = std::fs::read_dir(dir) else {
			return;
		};
		for entry in entries.flatten() {
			let Ok(kind) = entry.file_type() else {
				continue;
			};
			if kind.is_dir() && depth < max {
				walk(&entry.path(), depth + 1, max, out);
			} else if kind.is_file() {
				out.push(entry.path());
			}
		}
	}
	let mut files = Vec::new();
	walk(root, 0, max_depth, &mut files);
	files.sort();
	files
}

#[cfg(test)]
mod tests {
	use super::*;

	fn paths(list: &[&str]) -> Vec<PathBuf> {
		list.iter().map(PathBuf::from).collect()
	}

	#[test]
	fn helpers_are_not_candidates() {
		for helper in [
			"vc_redist.x64.exe",
			"UnityCrashHandler64.exe",
			"unins000.exe",
			"DXSETUP.exe",
			"NDP472-KB4054530-x86-x64-AllOS-ENU.exe",
			"UE4PrereqSetup_x64.exe",
			"CrashReportClient.exe",
			"readme.txt",
			"game.dll",
		] {
			assert!(!is_candidate(Path::new(helper)), "{helper}");
		}
		for game in [
			"Game.exe",
			"Start.bat",
			"setup.exe",
			"Uninstaller-Simulator.exe",
		] {
			assert_eq!(
				is_candidate(Path::new(game)),
				game != "Uninstaller-Simulator.exe",
				"{game}"
			);
		}
	}

	#[test]
	fn picks_launchers() {
		let eos = paths(&["G/Game.exe", "G/EOSAuthLauncher.exe"]);
		assert_eq!(auto_pick(&eos), Some(&eos[1]));
		let photon = paths(&["G/Launcher.exe", "G/OnlineFix.json", "G/Game.exe"]);
		assert_eq!(auto_pick(&photon), Some(&photon[0]));
		let plain = paths(&["G/Launcher.exe", "G/Game.exe"]);
		assert_eq!(auto_pick(&plain), None);
	}

	#[test]
	fn installers_and_base() {
		let files = paths(&[
			"Pack/Game/setup.exe",
			"Pack/Game/data.ftp",
			"Pack/readme.txt",
		]);
		assert!(is_freetp_installer(&files[0], &files));
		assert_eq!(base_dir(&files), Some(Path::new("Pack")));
	}

	#[test]
	fn names() {
		assert_eq!(
			game_name(Path::new("/g/Cool Game/Launcher.exe")),
			"Cool Game"
		);
		assert_eq!(game_name(Path::new("/g/Cool Game/bin/Cool.exe")), "Cool");
	}
}
