# OnlineFix Launcher

Linux launcher for Windows games with OnlineFix and FreeTP multiplayer fixes,
run through Proton. Written in Rust with [gpui](https://gpui.rs) and
[GPUI Kit](https://gpui-kit.com). The interface is in English and Russian.

Work in progress: the window, settings and logs are there; the game library,
fix detection and Proton management are next.

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
