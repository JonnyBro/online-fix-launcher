//! Changes some fixes need before they work under Proton. Each is safe to
//! run again: it does nothing when the change is already there.

use std::path::{Path, PathBuf};

use rust_i18n::t;

use crate::detect;
use crate::fix::FixInfo;
use crate::ini::Ini;

/// The official NuGet package of Newtonsoft.Json, for Photon launchers that
/// come without it. They're .NET Framework 4.x programs, hence `net45`.
const NEWTONSOFT_NUPKG: &str =
	"https://api.nuget.org/v3-flatcontainer/newtonsoft.json/13.0.3/newtonsoft.json.13.0.3.nupkg";
const NEWTONSOFT_ENTRY: &str = "lib/net45/Newtonsoft.Json.dll";
const NEWTONSOFT_DLL: &str = "Newtonsoft.Json.dll";

/// Fixes the INI files; returns a line per change made.
pub fn patch_ini(info: &FixInfo) -> Vec<String> {
	let mut done = Vec::new();
	for path in &info.ini_files {
		let Ok(text) = std::fs::read_to_string(path) else {
			continue;
		};
		let mut ini = Ini::parse(&text);
		let name = path
			.file_name()
			.map(|n| n.to_string_lossy().to_ascii_lowercase())
			.unwrap_or_default();
		// FreeTP's SteamFix reads the app id to pretend from `RealAppId`;
		// the game's own id is kept in a section of ours for covers.
		if name == "steamfix.ini"
			&& !ini.has_section("OnlineFix Linux")
			&& let (Some(real), Some(fake)) = (
				ini.get("Main", "RealAppId").map(str::to_owned),
				ini.get("Main", "FakeAppId").map(str::to_owned),
			) {
			ini.set("Main", "RealAppId", &fake);
			ini.set("OnlineFix Linux", "RealAppId", &real);
			done.push(t!("patch.freetp", file = path.display()).into_owned());
		}
		if ini
			.get("Misc", "ExtraProtection")
			.is_some_and(|v| !v.eq_ignore_ascii_case("false"))
		{
			ini.set("Misc", "ExtraProtection", "false");
			done.push(t!("patch.extra_protection", file = path.display()).into_owned());
		}
		let patched = ini.to_text();
		if patched != text
			&& let Err(e) = std::fs::write(path, patched)
		{
			tracing::warn!("could not patch {}: {e}", path.display());
		}
	}
	done
}

/// Photon launchers that lack Newtonsoft.Json get the game's own copy
/// linked in, or NuGet's. Returns a line per change made.
pub async fn patch_photon(
	info: &FixInfo,
	game_root: &Path,
	cache: &Path,
) -> Result<Vec<String>, String> {
	let mut done = Vec::new();
	for dir in &info.photon_dirs {
		let target = dir.join(NEWTONSOFT_DLL);
		if has_file_ci(dir, NEWTONSOFT_DLL) {
			continue;
		}
		match find_real_copy(game_root) {
			Some(copy) => {
				std::os::unix::fs::symlink(&copy, &target).map_err(|e| e.to_string())?;
				done.push(t!("patch.photon_linked", dir = dir.display()).into_owned());
			}
			None => {
				let dll = newtonsoft_from_nuget(cache).await?;
				std::fs::copy(&dll, &target).map_err(|e| e.to_string())?;
				done.push(t!("patch.photon_nuget", dir = dir.display()).into_owned());
			}
		}
	}
	Ok(done)
}

fn has_file_ci(dir: &Path, name: &str) -> bool {
	std::fs::read_dir(dir).is_ok_and(|entries| {
		entries.flatten().any(|e| {
			e.file_name()
				.to_str()
				.is_some_and(|n| n.eq_ignore_ascii_case(name))
		})
	})
}

/// A real file, not a link a previous patch made.
fn find_real_copy(root: &Path) -> Option<PathBuf> {
	detect::list_files(root, 8).into_iter().find(|f| {
		f.file_name()
			.and_then(|n| n.to_str())
			.is_some_and(|n| n.eq_ignore_ascii_case(NEWTONSOFT_DLL))
			&& std::fs::symlink_metadata(f).is_ok_and(|m| m.file_type().is_file())
	})
}

/// Downloads the package once and keeps the DLL in the cache.
async fn newtonsoft_from_nuget(cache: &Path) -> Result<PathBuf, String> {
	let dll = cache.join("nuget").join(NEWTONSOFT_DLL);
	if dll.is_file() {
		return Ok(dll);
	}
	let client = crate::http::client()?;
	let bytes = client
		.get(NEWTONSOFT_NUPKG)
		.send()
		.await
		.map_err(|e| e.to_string())?
		.error_for_status()
		.map_err(|e| e.to_string())?
		.bytes()
		.await
		.map_err(|e| e.to_string())?;
	let dll_bytes = extract_entry(&bytes, NEWTONSOFT_ENTRY)?;
	std::fs::create_dir_all(dll.parent().unwrap_or(cache)).map_err(|e| e.to_string())?;
	std::fs::write(&dll, dll_bytes).map_err(|e| e.to_string())?;
	Ok(dll)
}

fn extract_entry(zip_bytes: &[u8], entry: &str) -> Result<Vec<u8>, String> {
	use std::io::Read;
	let mut archive =
		zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| e.to_string())?;
	let mut file = archive.by_name(entry).map_err(|e| e.to_string())?;
	let mut out = Vec::new();
	file.read_to_end(&mut out).map_err(|e| e.to_string())?;
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fix;

	#[test]
	fn freetp_and_extra_protection() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("SteamFix64.dll"), "").unwrap();
		std::fs::write(
			dir.path().join("SteamFix.ini"),
			"[Main]\nRealAppId=777\nFakeAppId=480\n\n[Misc]\nExtraProtection=true\n",
		)
		.unwrap();
		let info = fix::scan(dir.path());
		assert_eq!(patch_ini(&info).len(), 2);
		let text = std::fs::read_to_string(dir.path().join("SteamFix.ini")).unwrap();
		assert_eq!(
			text,
			"[Main]\nRealAppId=480\nFakeAppId=480\n\n[Misc]\nExtraProtection=false\n\n[OnlineFix Linux]\nRealAppId=777\n"
		);
		// The game's id survives the patch, and a second run changes nothing.
		let again = fix::scan(dir.path());
		assert_eq!(again.real_app_id, Some(777));
		assert!(patch_ini(&again).is_empty());
	}

	#[tokio::test]
	async fn photon_links_the_games_copy() {
		let dir = tempfile::tempdir().unwrap();
		let launcher = dir.path().join("Launcher");
		std::fs::create_dir_all(&launcher).unwrap();
		std::fs::write(launcher.join("Launcher.exe"), "").unwrap();
		std::fs::write(launcher.join("OnlineFix.json"), "{}").unwrap();
		let managed = dir.path().join("Game_Data/Managed");
		std::fs::create_dir_all(&managed).unwrap();
		std::fs::write(managed.join("Newtonsoft.Json.dll"), "dll").unwrap();
		let info = fix::scan(dir.path());
		let done = patch_photon(&info, dir.path(), &dir.path().join("cache"))
			.await
			.unwrap();
		assert_eq!(done.len(), 1);
		let link = launcher.join("Newtonsoft.Json.dll");
		assert_eq!(
			std::fs::read_link(&link).unwrap(),
			managed.join("Newtonsoft.Json.dll")
		);
		assert!(
			patch_photon(
				&fix::scan(dir.path()),
				dir.path(),
				&dir.path().join("cache")
			)
			.await
			.unwrap()
			.is_empty()
		);
	}

	#[test]
	fn reads_zip_entries() {
		use std::io::Write;
		let mut buf = std::io::Cursor::new(Vec::new());
		{
			let mut zip = zip::ZipWriter::new(&mut buf);
			zip.start_file(NEWTONSOFT_ENTRY, zip::write::SimpleFileOptions::default())
				.unwrap();
			zip.write_all(b"MZ").unwrap();
			zip.finish().unwrap();
		}
		assert_eq!(
			extract_entry(buf.get_ref(), NEWTONSOFT_ENTRY).unwrap(),
			b"MZ"
		);
		assert!(extract_entry(buf.get_ref(), "missing").is_err());
	}
}
