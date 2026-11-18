//! Drawing. Everything renders into a [`Buffer`]; the CRT pass runs afterwards.

use unicode_width::UnicodeWidthChar;

use crate::{
    app::{App, CmdCtx, CmdLine, Overlay, Pane},
    collview::{CollView, Tab},
    buffer::{Buffer, Style},
    crt,
    doc::Heading,
    help,
    picker::Picker,
    reader::VMode,
};

pub fn render(app: &App, buf: &mut Buffer) {
    let th = &app.theme;
    buf.clear(Style::new(th.fg, th.bg));
    if let Some(b) = &app.boot {
        draw_boot(app, b.visible(), buf);
    } else if buf.w < 44 || buf.h < 12 {
        let s = Style::new(th.warn, th.bg).bold();
        buf.put(1, 1, "TERMINAL TOO SMALL", s, buf.w as usize);
        buf.put(1, 2, "need at least 44x12", Style::new(th.dim, th.bg), buf.w as usize);
    } else {
        draw_frame(app, buf);
        if app.reader.is_some() {
            draw_reader(app, buf);
        } else {
            draw_browser(app, buf);
        }
        draw_status(app, buf);
        draw_cmdline(app, buf);
        match &app.overlay {
            Overlay::None => {}
            Overlay::Help(s) => draw_help(app, *s, buf),
            Overlay::Picker(p) => draw_picker(app, p, buf),
            Overlay::Coll(v) => draw_coll(app, v, buf),
        }
    }
    crt::apply(buf, th, &app.fx, app.bright);
}

fn trunc(s: &str, w: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw > w.saturating_sub(if s.chars().count() > w { 1 } else { 0 }) && s.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>() > w {
            out.push('…');
            return out;
        }
        out.push(c);
        used += cw;
    }
    out
}

fn draw_boot(app: &App, lines: Vec<String>, buf: &mut Buffer) {
    let th = &app.theme;
    let st = Style::new(th.fg, th.bg);
    let n = lines.len();
    for (i, l) in lines.iter().enumerate() {
        let y = 2 + i as u16;
        let s = if i == 0 { st.fg(th.bright).bold() } else { st };
        buf.put(3, y, l, s, buf.w as usize);
        if i + 1 == n && app.blink {
            let x = 3 + l.chars().count() as u16;
            buf.set(x, y, '█', Style::new(th.bright, th.bg));
        }
    }
}

fn draw_frame(app: &App, buf: &mut Buffer) {
    let th = &app.theme;
    let (w, h) = (buf.w, buf.h);
    let bs = Style::new(th.border, th.bg);
    let bottom = h - 3;
    buf.set(0, 0, '╭', bs);
    buf.set(w - 1, 0, '╮', bs);
    buf.set(0, bottom, '╰', bs);
    buf.set(w - 1, bottom, '╯', bs);
    for x in 1..w - 1 {
        buf.set(x, 0, '─', bs);
        buf.set(x, bottom, '─', bs);
    }
    for y in 1..bottom {
        buf.set(0, y, '│', bs);
        buf.set(w - 1, y, '│', bs);
    }
    let title = " PHRACK INC. ";
    buf.set(2, 0, '┤', bs);
    let used = buf.put(3, 0, title, Style::new(th.bright, th.bg).bold(), w as usize);
    buf.set(3 + used as u16, 0, '├', bs);
    let total = app.lib.total_articles();
    let read = app.st.read.len();
    let right = format!(" READ {read}/{total} ");
    let rx = w.saturating_sub(right.chars().count() as u16 + 3);
    buf.set(rx - 1, 0, '┤', bs);
    buf.put(rx, 0, &right, Style::new(th.dim, th.bg), w as usize);
    buf.set(rx + right.chars().count() as u16, 0, '├', bs);
}

// ---------------------------------------------------------------- browser

fn draw_browser(app: &App, buf: &mut Buffer) {
    let th = &app.theme;
    let (x0, y0) = (1u16, 1u16);
    let cw = buf.w - 2;
    let ch = buf.h - 4;
    let lw = ((cw as usize * 2 / 5).clamp(22, 34)) as u16;
    let b = &app.browser;
    let focus_i = b.pane == Pane::Issues;

    // divider
    for y in 0..ch {
        buf.set(x0 + lw, y0 + y, '│', Style::new(th.faint, th.bg));
    }

    // -- issues
    let head = |focus: bool| if focus { Style::new(th.bright, th.bg).bold() } else { Style::new(th.dim, th.bg) };
    buf.put(x0 + 1, y0, if focus_i { "▶ ISSUE  PHILES  PROGRESS" } else { "  ISSUE  PHILES  PROGRESS" }, head(focus_i), lw as usize - 1);
    let lh = ch as usize - 1;
    let n = app.lib.issues.len();
    let top = b.issue.saturating_sub(lh / 2).min(n.saturating_sub(lh));
    for i in 0..lh.min(n) {
        let idx = top + i;
        let iss = &app.lib.issues[idx];
        let y = y0 + 1 + i as u16;
        let read = iss.articles.iter().filter(|a| app.st.read.contains(&(a.issue, a.num))).count();
        let tot = iss.articles.len();
        let sel = idx == b.issue;
        let bar_n = 8usize;
        let filled = if read == 0 { 0 } else { (read * bar_n).div_ceil(tot).min(bar_n) };
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_n - filled);
        let row = format!("{}{:>3}    {:>3}    ", if sel { "▶" } else { " " }, iss.num, tot);
        let (bg, fg) = if sel { (if focus_i { th.select } else { th.cursorline }, th.bright) } else { (th.bg, th.fg) };
        let mut st = Style::new(fg, bg);
        if sel && focus_i {
            st = st.bold();
        }
        buf.fill(x0, y, lw, 1, ' ', Style::new(fg, bg));
        buf.put(x0, y, &row, st, lw as usize);
        let bar_style = Style::new(if read == tot { th.warn } else if sel { th.fg } else { th.dim }, bg);
        buf.put(x0 + row.chars().count() as u16, y, &bar, bar_style, bar_n);
    }

    // -- articles of selected issue
    let rx = x0 + lw + 1;
    let rw = (cw - lw - 1) as usize;
    let iss = &app.lib.issues[b.issue];
    let hdr = format!(
        "{}PHRACK {}  ·  {} PHILES",
        if focus_i { "  " } else { "▶ " },
        iss.num,
        iss.articles.len()
    );
    buf.put(rx, y0, &hdr, head(!focus_i), rw);
    let n = iss.articles.len();
    let top = b.art.saturating_sub(lh / 2).min(n.saturating_sub(lh));
    let aw = if rw >= 64 { (rw / 3).min(26) } else { 0 };
    let tw = rw.saturating_sub(10 + aw + 6);
    for i in 0..lh.min(n) {
        let idx = top + i;
        let a = &iss.articles[idx];
        let y = y0 + 1 + i as u16;
        let sel = idx == b.art;
        let read = app.st.read.contains(&(a.issue, a.num));
        let (bg, fg) = if sel { (if !focus_i { th.select } else { th.cursorline }, th.bright) } else { (th.bg, th.fg) };
        let mut st = Style::new(fg, bg);
        if sel && !focus_i {
            st = st.bold();
        }
        buf.fill(rx, y, rw as u16, 1, ' ', Style::new(fg, bg));
        let mark = if read { "[x]" } else { "[ ]" };
        let prefix = format!("{}{:>2} ", if sel { "▶" } else { " " }, a.num);
        let mut x = rx;
        x += buf.put(x, y, &prefix, st, rw) as u16;
        x += buf.put(x, y, mark, Style::new(if read { th.dim } else { th.warn }, bg), 3) as u16;
        x += 1;
        buf.put(x, y, &trunc(&a.title, tw), if read && !sel { st.fg(th.dim) } else { st }, tw);
        if aw > 0 {
            let ax = rx + (rw - 6 - aw) as u16;
            buf.put(ax, y, &trunc(&a.author, aw - 1), Style::new(if sel { th.fg } else { th.dim }, bg), aw - 1);
        }
        let size = if a.size >= 1024 { format!("{}K", a.size / 1024) } else { format!("{}B", a.size) };
        let sx = rx + rw as u16 - size.chars().count() as u16 - 1;
        buf.put(sx, y, &size, Style::new(th.faint.mix(th.dim, 0.5), bg), 6);
    }
}

// ----------------------------------------------------------------- reader

fn heading_at(hs: &[Heading], row: usize) -> bool {
    hs.binary_search_by_key(&row, |h| h.row).is_ok()
}

fn draw_reader(app: &App, buf: &mut Buffer) {
    let th = &app.theme;
    let r = app.reader.as_ref().unwrap();
    let (x0, y0) = (1u16, 1u16);
    let cw = (buf.w - 2) as usize;
    let ch = buf.h as usize - 4;
    let (gutter, avail) = app.geometry();
    let page_x = if r.width < avail { (avail - r.width) / 2 } else { 0 };
    let tx = x0 as usize + gutter + page_x;
    let right = x0 as usize + gutter + avail; // exclusive
    let in_overlay = !matches!(app.overlay, Overlay::None) || app.cmd.as_ref().is_some_and(|c| c.prefix == ':');
    let show_cursor = app.blink && !in_overlay;

    for i in 0..ch {
        let row = r.top + i;
        let y = y0 + i as u16;
        if row >= r.len() {
            buf.put(x0, y, "~", Style::new(th.faint, th.bg), 1);
            continue;
        }
        let is_cur = row == r.row;
        let line_bg = if is_cur && r.visual.is_none() { th.cursorline } else { th.bg };
        if is_cur {
            buf.fill(x0, y, (gutter + avail) as u16, 1, ' ', Style::new(th.fg, line_bg));
        }
        let row_snips: Vec<_> = r.snips.iter().filter(|s| s.touches_row(row)).collect();
        if let Some(_id) = r.bookmark_on(row) {
            buf.set(x0, y, '◆', Style::new(th.mark, line_bg).bold());
        }
        if !row_snips.is_empty() {
            buf.set(x0 + 1, y, '▌', Style::new(th.link, line_bg));
        }
        if gutter > 2 {
            let num = if app.rnu && !is_cur { row.abs_diff(r.row) } else { row + 1 };
            let s = format!("{:>w$} ", num, w = gutter - 3);
            let st = if is_cur { Style::new(th.bright, line_bg).bold() } else { Style::new(th.faint.mix(th.dim, 0.4), th.bg) };
            buf.put(x0 + 2, y, &s, st, gutter - 2);
        }
        let line = &r.doc.lines[row];
        let is_head = heading_at(&r.doc.headings, row);
        let matches = if r.hl { r.line_matches(row) } else { Vec::new() };
        let refs: Vec<_> = r.doc.refs_on(row).collect();
        let base = if is_head { Style::new(th.heading, line_bg).bold() } else { Style::new(th.fg, line_bg) };

        let mut dc = 0usize;
        for (ci, &c) in line.iter().enumerate() {
            let w = c.width().unwrap_or(0);
            if w == 0 {
                continue;
            }
            let here = dc;
            dc += w;
            if here < r.hx {
                continue;
            }
            let sx = tx + here - r.hx;
            if sx + w > right {
                break;
            }
            let mut st = base;
            if refs.iter().any(|x| ci >= x.col && ci < x.col + x.len) {
                st = st.fg(th.link).ul();
            }
            if row_snips.iter().any(|s| s.contains(row, ci)) {
                st = st.bg(th.snippet).fg(th.fg.mix(th.link, 0.35));
            }
            if let Some(m) = matches.iter().find(|m| ci >= m.0 && ci < m.0 + m.1) {
                let cur = is_cur && r.col >= m.0 && r.col < m.0 + m.1;
                st = if cur { Style::new(th.cur_fg, th.cur_bg).bold() } else { Style::new(th.search_fg, th.search_bg) };
            }
            if r.in_selection(row, ci) {
                st = st.bg(th.select).fg(th.bright);
            }
            buf.set(sx as u16, y, c, st);
            if w == 2 {
                buf.set(sx as u16 + 1, y, '\0', st);
            }
        }
        // visual-line selection extends past the text
        if let Some((VMode::Line, _)) = r.visual {
            if r.in_selection(row, 0) {
                let from = (tx + r.disp_col(row, line.len())).saturating_sub(r.hx).max(x0 as usize + gutter);
                if from < right {
                    buf.fill(from as u16, y, (right - from) as u16, 1, ' ', Style::new(th.fg, th.select));
                }
            }
        }
        // line-wise snippets tint the whole row, like V-LINE does for a selection
        if row_snips.iter().any(|s| s.line) {
            let from = (tx + r.disp_col(row, line.len())).saturating_sub(r.hx).max(x0 as usize + gutter);
            if from < right {
                buf.fill(from as u16, y, (right - from) as u16, 1, ' ', Style::new(th.fg, th.snippet));
            }
        }
        if is_cur && show_cursor {
            let dcur = r.disp_col(row, r.col);
            if dcur >= r.hx {
                let sx = tx + dcur - r.hx;
                if sx < right {
                    let c = line.get(r.col).copied().unwrap_or(' ');
                    let under = buf.get(sx as u16, y).st;
                    buf.set(sx as u16, y, c, Style::new(th.bg, th.bright).bold().fg(th.bg));
                    let _ = under;
                }
            }
        }
    }

    // hint labels
    if let Some(hints) = &r.hints {
        for (rf, label) in hints {
            if rf.row < r.top || rf.row >= r.top + ch {
                continue;
            }
            let dcol = r.disp_col(rf.row, rf.col);
            if dcol < r.hx {
                continue;
            }
            let sx = tx + dcol - r.hx;
            if sx < right {
                buf.set(sx as u16, y0 + (rf.row - r.top) as u16, label.to_ascii_uppercase(), Style::new(th.cur_fg, th.mark).bold());
            }
        }
    }

    draw_tooltip(app, r, buf, tx, right, ch);

    // scrollbar
    let n = r.len();
    if n > ch && cw > 2 {
        let sx = x0 + cw as u16 - 1;
        let thumb = (ch * ch / n).max(1);
        let pos = r.top * (ch - thumb) / (n - ch).max(1);
        for i in 0..ch {
            let (c, col) = if i >= pos && i < pos + thumb { ('█', th.dim) } else { ('│', th.faint) };
            buf.set(sx, y0 + i as u16, c, Style::new(col, th.bg));
        }
    }
}

// ----------------------------------------------------------- status & cmd

fn draw_wildmenu(app: &App, buf: &mut Buffer, items: &[String], idx: Option<usize>) {
    let th = &app.theme;
    let y = buf.h - 2;
    let w = buf.w as usize;
    let base = Style::new(th.status_fg, th.status_bg);
    buf.fill(0, y, buf.w, 1, ' ', base);
    let cur = idx.unwrap_or(0);
    // scroll so the selected entry is visible
    let widths: Vec<usize> = items.iter().map(|s| s.chars().count() + 2).collect();
    let mut first = 0;
    while first < cur && widths[first..=cur].iter().sum::<usize>() + 2 > w {
        first += 1;
    }
    let mut x = 0usize;
    if first > 0 {
        x += buf.put(0, y, "< ", Style::new(th.dim, th.status_bg), 2);
    }
    for (i, it) in items.iter().enumerate().skip(first) {
        if x + widths[i] > w.saturating_sub(2) && i != cur {
            buf.put((w - 1) as u16, y, ">", Style::new(th.dim, th.status_bg), 1);
            break;
        }
        let st = if Some(i) == idx { Style::new(th.status_bg, th.bright).bold() } else { base };
        x += buf.put(x as u16, y, &format!(" {it} "), st, w - x);
        x += 1;
    }
}

fn draw_status(app: &App, buf: &mut Buffer) {
    let th = &app.theme;
    if let Some(comp) = app.cmd.as_ref().and_then(|c| c.comp.as_ref()) {
        if comp.items.len() > 1 {
            return draw_wildmenu(app, buf, &comp.items, comp.idx);
        }
    }
    let y = buf.h - 2;
    let w = buf.w as usize;
    let base = Style::new(th.status_fg, th.status_bg);
    buf.fill(0, y, buf.w, 1, ' ', base);
    let pill = Style::new(th.status_bg, th.bright).bold();

    let (mode, left, right) = if let Some(r) = &app.reader {
        let mode = if r.hints.is_some() {
            " HINT "
        } else {
            match r.visual {
                Some((VMode::Char, _)) => " VISUAL ",
                Some((VMode::Line, _)) => " V-LINE ",
                None => " NORMAL ",
            }
        };
        let a = app.lib.article(r.issue, r.art);
        let title = a.map(|a| a.title.as_str()).unwrap_or("");
        let total = app.lib.issue(r.issue).map_or(0, |i| i.articles.len());
        let mut left = format!(" PHRACK {} ▸ {}/{} ▸ {}", r.issue, r.art, total, title);
        if let Some(h) = r.current_heading() {
            left.push_str(&format!("  ▸ §{}", h.title));
        }
        let author = a.map(|a| a.author.as_str()).unwrap_or("");
        let vh = app.view_h();
        let right = format!("{}  {}  {}:{} ", author, r.percent(vh), r.row + 1, r.col + 1);
        (mode, left, right)
    } else {
        let i = &app.lib.issues[app.browser.issue];
        let left = match app.browser.pane {
            Pane::Issues => format!(" PHRACK {}  ({} philes)", i.num, i.articles.len()),
            Pane::Articles => {
                let a = &i.articles[app.browser.art];
                format!(" PHRACK {} ▸ {}/{} ▸ {}", i.num, a.num, i.articles.len(), a.title)
            }
        };
        (" BROWSE ", left, "F1 help  ^p find  gA authors  gr random ".to_string())
    };
    let mut x = buf.put(0, y, mode, pill, w) as u16;
    x += 0;
    let rw = right.chars().count();
    let room = w.saturating_sub(x as usize + rw + 1);
    buf.put(x, y, &trunc(&left, room), base, room);
    if w > rw {
        buf.put((w - rw) as u16, y, &right, Style::new(th.status_fg, th.status_bg).fg(th.dim.mix(th.status_fg, 0.5)), rw);
    }
}

fn draw_cmdline(app: &App, buf: &mut Buffer) {
    let th = &app.theme;
    let y = buf.h - 1;
    let w = buf.w as usize;
    let st = Style::new(th.fg, th.bg);
    // pending keys / count at far right
    let show: String = format!("{}{}", app.count.map(|c| c.to_string()).unwrap_or_default(), app.pending);
    if !show.is_empty() {
        buf.put((w - show.chars().count() - 1) as u16, y, &show, Style::new(th.warn, th.bg).bold(), w);
    }
    if let Some(CmdLine { prefix, text, ctx, .. }) = &app.cmd {
        let label = match (prefix, ctx) {
            ('S', _) => "snippet name> ",
            ('R', _) => "rename> ",
            ('E', CmdCtx::BmNote(_)) => "bookmark note> ",
            ('E', _) => "snippet note> ",
            (':', _) => ":",
            ('/', _) => "/",
            _ => "?",
        };
        let s = format!("{label}{text}");
        let used = buf.put(0, y, &s, st.fg(th.bright), w - 1);
        if app.blink {
            buf.set(used as u16, y, '█', Style::new(th.bright, th.bg));
        }
    } else if let Some(m) = &app.msg {
        let s = Style::new(if m.err { th.err } else { th.dim.mix(th.fg, 0.6) }, th.bg);
        buf.put(0, y, &trunc(&m.text, w.saturating_sub(10)), if m.err { s.bold() } else { s }, w - 1);
    }
}

// --------------------------------------------------------------- overlays

fn boxed(app: &App, buf: &mut Buffer, bw: u16, bh: u16, title: &str, footer: &str) -> (u16, u16, u16, u16) {
    let th = &app.theme;
    let panel = th.bg_center;
    let bx = (buf.w.saturating_sub(bw)) / 2;
    let by = (buf.h.saturating_sub(bh)) / 2;
    let bs = Style::new(th.bright, panel);
    buf.fill(bx, by, bw, bh, ' ', Style::new(th.fg, panel));
    for x in 1..bw - 1 {
        buf.set(bx + x, by, '═', bs);
        buf.set(bx + x, by + bh - 1, '═', bs);
    }
    for y in 1..bh - 1 {
        buf.set(bx, by + y, '║', bs);
        buf.set(bx + bw - 1, by + y, '║', bs);
    }
    buf.set(bx, by, '╔', bs);
    buf.set(bx + bw - 1, by, '╗', bs);
    buf.set(bx, by + bh - 1, '╚', bs);
    buf.set(bx + bw - 1, by + bh - 1, '╝', bs);
    let t = format!("╡ {title} ╞");
    buf.put(bx + 2, by, &trunc(&t, bw as usize - 4), bs.bold(), bw as usize - 4);
    if !footer.is_empty() {
        let f = format!("╡ {footer} ╞");
        let fw = f.chars().count().min(bw as usize - 4);
        buf.put(bx + bw - 2 - fw as u16, by + bh - 1, &trunc(&f, fw), Style::new(th.dim, panel), fw);
    }
    (bx + 2, by + 1, bw - 4, bh - 2)
}

fn draw_picker(app: &App, p: &Picker, buf: &mut Buffer) {
    let th = &app.theme;
    let panel = th.bg_center;
    let bw = (buf.w - 4).min(100);
    let bh = (buf.h - 3).min(30);
    let counter = format!("{}/{}", if p.view.is_empty() { 0 } else { p.sel + 1 }, p.view.len());
    let title = if p.searching { format!("{}  searching…  {counter}", p.title) } else { format!("{}  {counter}", p.title) };
    let footer = if p.filtering { "type to filter  ⏎ open  ^n/^p move  esc stop" } else { "j/k move  / filter  ⏎ open  q close" };
    let (ix, iy, iw, ih) = boxed(app, buf, bw, bh, &title, footer);
    let mut list_y = iy;
    let mut list_h = ih as usize;
    if p.filtering || !p.filter.is_empty() {
        let s = Style::new(th.bright, panel);
        let used = buf.put(ix, iy, &format!("> {}", p.filter), s, iw as usize);
        if p.filtering && app.blink {
            buf.set(ix + used as u16, iy, '█', Style::new(th.bright, panel));
        }
        for x in 0..iw {
            buf.set(ix + x, iy + 1, '─', Style::new(th.faint, panel));
        }
        list_y += 2;
        list_h = list_h.saturating_sub(2);
    }
    if p.view.is_empty() {
        let msg = if p.searching { "searching the archive…" } else { "no matches" };
        buf.put(ix + 1, list_y, msg, Style::new(th.dim, panel), iw as usize);
        return;
    }
    let n = p.view.len();
    let top = p.sel.saturating_sub(list_h / 2).min(n.saturating_sub(list_h));
    let lw = p.items.iter().take(200).map(|i| i.label.chars().count()).max().unwrap_or(4).clamp(4, 22) + 2;
    for i in 0..list_h.min(n) {
        let idx = top + i;
        let it = &p.items[p.view[idx]];
        let y = list_y + i as u16;
        let sel = idx == p.sel;
        let (bg, fg, fg2) = if sel { (th.select, th.bright, th.fg) } else { (panel, th.fg, th.dim) };
        buf.fill(ix, y, iw, 1, ' ', Style::new(fg, bg));
        let mut st = Style::new(fg, bg);
        let mut st2 = Style::new(fg2, bg);
        if sel {
            st = st.bold();
            st2 = st2.bold();
        }
        buf.put(ix, y, if sel { "▶" } else { " " }, st, 1);
        buf.put(ix + 1, y, &trunc(&it.label, lw - 1), Style::new(if sel { th.bright } else { th.warn.mix(th.fg, 0.5) }, bg), lw - 1);
        let dw = (iw as usize).saturating_sub(lw + 1);
        buf.put(ix + lw as u16 + 1, y, &trunc(&it.detail, dw), st2, dw);
    }
}

fn draw_help(app: &App, scroll: usize, buf: &mut Buffer) {
    let th = &app.theme;
    let panel = th.bg_center;
    let bw = (buf.w - 4).min(92);
    let bh = buf.h - 3;
    let (ix, iy, iw, ih) = boxed(app, buf, bw, bh, "HELP", "j/k scroll  q close");
    let lines = help::lines();
    let max = lines.len().saturating_sub(ih as usize);
    let top = scroll.min(max);
    for i in 0..ih as usize {
        let Some((k, d)) = lines.get(top + i) else { break };
        let y = iy + i as u16;
        if k.is_empty() {
            let st = if d.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) && !d.is_empty() {
                Style::new(th.heading, panel).bold()
            } else {
                Style::new(th.dim, panel)
            };
            buf.put(ix, y, d, st, iw as usize);
        } else {
            let kw = 30.min(iw as usize / 2);
            buf.put(ix + 1, y, k, Style::new(th.warn.mix(th.bright, 0.4), panel).bold(), kw);
            buf.put(ix + 1 + kw as u16 + 1, y, d, Style::new(th.fg, panel), iw as usize - kw - 3);
        }
    }
}

// ------------------------------------------------- tooltip & collection UI

/// Small info box for the bookmark / snippet under the cursor (or mouse).
fn draw_tooltip(app: &App, r: &crate::reader::Reader, buf: &mut Buffer, tx: usize, right: usize, ch: usize) {
    if !matches!(app.overlay, Overlay::None) || app.boot.is_some() {
        return;
    }
    let th = &app.theme;
    let (row, col) = app.hover.unwrap_or((r.row, r.col));
    if row >= r.len() || row < r.top || row >= r.top + ch {
        return;
    }
    let snip = r.snip_at(row, col).and_then(|s| app.coll.snippets.iter().find(|n| n.id == s.id));
    let bm = r.bookmark_on(row).and_then(|id| app.coll.bookmarks.iter().find(|b| b.id == id));
    if snip.is_none() && bm.is_none() {
        return;
    }
    // (text, colour, bold)
    let mut lines: Vec<(String, crate::theme::Rgb, bool)> = Vec::new();
    if let Some(s) = snip {
        lines.push((format!("▌ SNIPPET #{}", s.id), th.link, true));
        lines.push((s.name.clone(), th.fg, false));
        if !s.note.is_empty() {
            lines.push((s.note.clone(), th.dim.mix(th.fg, 0.5), false));
        }
        lines.push(("gS browse · gX delete".into(), th.dim, false));
    }
    if let Some(b) = bm {
        lines.push((format!("◆ BOOKMARK #{}", b.id), th.mark, true));
        if !b.note.is_empty() {
            lines.push((b.note.clone(), th.fg, false));
        }
        lines.push(("gB browse · gb remove".into(), th.dim, false));
    }
    let w = (lines.iter().map(|l| l.0.chars().count()).max().unwrap_or(10) + 4).clamp(24, 46);
    let h = lines.len() as u16 + 2;
    let panel = th.bg_center;
    let accent = if snip.is_some() { th.link } else { th.mark };
    let dcol = r.disp_col(row, col).saturating_sub(r.hx);
    let cx = (tx + dcol).min(right.saturating_sub(1));
    let y = 1 + (row - r.top) as u16;
    let below = (y as usize + 1 + h as usize) <= ch + 1;
    let by = if below { y + 1 } else { y.saturating_sub(h) };
    let bx = cx.min(right.saturating_sub(w)).max(1) as u16;
    let bs = Style::new(accent, panel);
    buf.fill(bx, by, w as u16, h, ' ', Style::new(th.fg, panel));
    for x in 1..w as u16 - 1 {
        buf.set(bx + x, by, '─', bs);
        buf.set(bx + x, by + h - 1, '─', bs);
    }
    for yy in 1..h - 1 {
        buf.set(bx, by + yy, '│', bs);
        buf.set(bx + w as u16 - 1, by + yy, '│', bs);
    }
    buf.set(bx, by, '┌', bs);
    buf.set(bx + w as u16 - 1, by, '┐', bs);
    buf.set(bx, by + h - 1, '└', bs);
    buf.set(bx + w as u16 - 1, by + h - 1, '┘', bs);
    for (i, (t, c, bold)) in lines.iter().enumerate() {
        let mut st = Style::new(*c, panel);
        if *bold {
            st = st.bold();
        }
        buf.put(bx + 2, by + 1 + i as u16, &trunc(t, w - 4), st, w - 4);
    }
}

fn draw_coll(app: &App, v: &CollView, buf: &mut Buffer) {
    let th = &app.theme;
    let panel = th.bg_center;
    let bw = buf.w - 4;
    let bh = buf.h - 3;
    let footer = if v.filtering {
        "type to filter  ⏎ done  esc stop"
    } else {
        "j/k move  ⏎ go  / filter  dd delete  r rename  e note  y yank  Tab switch  q close"
    };
    let (ix, iy, iw, ih) = boxed(app, buf, bw, bh, "BOOKMARKS & SNIPPETS", footer);
    let nb = app.coll.bookmarks.len();
    let ns = app.coll.snippets.len();
    // tabs
    let tab = |active: bool, accent: crate::theme::Rgb| {
        if active { Style::new(th.bg, accent).bold() } else { Style::new(th.dim, panel) }
    };
    let t1 = format!(" 1 ◆ BOOKMARKS ({nb}) ");
    let t2 = format!(" 2 ▌ SNIPPETS ({ns}) ");
    let u1 = buf.put(ix, iy, &t1, tab(v.tab == Tab::Bookmarks, th.mark), iw as usize);
    buf.put(ix + u1 as u16 + 1, iy, &t2, tab(v.tab == Tab::Snippets, th.link), iw as usize);
    if v.filtering || !v.filter.is_empty() {
        let s = format!("/{}", v.filter);
        let x = ix + iw - s.chars().count() as u16 - 2;
        let used = buf.put(x, iy, &s, Style::new(th.bright, panel), 40);
        if v.filtering && app.blink {
            buf.set(x + used as u16, iy, '█', Style::new(th.bright, panel));
        }
    }
    for x in 0..iw {
        buf.set(ix + x, iy + 1, '─', Style::new(th.faint, panel));
    }
    let items = app.coll_items(v.tab, &v.filter);
    let ly = iy + 2;
    let lh = ih.saturating_sub(2) as usize;
    let lw = (iw as usize * 42 / 100).max(24);
    let accent = if v.tab == Tab::Bookmarks { th.mark } else { th.link };
    for y in 0..lh as u16 {
        buf.set(ix + lw as u16, ly + y, '│', Style::new(th.faint, panel));
    }
    if items.is_empty() {
        let msg = match (v.tab, v.filter.is_empty()) {
            (_, false) => "no matches",
            (Tab::Bookmarks, _) => "no bookmarks yet — press gb on a line while reading",
            (Tab::Snippets, _) => "no snippets yet — select text with v / V, then press s",
        };
        buf.put(ix + 1, ly + 1, msg, Style::new(th.dim, panel), iw as usize - 2);
        return;
    }
    let n = items.len();
    let top = v.sel.saturating_sub(lh / 2).min(n.saturating_sub(lh));
    for i in 0..lh.min(n) {
        let idx = top + i;
        let it = &items[idx];
        let y = ly + i as u16;
        let sel = idx == v.sel;
        let (bg, fg, fg2) = if sel { (th.select, th.bright, th.fg) } else { (panel, th.fg, th.dim) };
        buf.fill(ix, y, lw as u16, 1, ' ', Style::new(fg, bg));
        let mut st = Style::new(fg, bg);
        if sel {
            st = st.bold();
        }
        buf.put(ix, y, if sel { "▶" } else { " " }, st, 1);
        let id = format!("#{:<3}", it.id);
        buf.put(ix + 1, y, &id, Style::new(accent, bg), 4);
        let place = format!("{:<9}", it.place);
        buf.put(ix + 6, y, &place, Style::new(fg2, bg), 9);
        let w = lw.saturating_sub(16);
        buf.put(ix + 16, y, &trunc(&it.label, w), st, w);
    }
    // preview
    let px = ix + lw as u16 + 2;
    let pw = (iw as usize).saturating_sub(lw + 3);
    let Some(it) = items.get(v.sel) else { return };
    let mut y = ly;
    buf.put(px, y, &trunc(&format!("PHRACK {} ▸ {}", it.issue, it.art_title), pw), Style::new(th.heading, panel).bold(), pw);
    y += 1;
    buf.put(px, y, &trunc(&format!("{}  ·  {}", it.author, it.place), pw), Style::new(th.dim, panel), pw);
    y += 1;
    if v.tab == Tab::Snippets {
        buf.put(px, y, &trunc(&format!("\"{}\"", it.label), pw), Style::new(th.link, panel).bold(), pw);
        y += 1;
    }
    if !it.note.is_empty() {
        buf.put(px, y, &trunc(&format!("note: {}", it.note), pw), Style::new(th.warn, panel), pw);
        y += 1;
    }
    y += 1;
    let avail = (ly as usize + lh).saturating_sub(y as usize);
    for (i, l) in v.preview.lines.iter().take(avail).enumerate() {
        let hi = v.preview.hi == Some(i);
        let st = if hi {
            Style::new(th.bright, th.cursorline).bold()
        } else if v.tab == Tab::Snippets {
            Style::new(th.fg.mix(th.link, 0.35), th.snippet)
        } else {
            Style::new(th.dim.mix(th.fg, 0.5), panel)
        };
        buf.fill(px, y + i as u16, pw as u16, 1, ' ', st);
        if hi {
            buf.set(px - 1, y + i as u16, '◆', Style::new(th.mark, panel).bold());
        }
        buf.put(px, y + i as u16, l, st, pw);
    }
}
