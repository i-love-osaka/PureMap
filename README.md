# PureMap

PureMap is a native Rust mind-mapping application built with `eframe`/`egui`.
It works offline and stores maps as readable `.puremap` JSON files. The app
provides a radial canvas, keyboard-driven editing, and automatic layout.

## Features

- Radial mind-map canvas with curved connections, zoom, pan, optional grid, and branch colors
- Add child, sibling, or parent nodes; edit, duplicate, delete, collapse, and expand branches
- Automatically arrange nodes, or drag branches to customize their positions
- Undo and redo (up to 200 actions)
- Search highlighting and branch focus mode
- Open/save `.puremap` and JSON files; import/export Markdown outlines
- Open `.puremap` files by double-clicking them in Finder (macOS app bundle)
- Light, dark, and automatic themes

## Run

Requires Rust 1.85 or later and the native GUI development libraries for your
operating system.

```sh
cargo run
```

Run tests and check formatting with:

```sh
cargo fmt --check
cargo test --locked
```

## Build a macOS app

On macOS 12 or later, create a local `PureMap.app` bundle with:

```sh
sh packaging/macos/build-app.sh
```

The script builds the release binary, assembles the app bundle with its icon,
registers it with Launch Services for `.puremap` files, and applies an ad-hoc
signature. The app is not notarized. On macOS, the menu bar is provided by
AppKit and appears at the top of the screen, outside the app window. Chinese
text uses a macOS system font.

## Shortcuts

| Action | Shortcut |
| --- | --- |
| Add child / sibling after selected / parent | `Tab` / `Enter` / `Shift+Tab` |
| Edit / delete | `F2` / `Delete` |
| Undo / redo | `Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z` |
| Duplicate branch | `Cmd/Ctrl+D` |
| Save / open | `Cmd/Ctrl+S` / `Cmd/Ctrl+O` |
| Search / focus mode | `Cmd/Ctrl+F` / `Cmd/Ctrl+K` |
| Center / toggle collapse | `Cmd/Ctrl+.` / `Cmd/Ctrl+/` |
| Zoom in / out / reset | `Cmd/Ctrl+=` / `Cmd/Ctrl+-` / `Cmd/Ctrl+0` |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for local checks and contribution
guidelines. Pull requests are checked by GitHub Actions.

## License

PureMap is licensed under the [MIT License](LICENSE).
