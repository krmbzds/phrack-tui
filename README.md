# phrack-tui 📟

<p align="center">
  <img src="docs/demo.svg" alt="Animated demo of phrack-tui: the boot sequence, the first-run download, browsing and reading issues, following cross-references, bookmarks and snippets, and the amber, green and violet palettes" width="880">
</p>

A terminal reader for the Phrack archive. It knows how Phrack is put together, so it can jump straight
to an issue's Loopback or take a "see Phrack 49" and open issue 49. It also looks like an amber
phosphor terminal from 1985, because that's how this stuff should be read.

## Installation

Install it with cargo:

    $ cargo install phrack-tui

Or skip the compile and fetch a prebuilt binary for macOS or Linux:

    $ cargo binstall phrack-tui

The same binaries are on the [releases page](https://github.com/krmbzds/phrack-tui/releases).

Or build it from a checkout:

    $ cargo install --path .

Nothing is bundled. The first run asks `Download issues? Y/n` and fetches the archive from
archives.phrack.org. An overlay shows the transfer while issues fill in behind it, and `Esc` aborts
without losing what already arrived.

## Usage

Start it:

    $ phrack-tui

Press `F1` inside for every key.

### Reading

The reader behaves like a read-only vim buffer, counts included (`5j`, `12G`).

| keys | does |
|---|---|
| `h j k l` `w b e` `0 ^ $` | move |
| `gg` `G` `{n}G` `H M L` | top, bottom, line n, screen top/middle/bottom |
| `^d ^u ^f ^b` `^e ^y` `zz zt zb` | scroll |
| `{ }` | previous / next paragraph |
| `/ ? n N * #` | search (smart-case regex) |
| `v V` then `y`, or `yy` | yank to the clipboard |
| `m{a-z}` `'{a-z}` | marks inside a phile |
| `m{A-Z}` `'{A-Z}` | global marks, across issues |
| `^o` `Tab` | jump list, back and forward, across philes |

The mouse wheel scrolls and a click moves the cursor.

### Getting Around Phrack

| keys | does |
|---|---|
| `]]` `[[` | next / previous section heading |
| `gs` | outline of the phile's sections |
| `]a` `[a` | next / previous phile, rolling over into the next issue |
| `]i` `[i` | next / previous issue |
| `gi` or `q` | this issue's table of contents |
| `gI` `gl` `gn` `gp` `gw` `gx` | this issue's Intro, Loopback, Linenoise, Prophile, World News, Index |
| `f` | label every "Phrack NN" reference on screen, then press a letter to follow it |
| `Enter` `gf` `]r` `[r` | follow the reference under the cursor, or step between references |
| `gA` `ga` | author index, or everything else by this phile's author |
| `^p` | fuzzy finder over every phile in the archive |
| `g/` or `:grep` | grep the whole archive; Enter opens the phile at the hit |
| `gu` `gr` | next unread phile, or a random one |

### Bookmarks and Snippets

Use `gb` to bookmark a line. It gets a `◆` in the margin.

Select a passage with `v` or `V`, then press `s` to save it as a named snippet. Saved snippets stay
tinted in the text with a `▌` in the margin, and resting the cursor or the mouse on one shows its name.

Use `gB` and `gS` to browse them. Enter jumps back to the exact selection, `/` filters, `dd` deletes,
`r` renames, `e` adds a note and `y` yanks. Inside a phile, `]b [b` and `]s [s` step through them.

### Commands

`:` opens a command line with Tab completion for commands and their arguments.

| command | does |
|---|---|
| `:q` `:qa` `ZZ` | close the phile, or quit |
| `:42` `:42/3` | line 42 (issue 42 from the browser); issue 42, phile 3 |
| `:issue N` `:find` `:grep` `:author` | open an issue, the finder, grep, an author |
| `:download [refresh] [N]` | fetch what's missing, refetch everything, or one issue |
| `:theme amber\|crt\|dracula` | switch palette and save it to the config |
| `:set nu` `:set rnu` | line numbers |
| `:set nocrt` | turn the CRT effects off (or one at a time: `noscan`, `noglow`, `novignette`, `noflicker`) |
| `:noh` `:marks` `:delm` | clear the search highlight, list marks, delete marks |

## CLI (Command Line Interface)

Run `phrack-tui --help` to learn more:

```
phrack-tui: read Phrack on a phosphor screen

USAGE:
    phrack-tui [OPTIONS] [ISSUES_DIR]

OPTIONS:
        --no-boot        skip the modem boot sequence
        --theme NAME     amber (default), crt, dracula (this run only; `:theme` inside saves it)
        --no-crt         disable scanlines / glow / vignette / flicker
        --download [N]   download the archive (or just issue N) without the UI, then exit
        --refresh        with --download: re-fetch philes that are already on disk
        --paths          print the config, issues and state locations
        --dump WxH       render the first frame as plain text and exit (debugging)
        --demo-svg FILE  record the animated README demo to FILE and exit
    -h, --help           show this help
```

`--download` skips philes that are already on disk, so running it again only fetches what's new.

## Configuration

Files follow the XDG layout. `phrack-tui --paths` prints where they resolved to.

| | default location |
|---|---|
| config | `~/.config/phrack-tui/config.toml`, written on first run |
| issues | `~/.local/share/phrack-tui/issues/`, or `download_path` in the config |
| state | `~/.local/state/phrack-tui/`: read marks, marks, positions, bookmarks, snippets |

A directory argument or `$PHRACK_ISSUES` points it at an archive somewhere else.

The config is read at every start. `:theme` is the only thing that writes to it, and it only touches
the `theme` line:

```toml
archive_url = "https://archives.phrack.org"
# download_path = "~/Phrack"       # default: ~/.local/share/phrack-tui/issues
concurrency = 6                    # parallel downloads, 1-16
download_prompt = "missing"        # ask when the archive is empty; or "always", "never"
theme = "amber"                    # "amber", "crt" or "dracula"
boot = true

[crt]
scan = true
glow = true
vignette = true
flicker = true
```

## Anatomy of the Screen

The look is scanlines, a vignette that dims the edges like a curved tube, a faint bloom around the
glyphs, an occasional flicker, and a modem boot sequence that reports the actual settings.

There are three palettes. `amber` is the default, a P3 phosphor. `crt` is P1 green with Dracula
accents. `dracula` is the Dracula palette itself, with green as the text colour.

Philes written in CP437 or another pre-UTF-8 encoding are decoded, so the DOS-era box art comes out
the way it was drawn.

## Development

Run `cargo test` to run the tests.

`docs/demo.svg` is recorded from the app itself. Regenerate it with a downloaded archive:

    $ PHRACK_ISSUES=~/.local/share/phrack-tui/issues cargo run --release -- --demo-svg docs/demo.svg

## Credits

The downloader is a port of [phrack-issue-manager](https://github.com/hasanen/phrack-issue-manager)
(MIT) by hasanen. Its EPUB, PDF and merged-text exporters aren't included.

## Is it any good?

Yes.

## License

Copyright © 2026 [Kerem Bozdas][Personal Webpage]

This project is available under the terms of the [MIT License][License].

[License]: https://kerem.mit-license.org
[Personal Webpage]: https://kerembozdas.com
