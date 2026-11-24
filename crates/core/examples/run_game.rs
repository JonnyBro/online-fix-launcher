//! Runs a game from the command line the way the launcher would.
//!
//! cargo run -p onlinefix-launcher-core --example run_game -- <Game.exe> [--dry-run] [--no-runtime]

use std::path::PathBuf;
use std::time::Duration;

use onlinefix_launcher_core::launch::{self, Context, Stream};
use onlinefix_launcher_core::library::Game;
use onlinefix_launcher_core::{AppPaths, detect, fix, proton, steam};

#[tokio::main]
async fn main() {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let Some(exe) = args
		.iter()
		.find(|a| !a.starts_with("--"))
		.map(PathBuf::from)
	else {
		eprintln!("usage: run_game <Game.exe> [--dry-run] [--no-runtime]");
		std::process::exit(2);
	};
	let dry_run = args.iter().any(|a| a == "--dry-run");
	let exe = exe.canonicalize().expect("game exe");
	let paths = AppPaths::detect().unwrap();

	let game_dir = exe.parent().unwrap();
	let info = fix::scan(game_dir);
	println!("fix: {info:#?}");
	let mut game = Game::new(detect::game_name(&exe), &exe);
	game.id = format!("cli-{}", game.name.replace(['/', ' '], "_"));
	game.fix_overrides = info.overrides_string();
	game.real_app_id = info.real_app_id;
	game.fake_app_id = info.fake_app_id;
	game.steam_runtime = !args.iter().any(|a| a == "--no-runtime");

	let root = steam::root();
	let libraries = root
		.as_deref()
		.map(steam::library_folders)
		.unwrap_or_default();
	let builds = proton::find_installed(&paths.protons_dir(), root.as_deref(), &libraries);
	for b in &builds {
		println!("proton: {} ({:?}) {}", b.name, b.source, b.dir.display());
	}
	let proton = proton::choose(&builds, None, None).expect("no Proton build found");
	let runtime = match (game.steam_runtime, proton.runtime_app_id) {
		(true, Some(id)) => {
			let run = steam::runtime(&libraries, id);
			if run.is_none() {
				eprintln!("Steam Linux Runtime {id} isn't installed: steam://install/{id}");
			}
			run
		}
		_ => None,
	};
	let ctx = Context {
		proton,
		prefix: game.prefix_dir(&paths),
		steam_root: root.clone(),
		runtime,
		wine_debug: None,
		ld_preload: std::env::var("LD_PRELOAD").ok(),
	};
	let plan = launch::plan(&game, &ctx).unwrap_or_else(|e| panic!("{e}"));
	println!("\n{}\n", plan.display());
	if dry_run {
		return;
	}

	if let Some(root) = &root
		&& !steam::is_running()
	{
		println!("starting Steam…");
		if !steam::start_and_wait(root, Duration::from_secs(420)).await {
			eprintln!("Steam didn't sign in");
			std::process::exit(1);
		}
	}
	let outcome = launch::run(&plan, |stream, line| match stream {
		Stream::Stdout => println!("{line}"),
		Stream::Stderr => eprintln!("{line}"),
	})
	.await
	.expect("run");
	println!("exit {:?} after {:?}", outcome.code, outcome.ran_for);
}
