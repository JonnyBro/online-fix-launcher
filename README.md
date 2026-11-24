# OnlineFix Launcher

[![CI](https://github.com/JonnyBro/online-fix-launcher/actions/workflows/ci.yml/badge.svg)](https://github.com/JonnyBro/online-fix-launcher/actions/workflows/ci.yml)

Linux launcher for Windows games with OnlineFix and FreeTP multiplayer fixes,
run through Proton. Written in Rust with [gpui](https://gpui.rs) and
[GPUI Kit](https://gpui-kit.com). The interface is in English and Russian.

## Features

- Library: a grid of covers with search and play time. From a game's page
  you start or stop it and open its last log, its folder and its prefix.
- Adding games from a folder, an archive (RAR, 7z, ZIP, multi-part RAR) or an
  exe, also by dropping it on the window. When the program that starts the
  game is obvious (the EOS auth launcher, a Photon launcher) it's picked for
  you; otherwise you choose from a list. OnlineFix archives open with their
  known password, and the launcher asks for any other.
- Fix detection: the DLL overrides a fix needs (`WINEDLLOVERRIDES`) and the
  Steam app ids from its INI. Before each run the launcher patches what some
  fixes need under Proton: the FreeTP app id swap, `ExtraProtection=false`,
  and Newtonsoft.Json for Photon launchers (linked from the game, or taken
  from NuGet).
- FreeTP installers run silently through Proton into a folder of your choice.
- Proton: builds the launcher downloaded, custom ones in
  `compatibilitytools.d` and Valve's from Steam. GE-Proton and Proton-CachyOS
  (the x86-64-v3 build when the CPU supports it) download from the Proton tab,
  checked against their SHA-512.
- The Steam Linux Runtime each Proton build asks for in its manifest. If
  Steam isn't running, the launcher starts it in the tray before a game.
- Per-game settings: Proton build, prefix, arguments, a wrapper like
  `gamemoderun`, extra DLL overrides, environment variables, Steam overlay,
  Steam Linux Runtime, native Wayland, WineD3D.
- Prefix tools: winecfg, winetricks, registry editor, task manager, control
  panel, file explorer, command prompt.
- Covers and banners come from Steam. With a SteamGridDB API key, SteamGridDB
  fills in what Steam lacks and offers a gallery; your own pictures work too.
  Without a cover, the grid shows the exe's icon.
- A checkup of what the launcher needs from the system, desktop
  notifications, color schemes and interface scale.

## Requirements

- Steam from your distribution's packages (not Flatpak), for the Steam Linux
  Runtime and for fixes that talk to the client.
- `7z` (the `7zip` package) to add games from archives; `unrar` also works
  for RAR.
- `winetricks` is used when installed; otherwise the launcher downloads the
  script from the Winetricks repository the first time it's needed.

To see what's missing, press Check everything in the General section of the
settings.

## Installing

Download `onlinefix-launcher-<version>-linux-x86_64.tar.gz` from the
[releases](https://github.com/JonnyBro/online-fix-launcher/releases), unpack
it and run `onlinefix-launcher`. For the application menu, copy
`onlinefix-launcher.desktop` to `~/.local/share/applications/` and
`onlinefix-launcher.svg` to `~/.local/share/icons/hicolor/scalable/apps/`,
with the binary somewhere on your `PATH`.

## Where things are

- `~/.config/onlinefix-launcher`: settings and the game list.
- `~/.local/share/onlinefix-launcher`: Wine prefixes, downloaded Proton
  builds, covers and logs (the launcher's own and each game's last run).
- `~/.cache/onlinefix-launcher`: SteamGridDB previews and downloads.

## Building

Needs Rust 1.95+, `clang` and `mold`, plus the usual gpui dependencies
(fontconfig, freetype, wayland, xkbcommon, xcb, vulkan headers).

```sh
cargo run -p onlinefix-launcher
```

The core crate has examples that work without the window:

```sh
cargo run -p onlinefix-launcher-core --example run_game -- /path/to/Game.exe --dry-run
cargo run -p onlinefix-launcher-core --example proton_list
```

## Thanks

To [ZzEdovec/onlinefix-linux](https://github.com/ZzEdovec/onlinefix-linux):
its launcher worked out how to run these fixes under Proton. This one shares
no code with it.

## License

GPL-3.0-only.
