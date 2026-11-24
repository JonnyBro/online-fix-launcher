//! Proton builds on this computer: the ones the launcher downloaded, custom
//! builds in Steam's `compatibilitytools.d`, and Valve's from Steam libraries.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::steam::{self, LibraryFolder};
use crate::vdf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
	Launcher,
	/// `compatibilitytools.d`, like ProtonUp-Qt installs.
	CompatTools,
	/// Valve's Proton, installed as a Steam app.
	Steam,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtonBuild {
	pub name: String,
	/// The folder with the `proton` script.
	pub dir: PathBuf,
	pub source: Source,
	/// The Steam Linux Runtime it's built for, from `toolmanifest.vdf`.
	pub runtime_app_id: Option<u32>,
	modified: Option<SystemTime>,
}

impl ProtonBuild {
	pub fn script(&self) -> PathBuf {
		self.dir.join("proton")
	}

	/// Wine's own programs (`wineserver`, `wine`) inside the build.
	pub fn wine_bin(&self, program: &str) -> PathBuf {
		self.dir.join("files/bin").join(program)
	}

	/// Reads one folder; `None` unless it has a `proton` script.
	pub fn read(dir: &Path, source: Source) -> Option<Self> {
		let script = dir.join("proton");
		if !script.is_file() {
			return None;
		}
		let name = std::fs::read_to_string(dir.join("version"))
			.ok()
			// `1795427215 GE-Proton11-14`: a build time, then the name.
			.and_then(|v| v.split_whitespace().nth(1).map(str::to_owned))
			.filter(|_| source != Source::Steam)
			.or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))?;
		let runtime_app_id = std::fs::read_to_string(dir.join("toolmanifest.vdf"))
			.ok()
			.and_then(|text| vdf::parse(&text))
			.and_then(|m| m.obj("manifest")?.str("require_tool_appid")?.parse().ok());
		Some(Self {
			name,
			dir: dir.to_path_buf(),
			source,
			runtime_app_id: runtime_app_id.or_else(|| guess_runtime(dir)),
			modified: std::fs::metadata(&script).and_then(|m| m.modified()).ok(),
		})
	}
}

/// For builds without a tool manifest, by GE-Proton's major version.
fn guess_runtime(dir: &Path) -> Option<u32> {
	let name = dir.file_name()?.to_str()?;
	let major: u32 = name
		.strip_prefix("GE-Proton")?
		.split(['-', '.'])
		.next()?
		.parse()
		.ok()?;
	Some(match major {
		11.. => steam::RUNTIME_4,
		8..=10 => steam::RUNTIME_SNIPER,
		_ => steam::RUNTIME_SOLDIER,
	})
}

/// Newest first, by when the build was installed.
pub fn find_installed(
	launcher_dir: &Path,
	steam_root: Option<&Path>,
	libraries: &[LibraryFolder],
) -> Vec<ProtonBuild> {
	let mut places: Vec<(PathBuf, Source)> = vec![(launcher_dir.to_path_buf(), Source::Launcher)];
	if let Some(root) = steam_root {
		places.push((root.join("compatibilitytools.d"), Source::CompatTools));
	}
	places.push((
		PathBuf::from("/usr/share/steam/compatibilitytools.d"),
		Source::CompatTools,
	));
	for lib in libraries {
		places.push((lib.path.join("steamapps/common"), Source::Steam));
	}

	let mut builds: Vec<ProtonBuild> = Vec::new();
	let mut seen = Vec::new();
	for (place, source) in places {
		let Ok(entries) = std::fs::read_dir(&place) else {
			continue;
		};
		for entry in entries.flatten() {
			let dir = entry.path();
			if source == Source::Steam && !entry.file_name().to_string_lossy().starts_with("Proton")
			{
				continue;
			}
			let canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
			if seen.contains(&canonical) {
				continue;
			}
			if let Some(build) = ProtonBuild::read(&dir, source) {
				seen.push(canonical);
				builds.push(build);
			}
		}
	}
	builds.sort_by(|a, b| {
		b.modified
			.cmp(&a.modified)
			.then_with(|| a.name.cmp(&b.name))
	});
	builds
}

/// The build a game runs with: its own pick, else the default, else the newest.
pub fn choose<'a>(
	builds: &'a [ProtonBuild],
	game: Option<&Path>,
	default: Option<&Path>,
) -> Option<&'a ProtonBuild> {
	let by_dir = |dir: &Path| builds.iter().find(|b| b.dir == dir);
	match (game, default) {
		(Some(dir), _) => by_dir(dir),
		(None, Some(dir)) => by_dir(dir).or_else(|| builds.first()),
		(None, None) => builds.first(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fake_build(dir: &Path, manifest_runtime: Option<u32>, version: Option<&str>) {
		std::fs::create_dir_all(dir).unwrap();
		std::fs::write(dir.join("proton"), "#!/usr/bin/env python3\n").unwrap();
		if let Some(id) = manifest_runtime {
			std::fs::write(
				dir.join("toolmanifest.vdf"),
				format!("\"manifest\"\n{{\n  \"require_tool_appid\" \"{id}\"\n}}\n"),
			)
			.unwrap();
		}
		if let Some(v) = version {
			std::fs::write(dir.join("version"), v).unwrap();
		}
	}

	#[test]
	fn finds_builds_in_every_place() {
		let dir = tempfile::tempdir().unwrap();
		let launcher = dir.path().join("protons");
		let root = dir.path().join("Steam");
		fake_build(
			&launcher.join("GE-Proton11-14-x86_64"),
			Some(4_183_110),
			Some("1795427215 GE-Proton11-14\n"),
		);
		fake_build(&root.join("compatibilitytools.d/GE-Proton9-20"), None, None);
		fake_build(
			&root.join("steamapps/common/Proton - Experimental"),
			Some(1_628_350),
			Some("1700000000 proton-9.0\n"),
		);
		std::fs::create_dir_all(root.join("steamapps/common/SomeGame")).unwrap();
		std::fs::create_dir_all(root.join("compatibilitytools.d/broken")).unwrap();

		let libraries = [LibraryFolder {
			path: root.clone(),
			apps: Vec::new(),
		}];
		let builds = find_installed(&launcher, Some(&root), &libraries);
		let mut names: Vec<_> = builds
			.iter()
			.map(|b| (b.name.as_str(), b.source, b.runtime_app_id))
			.collect();
		names.sort_by_key(|n| n.0);
		assert_eq!(
			names,
			[
				("GE-Proton11-14", Source::Launcher, Some(steam::RUNTIME_4)),
				(
					"GE-Proton9-20",
					Source::CompatTools,
					Some(steam::RUNTIME_SNIPER)
				),
				(
					"Proton - Experimental",
					Source::Steam,
					Some(steam::RUNTIME_SNIPER)
				),
			]
		);

		let ge9 = root.join("compatibilitytools.d/GE-Proton9-20");
		assert_eq!(
			choose(&builds, Some(&ge9), None).unwrap().name,
			"GE-Proton9-20"
		);
		assert_eq!(
			choose(&builds, None, Some(&ge9)).unwrap().name,
			"GE-Proton9-20"
		);
		assert!(choose(&builds, Some(Path::new("/gone")), None).is_none());
		assert_eq!(
			choose(&builds, None, Some(Path::new("/gone"))),
			builds.first()
		);
	}
}
