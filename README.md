# Nori

A native torrent finder: search public trackers in parallel, inspect the
results, and hand the magnet link to an embedded BitTorrent engine. The
download queue keeps running while the window is open, and finished files stay
writable.

Nori is written in Rust on [GPUI](https://github.com/zed-industries/zed), with
no browser, no account, and nothing in the middle.

## Building

Requirements:

- Rust 2024 edition toolchain
- A Wayland or X11 session, plus the usual GPUI build dependencies
  (`libwayland-dev`, `libxkbcommon-dev`, `libfontconfig-dev`, `libssl-dev`)

```sh
cargo build --release
./target/release/nori
```

The binary ships with Inter and JetBrains Mono Nerd Font, so no system font
setup is needed.

## Installing

```sh
./packaging/install.sh
```

This builds the release binary and installs `nori`, the desktop entry, and the
icon under your home directory.

## Using it

Type a query and press `Enter`. Results stream in per source; the status row
under the toolbar shows which sources are still working and which failed. Use
the category tabs or `f` to filter, and `s` to cycle the sort order.

Click a result to see its details, or tick its box to choose it for the batch.
The box at the top of the list chooses every visible result; `Ctrl-A` does the
same from the keyboard. With two or more chosen, the details pane shows each
torrent as a collapsible card, and `d` opens a choice sheet: start the whole
batch now, or queue it to run one torrent at a time in the order shown. A
queued batch resumes where it left off after a restart.

Shortcuts are scoped by section. When a text field has focus the letters go to
the field, so `Escape` returns to the list first.

| Anywhere | |
| --- | --- |
| `Ctrl-K` or `/` | Focus the search field |
| `Ctrl-R` | Search again |
| `Ctrl-V` | Paste a magnet link |
| `Ctrl-,` | Settings |
| `Escape` | Close a dialog, clear the filter, or clear the selection |
| `Enter` | Search from the field, otherwise open the selected row |

| Browse | |
| --- | --- |
| `Click` | Open a result's details |
| `Checkbox` | Choose a result for the batch |
| `Ctrl-A` | Choose every visible result |
| `d` | Download the selection (two or more opens the batch choice) |
| `y` | Copy the selected magnet links |
| `s` | Cycle sort |
| `f` | Toggle the filter field |
| `z` | Toggle hiding torrents with no seeders |
| `↑` / `↓` | Move the selection |

| Downloads | |
| --- | --- |
| `p` | Pause or resume |
| `c` | Remove from the list (files are kept) |
| `e` | Open the folder |

| Seeding | |
| --- | --- |
| `p` | Pause or resume seeding |
| `c` | Remove from the list (files are kept) |
| `e` | Open the folder |

## State

| Path | Contents |
| --- | --- |
| `~/.config/nori-torrent/config.json` | Download folder, limits, sources, trackers, theme |
| `~/.local/share/nori-torrent/queue.json` | Download queue and seeding state |
| `~/Downloads/nori` | Default download folder |

The `nori-torrent` suffix keeps this app's state apart from other desktop apps
that use the Nori name.

## Sources

FitGirl, YTS, The Pirate Bay (movies and TV), 1337x (movies and TV), EZTV, Nyaa,
SubsPlease, and BitTorrented. Each can be switched off in Settings.

Some upstreams rate-limit or block unauthenticated clients; a failing source is
reported in the status row and does not stop the others.

## Tests

```sh
cargo test
```
