mod app;
mod boot;
mod buffer;
mod collection;
mod collview;
mod complete;
mod crt;
mod data;
mod demo;
mod doc;
mod help;
mod input;
mod picker;
mod reader;
mod state;
mod theme;
mod ui;

use std::{
    io::{self, BufWriter, Write},
    time::Duration,
};

use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::{
    app::App,
    boot::Boot,
    buffer::{Buffer, Style, Terminal},
    input::K,
    state::PState,
};

const USAGE: &str = "\
phrack-tui — read Phrack on a green phosphor screen

USAGE:
    phrack-tui [OPTIONS] [ISSUES_DIR]

OPTIONS:
        --no-boot       skip the modem boot sequence
        --theme NAME    amber (default), crt, dracula (`:theme` inside saves your choice)
        --no-crt        disable scanlines / glow / vignette / flicker
        --dump WxH      render the first frame as plain text and exit (debugging)
        --demo-svg FILE record the animated README demo to FILE and exit
    -h, --help          show this help

ISSUES_DIR defaults to $PHRACK_ISSUES, ./issues, then the repo's issues/ directory.
Press F1 inside for the key reference.
";

struct Opts {
    dir: Option<String>,
    boot: bool,
    theme: Option<String>,
    crt: bool,
    dump: Option<(u16, u16)>,
    demo: Option<String>,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts { dir: None, boot: true, theme: None, crt: true, dump: None, demo: None };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--no-boot" => o.boot = false,
            "--no-crt" => o.crt = false,
            "--theme" => o.theme = Some(it.next().ok_or("--theme needs a name")?),
            "--demo-svg" => o.demo = Some(it.next().ok_or("--demo-svg needs a file")?),
            "--dump" => {
                let v = it.next().ok_or("--dump needs WxH")?;
                let (w, h) = v.split_once('x').ok_or("--dump expects WxH")?;
                o.dump = Some((w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?));
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ => o.dir = Some(a),
        }
    }
    Ok(o)
}

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("phrack-tui: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let Some(root) = data::find_root(opts.dir.as_deref()) else {
        eprintln!("phrack-tui: cannot find the issues directory. Pass it as an argument or set PHRACK_ISSUES.");
        std::process::exit(1);
    };
    let lib = match data::Library::scan(&root) {
        Ok(l) if !l.issues.is_empty() => l,
        Ok(_) => {
            eprintln!("phrack-tui: no issues found in {}", root.display());
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("phrack-tui: {}: {e}", root.display());
            std::process::exit(1);
        }
    };
    let st = PState::load(state::default_path());
    let mut theme_override: Option<String> = None;
    if let Some(t) = &opts.theme {
        if theme::by_name(t).is_none() {
            eprintln!("phrack-tui: unknown theme '{t}'");
            std::process::exit(2);
        }
        theme_override = Some(t.clone());
    }

    if let Some(out) = &opts.demo {
        if let Err(e) = demo::record(lib, out) {
            eprintln!("phrack-tui: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Some((w, h)) = opts.dump {
        let coll = collection::Collection::load(collection::path_for(st.path.as_deref()));
        let mut app = App::new(lib, st, coll, (w, h));
        if let Some(t) = theme_override.as_deref().and_then(theme::by_name) {
            app.theme = t;
        }
        app.fx = crt::Fx::none();
        let mut buf = Buffer::new(w, h, Style::new(app.theme.fg, app.theme.bg));
        ui::render(&app, &mut buf);
        print!("{}", buf.to_text());
        return;
    }

    let (w, h) = terminal::size().unwrap_or((100, 32));
    let coll = collection::Collection::load(collection::path_for(st.path.as_deref()));
    let mut app = App::new(lib, st, coll, (w, h));
    if let Some(t) = theme_override.as_deref().and_then(theme::by_name) {
        app.theme = t; // --theme applies to this session only; :theme persists
    }
    if !opts.crt {
        app.fx = crt::Fx::none();
    }
    if opts.boot && std::env::var_os("PHRACK_NO_BOOT").is_none() {
        app.boot = Some(Boot::new(&app.boot_info(buffer::detect_truecolor())));
    }
    if let Err(e) = run(&mut app) {
        eprintln!("phrack-tui: {e}");
        std::process::exit(1);
    }
}

fn restore() {
    let _ = execute!(io::stdout(), DisableMouseCapture, Show, LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

fn run(app: &mut App) -> io::Result<()> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));

    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)?;
    let result = event_loop(app);
    app.save_pos();
    restore();
    result
}

fn event_loop(app: &mut App) -> io::Result<()> {
    let mut term = Terminal::new();
    let mut out = BufWriter::with_capacity(1 << 16, io::stdout());
    let (w, h) = app.size;
    let mut buf = Buffer::new(w, h, Style::new(app.theme.fg, app.theme.bg));
    let mut dirty = true;
    loop {
        if dirty || app.redraw_all {
            if app.redraw_all {
                term.invalidate();
                app.redraw_all = false;
            }
            let (w, h) = app.size;
            if buf.w != w || buf.h != h {
                buf = Buffer::new(w, h, Style::new(app.theme.fg, app.theme.bg));
            }
            ui::render(app, &mut buf);
            term.flush(&buf, &mut out)?;
            out.flush()?;
            dirty = false;
        }
        if app.quit {
            return Ok(());
        }
        let wait = if app.boot.is_some() { 30 } else { 60 };
        if event::poll(Duration::from_millis(wait))? {
            // drain everything that is queued before redrawing
            loop {
                match event::read()? {
                    Event::Key(k) => {
                        if let Some(k) = K::from_event(k) {
                            app.on_key(k);
                        }
                    }
                    Event::Mouse(m) => app.on_mouse(m),
                    Event::Resize(w, h) => app.resize(w, h),
                    _ => {}
                }
                dirty = true;
                if app.quit || !event::poll(Duration::from_millis(0))? {
                    break;
                }
            }
        }
        dirty |= app.tick();
    }
}
