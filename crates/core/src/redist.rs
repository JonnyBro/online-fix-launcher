//! Runtimes games ship next to themselves (`_CommonRedist` and the like):
//! Visual C++, DirectX, .NET, XNA, PhysX. Steam runs them on first launch;
//! here the user does it from the game's panel.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Folder names, lowercased, that hold such installers.
const FOLDERS: &[&str] = &[
	"_commonredist",
	"commonredist",
	"redist",
	"_redist",
	"redist_packages",
	"redistributables",
	"prerequisites",
	"_prerequisites",
	"directx",
	"vcredist",
];
/// How far up from the exe's folder to look; Unreal games keep theirs at
/// the top, above `Game/Binaries/Win64`.
const LEVELS_UP: usize = 3;
const DEPTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
	VisualCpp,
	DirectX,
	DotNet,
	Xna,
	PhysX,
	UnrealPrereqs,
	OpenAl,
	/// Runs with its own window: no known way to make it quiet.
	Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Redist {
	pub path: PathBuf,
	pub kind: Kind,
	/// Like `2019 x64`; what the path says about the version.
	pub detail: String,
	/// Stays the same when the game folder moves; kept in the game once installed.
	pub key: String,
}

impl Redist {
	fn new(path: PathBuf) -> Option<Self> {
		let name = path.file_name()?.to_str()?.to_ascii_lowercase();
		let lower = path.to_string_lossy().to_ascii_lowercase();
		let msi = name.ends_with(".msi");
		if !msi && !name.ends_with(".exe") {
			return None;
		}
		let kind = if name.starts_with("vcredist") || name.starts_with("vc_redist") {
			Kind::VisualCpp
		} else if name == "dxsetup.exe" {
			Kind::DirectX
		} else if name.starts_with("ndp") || name.starts_with("dotnet") {
			Kind::DotNet
		} else if name.starts_with("xna") {
			Kind::Xna
		} else if name.starts_with("physx") {
			Kind::PhysX
		} else if name.starts_with("ue4prereq") || name.starts_with("ueprereq") {
			Kind::UnrealPrereqs
		} else if name.starts_with("oalinst") {
			Kind::OpenAl
		} else if lower.contains("directx") {
			// The rest of the DirectX folder is pieces DXSETUP itself unpacks.
			return None;
		} else {
			Kind::Other
		};
		let mut detail = Vec::new();
		if let Some(year) = year(&lower) {
			detail.push(year.to_string());
		}
		if lower.contains("x64") || lower.contains("amd64") || lower.contains("win64") {
			detail.push("x64".into());
		} else if lower.contains("x86") || lower.contains("win32") {
			detail.push("x86".into());
		}
		let key = path
			.iter()
			.rev()
			.take(2)
			.collect::<Vec<_>>()
			.into_iter()
			.rev()
			.map(|p| p.to_string_lossy().to_ascii_lowercase())
			.collect::<Vec<_>>()
			.join("/");
		Some(Self {
			path,
			kind,
			detail: detail.join(" "),
			key,
		})
	}

	/// What to run in Wine, and its flags for an unattended install.
	pub fn command(&self) -> Vec<String> {
		let file = crate::launch::windows_path(&self.path);
		let lower = self.path.to_string_lossy().to_ascii_lowercase();
		let year = year(&lower);
		if self
			.path
			.extension()
			.is_some_and(|e| e.eq_ignore_ascii_case("msi"))
		{
			return vec![
				"msiexec".into(),
				"/i".into(),
				file,
				"/qn".into(),
				"/norestart".into(),
			];
		}
		let flags: &[&str] = match self.kind {
			Kind::VisualCpp => match year {
				Some(..=2008) => &["/q"],
				Some(2010) => &["/q", "/norestart"],
				_ => &["/install", "/quiet", "/norestart"],
			},
			Kind::DirectX => &["/silent"],
			Kind::DotNet | Kind::UnrealPrereqs => &["/q", "/norestart"],
			Kind::PhysX => &["/quiet"],
			Kind::OpenAl => &["/s"],
			Kind::Xna | Kind::Other => &[],
		};
		std::iter::once(file)
			.chain(flags.iter().map(|f| (*f).to_owned()))
			.collect()
	}

	/// Codes the Windows installers give for "fine": done, needs a reboot,
	/// or a newer version is already there.
	pub fn succeeded(code: Option<i32>) -> bool {
		matches!(code, Some(0 | 1638 | 1641 | 3010))
	}
}

fn year(lower: &str) -> Option<u32> {
	(2005..=2030).rev().find(|y| lower.contains(&y.to_string()))
}

/// Installers in the redist folders around the game, sorted by kind and path.
pub fn find(exe: &Path) -> Vec<Redist> {
	let home = std::env::var_os("HOME").map(PathBuf::from);
	let mut roots: Vec<PathBuf> = Vec::new();
	for dir in exe.ancestors().skip(1).take(LEVELS_UP + 1) {
		if dir.parent().is_none() || Some(dir) == home.as_deref() {
			break;
		}
		let unreal = dir.join("Engine/Extras/Redist");
		if unreal.is_dir() {
			roots.push(unreal);
		}
		let Ok(entries) = std::fs::read_dir(dir) else {
			continue;
		};
		for entry in entries.flatten() {
			let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
			if FOLDERS.contains(&name.as_str()) && entry.path().is_dir() {
				roots.push(entry.path());
			}
		}
	}
	let mut found = Vec::new();
	for root in &roots {
		collect(root, DEPTH, &mut found);
	}
	let mut found: Vec<Redist> = found.into_iter().filter_map(Redist::new).collect();
	found.sort_by(|a, b| (a.kind as u8, &a.path).cmp(&(b.kind as u8, &b.path)));
	found.dedup_by(|a, b| a.path == b.path);
	found
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
	let Ok(entries) = std::fs::read_dir(dir) else {
		return;
	};
	for entry in entries.flatten() {
		let path = entry.path();
		if path.is_dir() {
			if depth > 0 {
				collect(&path, depth - 1, out);
			}
		} else {
			out.push(path);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_and_classifies() {
		let dir = tempfile::tempdir().unwrap();
		let game = dir.path().join("Game");
		let exe = game.join("Game/Binaries/Win64/Game.exe");
		for file in [
			"Game/Binaries/Win64/Game.exe",
			"_CommonRedist/vcredist/2019/VC_redist.x64.exe",
			"_CommonRedist/vcredist/2008/vcredist_x86.exe",
			"_CommonRedist/DirectX/Jun2010/DXSETUP.exe",
			"_CommonRedist/DirectX/Jun2010/dsetup32.dll",
			"_CommonRedist/DirectX/Jun2010/APR2007_d3dx9_33_x86.cab",
			"_CommonRedist/XNA/4.0/xnafx40_redist.msi",
			"Engine/Extras/Redist/en-us/UE4PrereqSetup_x64.exe",
			"Redist/readme.txt",
		] {
			let path = game.join(file);
			std::fs::create_dir_all(path.parent().unwrap()).unwrap();
			std::fs::write(&path, "").unwrap();
		}
		let found = find(&exe);
		let summary: Vec<_> = found
			.iter()
			.map(|r| (r.kind, r.detail.as_str(), r.key.as_str()))
			.collect();
		assert_eq!(
			summary,
			[
				(Kind::VisualCpp, "2008 x86", "2008/vcredist_x86.exe"),
				(Kind::VisualCpp, "2019 x64", "2019/vc_redist.x64.exe"),
				(Kind::DirectX, "2010", "jun2010/dxsetup.exe"),
				(Kind::Xna, "", "4.0/xnafx40_redist.msi"),
				(Kind::UnrealPrereqs, "x64", "en-us/ue4prereqsetup_x64.exe"),
			]
		);
		assert_eq!(found[0].command()[1..], ["/q"]);
		assert_eq!(
			found[1].command()[1..],
			["/install", "/quiet", "/norestart"]
		);
		assert_eq!(found[2].command()[1..], ["/silent"]);
		assert_eq!(found[3].command()[0], "msiexec");
		assert!(Redist::succeeded(Some(3010)));
		assert!(!Redist::succeeded(Some(1603)));
	}
}
