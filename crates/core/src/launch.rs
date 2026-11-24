//! Turning a game into a command line: Proton, the Steam Linux Runtime,
//! DLL overrides and the rest of the environment, then running it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use rust_i18n::t;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::library::Game;
use crate::proton::ProtonBuild;

/// Proton puts DXVK and VKD3D into the prefix as native DLLs; this makes
/// Wine load them instead of its own Direct3D.
const DXVK_OVERRIDES: &str =
	"d3d11=n;d3d10=n;d3d10core=n;dxgi=n;openvr_api_dxvk=n;d3d12=n;d3d12core=n;d3d9=n;d3d8=n";

/// Steam's own test app; the overlay needs some id.
const SPACEWAR: u32 = 480;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LaunchError {
	#[error("{}", t!("err.exe_missing", path = .0.display()))]
	ExeMissing(PathBuf),
	#[error("{}", t!("err.proton_missing"))]
	ProtonMissing,
	#[error("{}", t!("err.bad_args", args = .0))]
	BadArgs(String),
}

/// What the plan needs besides the game.
#[derive(Debug, Clone)]
pub struct Context<'a> {
	pub proton: &'a ProtonBuild,
	pub prefix: PathBuf,
	/// `None` when Steam isn't installed; Proton still wants the variable.
	pub steam_root: Option<PathBuf>,
	/// The runtime's `run` script, when the game uses it and it's installed.
	pub runtime: Option<PathBuf>,
	/// Wine's debug channels; `None` silences Wine.
	pub wine_debug: Option<String>,
	/// `LD_PRELOAD` the launcher itself got, kept in front of the overlay.
	pub ld_preload: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
	pub program: String,
	pub args: Vec<String>,
	pub cwd: PathBuf,
	/// Set on top of the launcher's own environment.
	pub env: Vec<(String, String)>,
}

impl Plan {
	/// For the log: env assignments and the command, shell-quoted.
	pub fn display(&self) -> String {
		let env = self
			.env
			.iter()
			.map(|(k, v)| format!("{k}={}", shell_words::quote(v)));
		let cmd = std::iter::once(&self.program)
			.chain(&self.args)
			.map(|a| shell_words::quote(a).into_owned());
		env.chain(cmd).collect::<Vec<_>>().join(" ")
	}
}

pub fn plan(game: &Game, ctx: &Context) -> Result<Plan, LaunchError> {
	if !game.exe.is_file() {
		return Err(LaunchError::ExeMissing(game.exe.clone()));
	}
	if !ctx.proton.script().is_file() {
		return Err(LaunchError::ProtonMissing);
	}
	let words =
		|text: &str| shell_words::split(text).map_err(|_| LaunchError::BadArgs(text.to_owned()));

	let mut command: Vec<String> = words(&game.args_before)?;
	if let Some(run) = &ctx.runtime {
		command.push(path_str(run));
		command.push("--".into());
	}
	command.push(path_str(&ctx.proton.script()));
	command.push("run".into());
	command.push(path_str(&game.exe));
	command.extend(words(&game.args_after)?);

	let overrides = [
		(!game.wined3d).then_some(DXVK_OVERRIDES),
		Some(game.fix_overrides.as_str()),
		Some(game.extra_overrides.as_str()),
	]
	.into_iter()
	.flatten()
	.map(|s| s.trim_matches(';'))
	.filter(|s| !s.is_empty())
	.collect::<Vec<_>>()
	.join(";");

	let steam_root = ctx.steam_root.clone().unwrap_or_else(|| {
		std::env::var_os("HOME")
			.map(PathBuf::from)
			.unwrap_or_default()
			.join(".steam/steam")
	});
	let mut env = vec![
		("WINEDLLOVERRIDES".to_owned(), overrides),
		(
			"WINEDEBUG".to_owned(),
			ctx.wine_debug.clone().unwrap_or_else(|| "-all".into()),
		),
		("STEAM_COMPAT_DATA_PATH".to_owned(), path_str(&ctx.prefix)),
		(
			"STEAM_COMPAT_CLIENT_INSTALL_PATH".to_owned(),
			path_str(&steam_root),
		),
	];
	if game.wined3d {
		env.push(("PROTON_USE_WINED3D".into(), "1".into()));
	}
	if game.wayland {
		env.push(("PROTON_ENABLE_WAYLAND".into(), "1".into()));
	}
	if game.steam_overlay {
		let preload = ctx
			.ld_preload
			.iter()
			.filter(|p| !p.is_empty())
			.cloned()
			.chain(
				crate::steam::overlay_preload(&steam_root)
					.iter()
					.map(|p| path_str(p)),
			)
			.collect::<Vec<_>>()
			.join(":");
		env.push(("LD_PRELOAD".into(), preload));
		env.push(("ENABLE_VK_LAYER_VALVE_steam_overlay_1".into(), "1".into()));
		env.push((
			"SteamOverlayGameId".into(),
			game.fake_app_id.unwrap_or(SPACEWAR).to_string(),
		));
	}
	for (key, value) in &game.env {
		env.retain(|(k, _)| k != key);
		env.push((key.clone(), value.clone()));
	}

	let program = command.remove(0);
	Ok(Plan {
		program,
		args: command,
		cwd: game.work_dir().to_path_buf(),
		env,
	})
}

fn path_str(path: &Path) -> String {
	path.to_string_lossy().into_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
	Stdout,
	Stderr,
}

#[derive(Debug)]
pub struct Outcome {
	pub code: Option<i32>,
	pub ran_for: Duration,
}

/// Runs the plan to the end, handing every output line to `on_line`.
pub async fn run(
	plan: &Plan,
	on_line: impl Fn(Stream, String) + Send + Sync + 'static,
) -> std::io::Result<Outcome> {
	std::fs::create_dir_all(
		plan.env
			.iter()
			.find(|(k, _)| k == "STEAM_COMPAT_DATA_PATH")
			.map(|(_, v)| PathBuf::from(v))
			.unwrap_or_default(),
	)?;
	let started = Instant::now();
	let mut child = tokio::process::Command::new(&plan.program)
		.args(&plan.args)
		.current_dir(&plan.cwd)
		.envs(plan.env.iter().map(|(k, v)| (k, v)))
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()?;
	let on_line = std::sync::Arc::new(on_line);
	let mut readers = Vec::new();
	if let Some(out) = child.stdout.take() {
		let on_line = on_line.clone();
		readers.push(tokio::spawn(async move {
			let mut lines = BufReader::new(out).lines();
			while let Ok(Some(line)) = lines.next_line().await {
				on_line(Stream::Stdout, line);
			}
		}));
	}
	if let Some(err) = child.stderr.take() {
		let on_line = on_line.clone();
		readers.push(tokio::spawn(async move {
			let mut lines = BufReader::new(err).lines();
			while let Ok(Some(line)) = lines.next_line().await {
				on_line(Stream::Stderr, line);
			}
		}));
	}
	let status = child.wait().await?;
	for reader in readers {
		let _ = reader.await;
	}
	Ok(Outcome {
		code: status.code(),
		ran_for: started.elapsed(),
	})
}

/// Ends every Wine process of the prefix, the game included.
pub async fn stop(proton: &ProtonBuild, prefix: &Path) -> std::io::Result<()> {
	let status = tokio::process::Command::new(proton.wine_bin("wineserver"))
		.arg("-k")
		.env("WINEPREFIX", prefix.join("pfx"))
		.status()
		.await?;
	if status.success() {
		Ok(())
	} else {
		Err(std::io::Error::other(format!("wineserver -k: {status}")))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::proton::Source;

	fn setup() -> (tempfile::TempDir, Game, ProtonBuild) {
		let dir = tempfile::tempdir().unwrap();
		let game_dir = dir.path().join("My Game");
		std::fs::create_dir_all(&game_dir).unwrap();
		std::fs::write(game_dir.join("Game.exe"), "").unwrap();
		let proton_dir = dir.path().join("GE-Proton10-1");
		std::fs::create_dir_all(&proton_dir).unwrap();
		std::fs::write(proton_dir.join("proton"), "").unwrap();
		let proton = ProtonBuild::read(&proton_dir, Source::Launcher).unwrap();
		let mut game = Game::new("My Game", game_dir.join("Game.exe"));
		game.fix_overrides = "onlinefix64=n;winmm=n,b".into();
		(dir, game, proton)
	}

	fn env<'a>(plan: &'a Plan, key: &str) -> Option<&'a str> {
		plan.env
			.iter()
			.find(|(k, _)| k == key)
			.map(|(_, v)| v.as_str())
	}

	#[test]
	fn plain_launch() {
		let (dir, mut game, proton) = setup();
		game.steam_runtime = false;
		game.args_after = "-windowed --name \"Big Bob\"".into();
		let ctx = Context {
			proton: &proton,
			prefix: dir.path().join("pfx"),
			steam_root: Some("/steam".into()),
			runtime: None,
			wine_debug: None,
			ld_preload: None,
		};
		let plan = plan(&game, &ctx).unwrap();
		assert_eq!(plan.program, path_str(&proton.script()));
		assert_eq!(
			plan.args,
			[
				"run",
				&path_str(&game.exe),
				"-windowed",
				"--name",
				"Big Bob"
			]
		);
		assert_eq!(plan.cwd, dir.path().join("My Game"));
		assert_eq!(
			env(&plan, "WINEDLLOVERRIDES"),
			Some(&*format!("{DXVK_OVERRIDES};onlinefix64=n;winmm=n,b"))
		);
		assert_eq!(env(&plan, "WINEDEBUG"), Some("-all"));
		assert_eq!(
			env(&plan, "STEAM_COMPAT_CLIENT_INSTALL_PATH"),
			Some("/steam")
		);
		assert_eq!(env(&plan, "LD_PRELOAD"), None);
		assert_eq!(env(&plan, "PROTON_USE_WINED3D"), None);
	}

	#[test]
	fn runtime_overlay_and_user_env() {
		let (dir, mut game, proton) = setup();
		game.args_before = "gamemoderun".into();
		game.wined3d = true;
		game.wayland = true;
		game.steam_overlay = true;
		game.fake_app_id = Some(1234);
		game.extra_overrides = "dinput8=n,b".into();
		game.env.insert("WINEDEBUG".into(), "+seh".into());
		let ctx = Context {
			proton: &proton,
			prefix: dir.path().join("pfx"),
			steam_root: Some("/steam".into()),
			runtime: Some("/slr/run".into()),
			wine_debug: None,
			ld_preload: Some("/usr/lib/libmangohud.so".into()),
		};
		let plan = plan(&game, &ctx).unwrap();
		assert_eq!(plan.program, "gamemoderun");
		assert_eq!(
			&plan.args[..3],
			["/slr/run", "--", &path_str(&proton.script())]
		);
		assert_eq!(
			env(&plan, "WINEDLLOVERRIDES"),
			Some("onlinefix64=n;winmm=n,b;dinput8=n,b")
		);
		assert_eq!(env(&plan, "WINEDEBUG"), Some("+seh"));
		assert_eq!(env(&plan, "PROTON_USE_WINED3D"), Some("1"));
		assert_eq!(env(&plan, "PROTON_ENABLE_WAYLAND"), Some("1"));
		assert_eq!(
			env(&plan, "LD_PRELOAD"),
			Some(
				"/usr/lib/libmangohud.so:/steam/ubuntu12_32/gameoverlayrenderer.so:/steam/ubuntu12_64/gameoverlayrenderer.so"
			)
		);
		assert_eq!(env(&plan, "SteamOverlayGameId"), Some("1234"));
	}

	#[test]
	fn errors() {
		let (dir, mut game, proton) = setup();
		let ctx = Context {
			proton: &proton,
			prefix: dir.path().join("pfx"),
			steam_root: None,
			runtime: None,
			wine_debug: None,
			ld_preload: None,
		};
		game.args_after = "\"unclosed".into();
		assert!(matches!(plan(&game, &ctx), Err(LaunchError::BadArgs(_))));
		game.exe = dir.path().join("missing.exe");
		assert!(matches!(plan(&game, &ctx), Err(LaunchError::ExeMissing(_))));
	}
}
