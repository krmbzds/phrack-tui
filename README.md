# phrack-tui

A green-phosphor CRT terminal reader for the Phrack archive in `issues/`, with vim keys and Phrack-aware navigation.

    cargo run --release            # from the repo root (uses ./issues)
    cargo install --path .         # then: phrack-tui [ISSUES_DIR]

Options: `--no-boot`, `--theme amber|crt|dracula` (this session only; `:theme NAME` inside saves it), `--no-crt`. Press **F1** inside for the full key reference.

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
- Legacy CP437 / non-UTF-8 philes are decoded; read marks, marks and positions persist in `~/.local/state/phrack-tui/state`.
