//! Wine's own programs and winetricks, run against a game's prefix.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::proton::ProtonBuild;

const WINETRICKS_URL: &str =
	"https://raw.githubusercontent.com/Winetricks/winetricks/master/src/winetricks";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WineTool {
	Winecfg,
	Regedit,
	Taskmgr,
	Control,
	Explorer,
	Console,
	Winetricks,
}

impl WineTool {
	pub const ALL: [WineTool; 7] = [
		Self::Winecfg,
		Self::Winetricks,
		Self::Regedit,
		Self::Taskmgr,
		Self::Control,
		Self::Explorer,
		Self::Console,
	];

	/// What `proton run` starts; winetricks runs on its own.
	fn program(self) -> &'static str {
		match self {
			Self::Winecfg => "winecfg",
			Self::Regedit => "regedit",
			Self::Taskmgr => "taskmgr",
			Self::Control => "control",
			Self::Explorer => "explorer",
			Self::Console => "wineconsole",
			Self::Winetricks => "winetricks",
		}
	}
}

/// What the tools need to find the prefix and Wine.
pub struct Target<'a> {
	pub proton: &'a ProtonBuild,
	/// The game's `STEAM_COMPAT_DATA_PATH`; Wine's prefix is `pfx` inside.
	pub prefix: &'a Path,
	pub steam_root: PathBuf,
}

fn proton_command(target: &Target, args: &[&str]) -> tokio::process::Command {
	let mut cmd = tokio::process::Command::new(target.proton.script());
	cmd.arg("run")
		.args(args)
		.env("STEAM_COMPAT_DATA_PATH", target.prefix)
		.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", &target.steam_root)
		.env("WINEDEBUG", "-all")
		.stdin(Stdio::null());
	cmd
}

pub async fn run(tool: WineTool, target: &Target<'_>, data_dir: &Path) -> Result<(), String> {
	std::fs::create_dir_all(target.prefix).map_err(|e| e.to_string())?;
	let status = match tool {
		WineTool::Winetricks => {
			// winetricks talks to the prefix directly, so Proton has to set it up first.
			if !target.prefix.join("pfx").is_dir() {
				proton_command(target, &["wineboot", "-u"])
					.status()
					.await
					.map_err(|e| e.to_string())?;
			}
			let script = winetricks(data_dir).await?;
			let wine = ["wine", "wine64"]
				.iter()
				.map(|w| target.proton.wine_bin(w))
				.find(|p| p.is_file())
				.unwrap_or_else(|| target.proton.wine_bin("wine"));
			tokio::process::Command::new(script)
				.env("WINEPREFIX", target.prefix.join("pfx"))
				.env("WINE", wine)
				.env("WINESERVER", target.proton.wine_bin("wineserver"))
				.env("WINEDEBUG", "-all")
				.stdin(Stdio::null())
				.status()
				.await
		}
		tool => proton_command(target, &[tool.program()]).status().await,
	}
	.map_err(|e| e.to_string())?;
	if status.success() {
		Ok(())
	} else {
		Err(format!("{}: {status}", tool.program()))
	}
}

/// The system's winetricks, or a copy downloaded once from its repository.
async fn winetricks(data_dir: &Path) -> Result<PathBuf, String> {
	if let Some(found) = std::env::var_os("PATH").and_then(|path| {
		std::env::split_paths(&path)
			.map(|dir| dir.join("winetricks"))
			.find(|p| p.is_file())
	}) {
		return Ok(found);
	}
	let script = data_dir.join("tools").join("winetricks");
	if script.is_file() {
		return Ok(script);
	}
	let client = crate::http::client()?;
	let text = client
		.get(WINETRICKS_URL)
		.send()
		.await
		.map_err(|e| e.to_string())?
		.error_for_status()
		.map_err(|e| e.to_string())?
		.text()
		.await
		.map_err(|e| e.to_string())?;
	if !text.starts_with("#!") {
		return Err("winetricks: unexpected download".into());
	}
	std::fs::create_dir_all(script.parent().unwrap_or(data_dir)).map_err(|e| e.to_string())?;
	std::fs::write(&script, text).map_err(|e| e.to_string())?;
	use std::os::unix::fs::PermissionsExt;
	std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
		.map_err(|e| e.to_string())?;
	Ok(script)
}
