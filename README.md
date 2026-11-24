# OnlineFix Launcher

Linux launcher for Windows games with OnlineFix and FreeTP multiplayer fixes,
run through Proton. Written in Rust with [gpui](https://gpui.rs) and
[GPUI Kit](https://gpui-kit.com). The interface is in English and Russian.

Work in progress: games can be added from a folder, an archive or an exe,
get covers from SteamGridDB or Steam, and run through Proton. Proton
management and FreeTP installers are next.

## Requirements

- Steam, for the Steam Linux Runtime and for fixes that talk to the client.
- `7z` (the `7zip` package) to add games from archives; `unrar` also works
  for RAR.

## Building

Needs Rust 1.95+, `clang` and `mold`, plus the usual gpui dependencies
(fontconfig, freetype, wayland, xkbcommon, xcb, vulkan headers).

```sh
cargo run -p onlinefix-launcher
```

Settings live in `~/.config/onlinefix-launcher`, logs, prefixes and Proton
builds in `~/.local/share/onlinefix-launcher`.

## License

GPL-3.0-only.
