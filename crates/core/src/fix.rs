//! What a multiplayer fix put into a game folder: the DLLs Wine must load
//! from the folder instead of its own, and the app ids from the fix's INI.

use std::path::{Path, PathBuf};

use crate::ini::Ini;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
	OnlineFix,
	FreeTp,
	Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOrder {
	/// `n`: only the game's copy.
	Native,
	/// `n,b`: the game's copy, Wine's when there is none. Proxies of system
	/// DLLs (`winmm`, `winhttp`) load the real one through this.
	NativeThenBuiltin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
	/// Lowercase, without `.dll`.
	pub dll: String,
	pub order: LoadOrder,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FixInfo {
	pub vendor: Option<Vendor>,
	pub overrides: Vec<Override>,
	pub real_app_id: Option<u32>,
	pub fake_app_id: Option<u32>,
	/// `OnlineFix.ini` and `SteamFix.ini` files.
	pub ini_files: Vec<PathBuf>,
	/// Folder of the SteamFix DLLs.
	pub steamfix_dir: Option<PathBuf>,
	/// EOS emulation, which needs the EOS auth launcher.
	pub eos: bool,
	/// Folders of a Photon (custom server) launcher: `Launcher.exe` with
	/// `launch_data.of*` or `OnlineFix.json` next to it.
	pub photon_dirs: Vec<PathBuf>,
}

impl FixInfo {
	pub fn is_empty(&self) -> bool {
		self.overrides.is_empty() && self.ini_files.is_empty() && self.photon_dirs.is_empty()
	}

	/// The `WINEDLLOVERRIDES` part for the fix, like `winmm=n,b;steam_api64=n`.
	pub fn overrides_string(&self) -> String {
		join_overrides(&self.overrides)
	}

	fn add(&mut self, dll: &str, order: LoadOrder) {
		let dll = dll.to_ascii_lowercase();
		if !self.overrides.iter().any(|o| o.dll == dll) {
			self.overrides.push(Override { dll, order });
		}
	}
}

pub fn join_overrides(overrides: &[Override]) -> String {
	overrides
		.iter()
		.map(|o| {
			let order = match o.order {
				LoadOrder::Native => "n",
				LoadOrder::NativeThenBuiltin => "n,b",
			};
			format!("{}={order}", o.dll)
		})
		.collect::<Vec<_>>()
		.join(";")
}

/// The game's top folder for an exe deep inside it, like Unreal's
/// `Game/Binaries/Win64/Game-Win64-Shipping.exe`; fixes can sit anywhere in it.
pub fn game_root(exe: &Path) -> PathBuf {
	let mut dir = exe.parent().unwrap_or(exe);
	while let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
		let lower = name.to_ascii_lowercase();
		if !matches!(
			lower.as_str(),
			"binaries" | "win64" | "win32" | "x64" | "x86" | "bin" | "bin64"
		) {
			break;
		}
		match dir.parent() {
			Some(parent) => dir = parent,
			None => break,
		}
	}
	// Unreal puts the shipping exe in `<Project>/Binaries`, next to `Engine`.
	if dir.parent().is_some_and(|p| p.join("Engine").is_dir()) {
		dir = dir.parent().unwrap_or(dir);
	}
	dir.to_path_buf()
}

/// Unreal plugins bury their own `steam_api64.dll` eight folders deep.
const MAX_DEPTH: usize = 12;

/// Looks through the game folder. Unreadable folders are skipped.
pub fn scan(game_dir: &Path) -> FixInfo {
	let mut files = Vec::new();
	walk(game_dir, 0, &mut files);
	files.sort();
	let mut info = FixInfo::default();
	for path in &files {
		let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
			continue;
		};
		let lower = name.to_ascii_lowercase();
		let dir = path.parent().unwrap_or(game_dir);
		match classify(&lower) {
			Some(Kind::DllList) => {
				if let Ok(text) = std::fs::read_to_string(path) {
					for dll in dll_list(&text) {
						info.add(&dll, LoadOrder::Native);
					}
				}
			}
			Some(Kind::Ini { freetp }) => {
				read_ini(path, freetp, &mut info);
				info.ini_files.push(path.clone());
			}
			Some(Kind::PhotonData) => {
				if has_file(dir, "launcher.exe") && !info.photon_dirs.iter().any(|d| d == dir) {
					info.photon_dirs.push(dir.to_path_buf());
				}
			}
			Some(Kind::Dll { order, eos }) => {
				if lower.starts_with("steamfix") {
					info.steamfix_dir.get_or_insert_with(|| dir.to_path_buf());
				}
				if lower.starts_with("onlinefix") {
					info.vendor.get_or_insert(Vendor::OnlineFix);
				}
				info.eos |= eos;
				let stem = &name[..name.len() - ".dll".len()];
				info.add(stem, order);
			}
			None => {}
		}
	}
	if info.vendor.is_none() && !info.is_empty() {
		info.vendor = Some(Vendor::Unknown);
	}
	info
}

enum Kind {
	Dll {
		order: LoadOrder,
		eos: bool,
	},
	/// `winmm.txt` or `dlllist.txt`: more DLLs to override, one per line.
	DllList,
	Ini {
		freetp: bool,
	},
	PhotonData,
}

fn classify(lower: &str) -> Option<Kind> {
	if lower == "winmm.txt" || lower == "dlllist.txt" {
		return Some(Kind::DllList);
	}
	if lower == "onlinefix.ini" {
		return Some(Kind::Ini { freetp: false });
	}
	if lower == "steamfix.ini" {
		return Some(Kind::Ini { freetp: true });
	}
	if lower.starts_with("launch_data.of") || lower == "onlinefix.json" {
		return Some(Kind::PhotonData);
	}
	let stem = lower.strip_suffix(".dll")?;
	if stem.starts_with("win") {
		return Some(Kind::Dll {
			order: LoadOrder::NativeThenBuiltin,
			eos: false,
		});
	}
	let eos = stem.starts_with("eos") || stem.starts_with("epicfix");
	let fix = eos
		|| stem.starts_with("online")
		|| stem.starts_with("steam")
		|| stem == "emp"
		|| stem == "custom";
	fix.then_some(Kind::Dll {
		order: LoadOrder::Native,
		eos,
	})
}

/// DLL names from a list file, which may hold Windows paths.
fn dll_list(text: &str) -> Vec<String> {
	text.lines()
		.filter_map(|line| {
			let name = line.trim().rsplit(['\\', '/']).next()?;
			let stem = name
				.len()
				.checked_sub(4)
				.filter(|&at| name[at..].eq_ignore_ascii_case(".dll"))
				.map(|at| &name[..at])?;
			(!stem.is_empty()).then(|| stem.to_ascii_lowercase())
		})
		.collect()
}

fn read_ini(path: &Path, freetp: bool, info: &mut FixInfo) {
	let Ok(text) = std::fs::read_to_string(path) else {
		return;
	};
	let ini = Ini::parse(&text);
	let id = |section: &str, key: &str| ini.get(section, key).and_then(|v| v.parse().ok());
	// After the FreeTP patch the game's id moves to its own section.
	info.real_app_id = info
		.real_app_id
		.or_else(|| id("OnlineFix Linux", "RealAppId"))
		.or_else(|| id("Main", "RealAppId"));
	info.fake_app_id = info.fake_app_id.or_else(|| id("Main", "FakeAppId"));
	info.vendor.get_or_insert(if freetp {
		Vendor::FreeTp
	} else {
		Vendor::OnlineFix
	});
}

fn has_file(dir: &Path, lower_name: &str) -> bool {
	std::fs::read_dir(dir).is_ok_and(|entries| {
		entries.flatten().any(|e| {
			e.file_name()
				.to_str()
				.is_some_and(|n| n.eq_ignore_ascii_case(lower_name))
		})
	})
}

fn walk(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
	let Ok(entries) = std::fs::read_dir(dir) else {
		return;
	};
	for entry in entries.flatten() {
		let Ok(kind) = entry.file_type() else {
			continue;
		};
		let path = entry.path();
		if kind.is_dir() {
			if depth < MAX_DEPTH {
				walk(&path, depth + 1, files);
			}
		} else if kind.is_file() {
			files.push(path);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn touch(root: &Path, rel: &str, text: &str) {
		let path = root.join(rel);
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, text).unwrap();
	}

	#[test]
	fn onlinefix_folder() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path();
		touch(root, "Game/Binaries/Win64/Game.exe", "");
		touch(root, "Game/Binaries/Win64/OnlineFix64.dll", "");
		touch(root, "Game/Binaries/Win64/steam_api64.dll", "");
		touch(root, "Game/Binaries/Win64/winmm.dll", "");
		touch(
			root,
			"Game/Binaries/Win64/winmm.txt",
			"OnlineFix64.dll\r\nC:\\x\\Extra.DLL\r\nreadme.txt\r\n",
		);
		touch(
			root,
			"Game/Binaries/Win64/OnlineFix.ini",
			"[Main]\nRealAppId=1234\nFakeAppId=480\n",
		);
		touch(root, "Game/Content/textures.dll", "");
		let info = scan(root);
		assert_eq!(info.vendor, Some(Vendor::OnlineFix));
		assert_eq!(info.real_app_id, Some(1234));
		assert_eq!(info.fake_app_id, Some(480));
		assert!(!info.eos);
		let mut overrides: Vec<_> = info
			.overrides_string()
			.split(';')
			.map(str::to_owned)
			.collect();
		overrides.sort();
		assert_eq!(
			overrides,
			["extra=n", "onlinefix64=n", "steam_api64=n", "winmm=n,b"]
		);
	}

	#[test]
	fn freetp_after_patch_keeps_real_id() {
		let dir = tempfile::tempdir().unwrap();
		touch(
			dir.path(),
			"SteamFix.ini",
			"[Main]\nRealAppId=480\n[OnlineFix Linux]\nRealAppId=777\n",
		);
		touch(dir.path(), "SteamFix64.dll", "");
		let info = scan(dir.path());
		assert_eq!(info.vendor, Some(Vendor::FreeTp));
		assert_eq!(info.real_app_id, Some(777));
		assert_eq!(info.steamfix_dir.as_deref(), Some(dir.path()));
	}

	#[test]
	fn eos_and_photon() {
		let dir = tempfile::tempdir().unwrap();
		touch(dir.path(), "EOSSDK-Win64-Shipping.dll", "");
		touch(dir.path(), "Launcher/Launcher.exe", "");
		touch(dir.path(), "Launcher/launch_data.of", "");
		let info = scan(dir.path());
		assert!(info.eos);
		assert_eq!(info.overrides_string(), "eossdk-win64-shipping=n");
		assert_eq!(info.photon_dirs, [dir.path().join("Launcher")]);
	}

	#[test]
	fn root_of_unreal_exe() {
		let dir = tempfile::tempdir().unwrap();
		touch(dir.path(), "Engine/x", "");
		let exe = dir
			.path()
			.join("Game/Binaries/Win64/Game-Win64-Shipping.exe");
		assert_eq!(game_root(&exe), dir.path());
		assert_eq!(
			game_root(Path::new("/g/Game/Game.exe")),
			Path::new("/g/Game")
		);
	}

	#[test]
	fn plain_game_has_no_fix() {
		let dir = tempfile::tempdir().unwrap();
		touch(dir.path(), "Game.exe", "");
		touch(dir.path(), "d3dcompiler_47.dll", "");
		let info = scan(dir.path());
		assert!(info.is_empty());
		assert_eq!(info.vendor, None);
	}
}
