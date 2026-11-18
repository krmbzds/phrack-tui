# phrack-tui

<p align="center">
  <img src="docs/demo.svg" alt="phrack-tui demo: boot sequence, browsing issues, vim-style reading, Tab-completed commands, cross-reference hints, fuzzy finder, whole-archive grep, bookmarks and snippets, across amber, green and violet palettes" width="880">
</p>


A green-phosphor CRT terminal reader for the Phrack archive, with vim keys and Phrack-aware navigation.
It downloads the archive itself on first run: no separate tool, no checked-in copy.

    cargo run --release            # first run offers: "Download issues? Y/n"
    cargo install --path .         # then just: phrack-tui

Options: `--no-boot`, `--theme amber|crt|dracula` (this run only; `:theme NAME` inside saves it to the config), `--no-crt`,
`--download [N] [--refresh]` (headless download), `--paths`. Press **F1** inside for the full key reference.

## First run, files and config

After the boot sequence an empty archive gets a centred `Download issues? Y/n` (Enter, `y` or `Y` accepts). A live overlay then
shows the transfer while issues and philes populate the screen behind it, and hands you the main screen when it is done.
`Esc` aborts; whatever arrived is kept. Run `:download` any time to top up (`:download refresh`, `:download 25`).
Existing philes are skipped, so it resumes and only fetches what is new.

| what | where (XDG) |
|---|---|
| config | `$XDG_CONFIG_HOME/phrack-tui/config.toml` (default `~/.config/...`), created on first run |
| issues | `$XDG_DATA_HOME/phrack-tui/issues/` (default `~/.local/share/...`), or `download_path` in the config |
| state  | `$XDG_STATE_HOME/phrack-tui/state` and `state.collection` (read marks, marks, bookmarks, snippets) |

`phrack-tui --paths` prints the resolved locations; `$PHRACK_ISSUES` or a directory argument overrides the issues location.

```toml
archive_url = "https://archives.phrack.org"
# download_path = "~/Phrack"       # default: the XDG data dir above
concurrency = 6                    # parallel downloads, 1-16
download_prompt = "missing"        # "missing" | "always" | "never"
theme = "amber"                    # "amber" | "crt" | "dracula"; `:theme NAME` rewrites just this line
boot = true
[crt]
scan = true
glow = true
vignette = true
flicker = true
```

The downloader is a port of [phrack-issue-manager](https://github.com/hasanen/phrack-issue-manager) (MIT) by hasanen: same archive,
same layout. It is reimplemented on blocking `ureq` and threads, resumes by skipping what is on disk, writes atomically, and keeps the
server's bytes untouched. The original's EPUB/PDF/merged-text exporters and PDF downloads are not included.

- **Look**: scanlines, tube vignette, glyph bloom, rare flicker, rounded bezel, a 1985 modem boot screen.
  Themes: `amber` (default, P3 phosphor), `crt` (P1 green, accents from the nvim dracula palette), `dracula` (bg, accents and comment colours from the nvim `colorscheme.lua`, with its green as text colour; the cursor-line, tooltip and snippet tints are my own picks).
  Toggle effects with `:set noscan noglow novignette noflicker`, or `:set nocrt`.
- **Vim**: `hjkl w b e 0 ^ $ gg G {n}G H M L ^d ^u ^f ^b zz zt zb { } / ? n N * # v V y m' ` and `:` commands.
- **Phrack tricks**: `]]`/`[[` sections, `gs` outline, `]a`/`[a` next/prev phile (rolls across issues), `]i`/`[i` issues,
  `gl gn gp gw gI gx` jump to this issue's Loopback / Linenoise / Prophile / World News / Intro / Index,
  `f` hint-follow `Phrack NN` cross-references (also `Enter`, `gf`, `]r`), `gA`/`ga` authors, `^p` fuzzy finder over all 1057 philes,
  `g/` grep the whole archive, `gu` next unread, `gr` random, global marks `mA`/`'A`, `^o`/`Tab` jump list across philes.
- **Bookmarks & snippets**: `gb` bookmarks the line (`◆` in the margin), `v`/`V` then `s` saves the selection as a named snippet.
  Snippets stay tinted in the text (teal, deliberately unlike the green selection, yellow search and cursor line) with a `▌` margin bar;
  put the cursor or the mouse over one for a tooltip. `gB` / `gS` open the collection browser (preview, filter, jump back to the exact
  selection, `dd` delete, `r` rename, `e` note, `y` yank); `]b [b ]s [s` step through them. Stored in `state.collection`
  next to the state file, and re-anchored by text if a phile changes.
- Legacy CP437 / non-UTF-8 philes are decoded; read marks, marks and positions persist under `$XDG_STATE_HOME/phrack-tui/`.

## Regenerating the demo

`docs/demo.svg` is not hand-drawn. It is recorded from the real UI: a scripted key sequence (`src/demo.rs`) is fed to the app, every
frame is rendered by the actual renderer, and the result is written as one SMIL-animated SVG (CRT vignette, scanlines and bloom are SVG
overlays). It starts from an empty archive, replays a local copy of the archive through the downloader's own events (no network) and uses a throw-away in-memory profile, so it never touches your config or state. Point `PHRACK_ISSUES` at a downloaded archive first.

    PHRACK_ISSUES=~/.local/share/phrack-tui/issues cargo run --release -- --demo-svg docs/demo.svg
    PHRACK_DEMO_VERBOSE=1 ...   # print when each caption starts
    PHRACK_DEMO_STILLS=6000,20000 ...   # also write static previews of those instants (ms)

