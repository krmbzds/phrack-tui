mod app;
mod boot;
mod buffer;
mod collection;
mod collview;
mod complete;
mod crt;
mod config;
mod data;
mod demo;
mod dlflow;
mod doc;
mod download;
mod help;
mod input;
mod paths;
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

FILES (XDG):
    config   $XDG_CONFIG_HOME/phrack-tui/config.toml   (created on first run)
    issues   $XDG_DATA_HOME/phrack-tui/issues/         (or download_path in the config)
    state    $XDG_STATE_HOME/phrack-tui/state

ISSUES_DIR overrides the location; so does $PHRACK_ISSUES.
On a first run the app offers to download the archive; `:download` does it any time.
Press F1 inside for the key reference.
";

struct Opts {
    dir: Option<String>,
    boot: bool,
    theme: Option<String>,
    crt: bool,
    dump: Option<(u16, u16)>,
    demo: Option<String>,
    paths: bool,
    /// `--download [ISSUE]`: headless download, then exit.
    download: Option<Option<u32>>,
    refresh: bool,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts { dir: None, boot: true, theme: None, crt: true, dump: None, demo: None, paths: false, download: None, refresh: false };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--no-boot" => o.boot = false,
            "--no-crt" => o.crt = false,
            "--theme" => o.theme = Some(it.next().ok_or("--theme needs a name")?),
            "--paths" => o.paths = true,
            "--refresh" => o.refresh = true,
            "--download" => {
                o.download = Some(None);
                if let Some(n) = it.peek().and_then(|v| v.parse::<u32>().ok()) {
                    it.next();
                    o.download = Some(Some(n));
                }
            }
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
    let readonly = opts.paths || opts.demo.is_some();
    let (cfg, warnings) = if readonly { config::load_readonly() } else { config::load() };
    for w in &warnings {
        eprintln!("phrack-tui: config: {w}");
    }
    let root = data::resolve_root(opts.dir.as_deref(), &cfg);

    if opts.paths {
        println!("config   {}", paths::config_file().display());
        println!("issues   {}", root.display());
        println!("state    {}", state::default_path().map(|p| p.display().to_string()).unwrap_or_default());
        println!("archive  {}", cfg.archive_url);
        return;
    }

    if let Some(issue) = &opts.download {
        let only = match issue {
            Some(n) => Some(*n),
            None => None,
        };
        std::process::exit(download::run_cli(download::Options {
            archive_url: cfg.archive_url.clone(),
            root,
            concurrency: cfg.concurrency,
            refresh: opts.refresh,
            only,
        }));
    }

    let lib = match data::Library::scan(&root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("phrack-tui: cannot read {}: {e}", root.display());
            std::process::exit(1);
        }
    };

    if let Some(out) = &opts.demo {
        if let Err(e) = demo::record(lib, out) {
            eprintln!("phrack-tui: {e}");
            std::process::exit(1);
        }
        return;
    }

    let mut cfg = cfg;
    if let Some(t) = &opts.theme {
        if theme::by_name(t).is_none() {
            eprintln!("phrack-tui: unknown theme '{t}'");
            std::process::exit(2);
        }
        cfg.theme = t.clone(); // this run only; `:theme` is what writes config.toml
    }
    if !opts.crt {
        cfg.crt = config::Crt { scan: false, glow: false, vignette: false, flicker: false };
    }
    let st = PState::load(state::default_path());
    let coll = collection::Collection::load(collection::path_for(st.path.as_deref()));

    if let Some((w, h)) = opts.dump {
        let mut app = App::new(lib, st, coll, cfg, root, (w, h));
        app.fx = crt::Fx::none();
        app.start();
        let mut buf = Buffer::new(w, h, Style::new(app.theme.fg, app.theme.bg));
        ui::render(&app, &mut buf);
        print!("{}", buf.to_text());
        return;
    }

    let (w, h) = terminal::size().unwrap_or((100, 32));
    let want_boot = cfg.boot && opts.boot && std::env::var_os("PHRACK_NO_BOOT").is_none();
    let mut app = App::new(lib, st, coll, cfg, root, (w, h));
    app.persist_config = true;
    if want_boot {
        app.boot = Some(Boot::new(&app.boot_info(buffer::detect_truecolor())));
    }
    app.start();
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
