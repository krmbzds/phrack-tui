//! `$XDG_CONFIG_HOME/phrack-tui/config.toml`.
//!
//! The file is created on first run from a commented template, and read on every
//! start; edit it by hand. `:theme NAME` rewrites only the `theme` line, so your
//! comments survive.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PromptMode {
    /// Ask only when no issues are on disk.
    #[default]
    Missing,
    Always,
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Crt {
    pub scan: bool,
    pub glow: bool,
    pub vignette: bool,
    pub flicker: bool,
}

impl Default for Crt {
    fn default() -> Crt {
        Crt { scan: true, glow: true, vignette: true, flicker: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub archive_url: String,
    pub download_path: Option<PathBuf>,
    pub concurrency: usize,
    pub download_prompt: PromptMode,
    pub theme: String,
    pub boot: bool,
    pub crt: Crt,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            archive_url: "https://archives.phrack.org".into(),
            download_path: None,
            concurrency: 6,
            download_prompt: PromptMode::Missing,
            theme: "amber".into(),
            boot: true,
            crt: Crt::default(),
        }
    }
}

const TEMPLATE: &str = r#"# phrack-tui configuration.
# Edit freely: it is read at every start. `:theme NAME` in the app only
# rewrites the `theme` line below.

# Where philes are downloaded from.
archive_url = "https://archives.phrack.org"

# Where issues are stored. Default: $XDG_DATA_HOME/phrack-tui/issues
# (normally ~/.local/share/phrack-tui/issues).
# download_path = "~/Phrack"

# Parallel downloads, 1-16. Be kind to the server.
concurrency = 6

# When to show the "Download issues? Y/n" prompt after the boot sequence:
#   "missing"  only when no issues are on disk yet (default)
#   "always"   on every start, so you can top up with new issues
#   "never"    never; run :download yourself
download_prompt = "missing"

# Palette: "amber" (default), "crt" (green) or "dracula".
theme = "amber"

# Play the boot sequence at startup.
boot = true

# CRT effects.
[crt]
scan = true
glow = true
vignette = true
flicker = true
"#;

impl Config {
    /// Where issues live: `download_path` if set, else the XDG data directory.
    pub fn issues_dir(&self) -> PathBuf {
        match &self.download_path {
            Some(p) => paths::expand_tilde(p),
            None => paths::default_issues_dir(),
        }
    }

    /// `host[:port]` of the archive, for display.
    pub fn host(&self) -> String {
        self.archive_url.split("://").nth(1).unwrap_or(&self.archive_url).trim_end_matches('/').to_string()
    }

    fn sanitize(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        self.archive_url = self.archive_url.trim().trim_end_matches('/').to_string();
        if !(self.archive_url.starts_with("http://") || self.archive_url.starts_with("https://")) {
            w.push(format!("archive_url '{}' must start with http:// or https://; using the default", self.archive_url));
            self.archive_url = Config::default().archive_url;
        }
        if !(1..=16).contains(&self.concurrency) {
            self.concurrency = self.concurrency.clamp(1, 16);
            w.push(format!("concurrency clamped to {}", self.concurrency));
        }
        if crate::theme::by_name(&self.theme).is_none() {
            w.push(format!("unknown theme '{}'; using amber", self.theme));
            self.theme = "amber".into();
        }
        w
    }
}

/// Load the config, creating the template on first run. Never fails: problems
/// come back as warnings and the defaults are used.
pub fn load() -> (Config, Vec<String>) {
    load_from(&paths::config_file())
}

/// Like [`load`], but never creates the file (for `--paths` and the demo recorder).
pub fn load_readonly() -> (Config, Vec<String>) {
    let path = paths::config_file();
    if path.exists() { load_from(&path) } else { (Config::default(), Vec::new()) }
}

pub fn load_from(path: &std::path::Path) -> (Config, Vec<String>) {
    let mut warnings = Vec::new();
    let mut cfg = match fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("{}: {}; using defaults", path.display(), e.message()));
                Config::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            if let Err(e) = fs::write(path, TEMPLATE) {
                warnings.push(format!("could not create {}: {e}", path.display()));
            }
            Config::default()
        }
        Err(e) => {
            warnings.push(format!("could not read {}: {e}", path.display()));
            Config::default()
        }
    };
    warnings.extend(cfg.sanitize());
    (cfg, warnings)
}

/// Set `theme = "…"` in the config file, preserving everything else.
pub fn set_theme(name: &str) -> std::io::Result<()> {
    set_theme_in(&paths::config_file(), name)
}

pub fn set_theme_in(path: &std::path::Path, name: &str) -> std::io::Result<()> {
    let text = fs::read_to_string(path).unwrap_or_else(|_| TEMPLATE.to_string());
    let line = format!("theme = \"{name}\"");
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    let mut in_table = false;
    for l in text.lines() {
        let t = l.trim_start();
        if t.starts_with('[') {
            in_table = true;
        }
        if !in_table && !done && t.starts_with("theme") && t[5..].trim_start().starts_with('=') {
            out.push(line.clone());
            done = true;
        } else {
            out.push(l.to_string());
        }
    }
    if !done {
        // top-level keys must come before the first [table]
        let at = out.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(out.len());
        out.insert(at, line);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, out.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("phrack-cfg-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn template_parses_to_defaults_and_is_created() {
        let p = tmp("a").join("config.toml");
        let (c, w) = load_from(&p);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Config::default());
        assert!(p.exists());
        // the file it wrote parses back to the same thing
        let (c2, w2) = load_from(&p);
        assert!(w2.is_empty());
        assert_eq!(c2, Config::default());
    }

    #[test]
    fn partial_and_bad_values() {
        let p = tmp("b").join("config.toml");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "concurrency = 99\ntheme = \"nope\"\narchive_url = \"ftp://x\"\n[crt]\nflicker = false\n").unwrap();
        let (c, w) = load_from(&p);
        assert_eq!(c.concurrency, 16);
        assert_eq!(c.theme, "amber");
        assert_eq!(c.archive_url, "https://archives.phrack.org");
        assert!(!c.crt.flicker && c.crt.scan);
        assert_eq!(w.len(), 3, "{w:?}");
        fs::write(&p, "this is = not [valid").unwrap();
        let (c, w) = load_from(&p);
        assert_eq!(c, Config::default());
        assert!(w[0].contains("using defaults"));
    }

    #[test]
    fn set_theme_keeps_comments() {
        let p = tmp("c").join("config.toml");
        let _ = load_from(&p);
        set_theme_in(&p, "dracula").unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("theme = \"dracula\"") && !text.contains("theme = \"amber\""));
        assert!(text.contains("# Palette:") && text.contains("[crt]"));
        assert_eq!(load_from(&p).0.theme, "dracula");
        // no theme line at all: inserted before the first table
        fs::write(&p, "boot = false\n[crt]\nscan = false\n").unwrap();
        set_theme_in(&p, "crt").unwrap();
        let (c, w) = load_from(&p);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((c.theme.as_str(), c.boot, c.crt.scan), ("crt", false, false));
    }

    #[test]
    fn issues_dir_default_and_override() {
        let mut c = Config::default();
        assert!(c.issues_dir().ends_with("phrack-tui/issues"));
        c.download_path = Some("/srv/phrack".into());
        assert_eq!(c.issues_dir(), PathBuf::from("/srv/phrack"));
        assert_eq!(c.host(), "archives.phrack.org");
    }
}
