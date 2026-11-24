//! Game archives (RAR, 7z, ZIP) through the system's `7z` or `unrar`.
//! OnlineFix releases are RAR with encrypted headers and a known password.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use rust_i18n::t;
use tokio::io::AsyncReadExt;

/// What OnlineFix puts on its archives.
pub const ONLINEFIX_PASSWORD: &str = "online-fix.me";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
	SevenZip,
	Unrar,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ArchiveError {
	#[error("{}", t!("err.no_archiver"))]
	NoTool,
	#[error("{}", t!("err.archive_password"))]
	Password,
	#[error("{}", t!("err.archive", error = .0))]
	Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
	/// Relative, with `/` separators.
	pub path: PathBuf,
	pub dir: bool,
}

pub fn is_archive(path: &Path) -> bool {
	path.file_name()
		.and_then(|n| n.to_str())
		.map(str::to_ascii_lowercase)
		.is_some_and(|n| [".rar", ".7z", ".zip"].iter().any(|ext| n.ends_with(ext)))
}

/// The first volume of a multi-part RAR, which the tools want; other archives as they are.
pub fn first_volume(path: &Path) -> PathBuf {
	let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
		return path.to_path_buf();
	};
	let lower = name.to_ascii_lowercase();
	let Some(at) = lower.rfind(".part") else {
		return path.to_path_buf();
	};
	let digits = &lower[at + 5..lower.len().saturating_sub(4)];
	if !lower.ends_with(".rar") || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit())
	{
		return path.to_path_buf();
	}
	let stem = &name[..at];
	let first = format!("{stem}.part{:0width$}.rar", 1, width = digits.len());
	let candidate = path.with_file_name(first);
	if candidate.is_file() {
		candidate
	} else {
		path.to_path_buf()
	}
}

/// `7z` handles every format, `unrar` only RAR.
pub fn find_tool(archive: &Path) -> Option<(Tool, PathBuf)> {
	let rar = archive
		.extension()
		.and_then(|e| e.to_str())
		.is_some_and(|e| e.eq_ignore_ascii_case("rar"));
	which("7z")
		.or_else(|| which("7zz"))
		.map(|p| (Tool::SevenZip, p))
		.or_else(|| {
			rar.then(|| which("unrar"))
				.flatten()
				.map(|p| (Tool::Unrar, p))
		})
}

fn which(name: &str) -> Option<PathBuf> {
	std::env::var_os("PATH").and_then(|path| {
		std::env::split_paths(&path)
			.map(|dir| dir.join(name))
			.find(|p| p.is_file())
	})
}

/// Without a password 7z waits for one on stdin; plain archives ignore it,
/// so there is always one.
fn password_arg(tool: Tool, password: &str) -> String {
	match tool {
		Tool::SevenZip | Tool::Unrar => format!("-p{password}"),
	}
}

pub async fn list(archive: &Path, password: &str) -> Result<Vec<Entry>, ArchiveError> {
	let (tool, program) = find_tool(archive).ok_or(ArchiveError::NoTool)?;
	let mut cmd = tokio::process::Command::new(program);
	match tool {
		Tool::SevenZip => cmd.args(["l", "-ba", "-slt"]),
		Tool::Unrar => cmd.args(["lb"]),
	};
	let output = cmd
		.arg(password_arg(tool, password))
		.arg("--")
		.arg(archive)
		.stdin(Stdio::null())
		.output()
		.await
		.map_err(|e| ArchiveError::Failed(e.to_string()))?;
	let stdout = String::from_utf8_lossy(&output.stdout);
	let stderr = String::from_utf8_lossy(&output.stderr);
	if wrong_password(&stdout) || wrong_password(&stderr) {
		return Err(ArchiveError::Password);
	}
	if !output.status.success() {
		return Err(ArchiveError::Failed(last_line(&stderr, &stdout)));
	}
	Ok(match tool {
		Tool::SevenZip => parse_7z_listing(&stdout),
		Tool::Unrar => parse_bare_listing(&stdout),
	})
}

/// Unpacks everything into `dest`, reporting 0..=1 as it goes.
pub async fn extract(
	archive: &Path,
	dest: &Path,
	password: &str,
	progress: impl Fn(f32),
) -> Result<(), ArchiveError> {
	let (tool, program) = find_tool(archive).ok_or(ArchiveError::NoTool)?;
	std::fs::create_dir_all(dest).map_err(|e| ArchiveError::Failed(e.to_string()))?;
	let mut cmd = tokio::process::Command::new(program);
	match tool {
		Tool::SevenZip => {
			let mut out = std::ffi::OsString::from("-o");
			out.push(dest);
			cmd.args(["x", "-y", "-bsp1", "-bso0"]).arg(out);
		}
		Tool::Unrar => {
			cmd.args(["x", "-y", "-idc"]);
		}
	}
	cmd.arg(password_arg(tool, password)).arg("--").arg(archive);
	if tool == Tool::Unrar {
		// A trailing slash tells unrar it's the destination folder.
		let mut dir = dest.as_os_str().to_owned();
		dir.push("/");
		cmd.arg(dir);
	}
	let mut child = cmd
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|e| ArchiveError::Failed(e.to_string()))?;
	let mut stderr = child.stderr.take();
	let errors = tokio::spawn(async move {
		let mut text = String::new();
		if let Some(err) = &mut stderr {
			let _ = err.read_to_string(&mut text).await;
		}
		text
	});
	// Both tools redraw the percentage in place with `\r` or backspaces.
	let mut seen = String::new();
	if let Some(mut out) = child.stdout.take() {
		let mut buf = [0u8; 4096];
		while let Ok(n) = out.read(&mut buf).await {
			if n == 0 {
				break;
			}
			seen.push_str(&String::from_utf8_lossy(&buf[..n]));
			if let Some(p) = last_percent(&seen) {
				progress(p);
			}
			if seen.len() > 8192 {
				seen.drain(..seen.len() - 1024);
			}
		}
	}
	let status = child
		.wait()
		.await
		.map_err(|e| ArchiveError::Failed(e.to_string()))?;
	let stderr = errors.await.unwrap_or_default();
	if wrong_password(&stderr) || wrong_password(&seen) {
		return Err(ArchiveError::Password);
	}
	if !status.success() {
		return Err(ArchiveError::Failed(last_line(&stderr, &seen)));
	}
	progress(1.);
	Ok(())
}

fn wrong_password(text: &str) -> bool {
	text.contains("Wrong password") || text.contains("Incorrect password")
}

fn last_line(stderr: &str, stdout: &str) -> String {
	[stderr, stdout]
		.iter()
		.flat_map(|t| t.lines().rev())
		.map(str::trim)
		.find(|l| !l.is_empty())
		.unwrap_or("?")
		.to_owned()
}

fn last_percent(text: &str) -> Option<f32> {
	let end = text.rfind('%')?;
	let digits: String = text[..end]
		.chars()
		.rev()
		.skip_while(|c| *c == ' ')
		.take_while(char::is_ascii_digit)
		.collect::<Vec<_>>()
		.into_iter()
		.rev()
		.collect();
	let percent: f32 = digits.parse().ok()?;
	Some((percent / 100.).clamp(0., 1.))
}

fn parse_7z_listing(text: &str) -> Vec<Entry> {
	let mut entries = Vec::new();
	let mut path: Option<String> = None;
	let mut dir = false;
	let mut flush = |path: &mut Option<String>, dir: &mut bool| {
		if let Some(p) = path.take() {
			entries.push(Entry {
				path: PathBuf::from(p.replace('\\', "/")),
				dir: *dir,
			});
		}
		*dir = false;
	};
	for line in text.lines() {
		if let Some(p) = line.strip_prefix("Path = ") {
			flush(&mut path, &mut dir);
			path = Some(p.to_owned());
		} else if line == "Folder = +" {
			dir = true;
		} else if let Some(attrs) = line.strip_prefix("Attributes = ") {
			dir |= attrs.starts_with('D');
		}
	}
	flush(&mut path, &mut dir);
	entries
}

/// `unrar lb` prints names only; folders are the names others start with.
fn parse_bare_listing(text: &str) -> Vec<Entry> {
	let names: Vec<String> = text
		.lines()
		.map(|l| l.trim().replace('\\', "/"))
		.filter(|l| !l.is_empty())
		.collect();
	names
		.iter()
		.map(|name| Entry {
			path: PathBuf::from(name),
			dir: names
				.iter()
				.any(|other| other.len() > name.len() && other.starts_with(&format!("{name}/"))),
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_listings() {
		let slt = "Path = Game\nSize = 0\nAttributes = D_ drwxr-xr-x\n\nPath = Game/Game.exe\nSize = 2\nAttributes = A_ -rw-r--r--\n";
		assert_eq!(
			parse_7z_listing(slt),
			[
				Entry {
					path: "Game".into(),
					dir: true
				},
				Entry {
					path: "Game/Game.exe".into(),
					dir: false
				}
			]
		);
		let bare = "Game\nGame\\Bin\nGame\\Bin\\Game.exe\n";
		let entries = parse_bare_listing(bare);
		assert!(entries[0].dir && entries[1].dir && !entries[2].dir);
		assert_eq!(entries[2].path, PathBuf::from("Game/Bin/Game.exe"));
	}

	#[test]
	fn reads_progress() {
		assert_eq!(
			last_percent("  3% 1 - a\u{8}\u{8}\u{8} 45% 12 - b"),
			Some(0.45)
		);
		assert_eq!(last_percent("Scan"), None);
	}

	#[test]
	fn volumes() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("Game.part01.rar"), "").unwrap();
		std::fs::write(dir.path().join("Game.part02.rar"), "").unwrap();
		assert_eq!(
			first_volume(&dir.path().join("Game.part02.rar")),
			dir.path().join("Game.part01.rar")
		);
		assert_eq!(
			first_volume(Path::new("/x/Game.rar")),
			Path::new("/x/Game.rar")
		);
		assert!(is_archive(Path::new("A.RAR")) && !is_archive(Path::new("a.exe")));
	}

	/// Needs `7z` on PATH; skipped otherwise.
	#[tokio::test]
	async fn real_7z_roundtrip() {
		if which("7z").is_none() {
			return;
		}
		let dir = tempfile::tempdir().unwrap();
		let src = dir.path().join("src/Game");
		std::fs::create_dir_all(&src).unwrap();
		std::fs::write(src.join("Game.exe"), "x").unwrap();
		let archive = dir.path().join("enc.7z");
		let made = std::process::Command::new("7z")
			.args(["a", "-pP", "-mhe=on"])
			.arg(&archive)
			.arg("Game")
			.current_dir(dir.path().join("src"))
			.stdout(Stdio::null())
			.status()
			.unwrap();
		assert!(made.success());
		assert_eq!(list(&archive, "wrong").await, Err(ArchiveError::Password));
		let entries = list(&archive, "P").await.unwrap();
		assert!(
			entries
				.iter()
				.any(|e| e.path == Path::new("Game/Game.exe") && !e.dir)
		);
		let out = dir.path().join("out");
		extract(&archive, &out, "P", |_| {}).await.unwrap();
		assert!(out.join("Game/Game.exe").is_file());
	}
}
