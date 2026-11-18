//! XDG base-directory locations.
//!
//!   config  $XDG_CONFIG_HOME/phrack-tui/config.toml   (~/.config)
//!   data    $XDG_DATA_HOME/phrack-tui/issues/          (~/.local/share)
//!   state   $XDG_STATE_HOME/phrack-tui/state           (~/.local/state)
//!
//! Per the spec, an unset, empty or relative `XDG_*` variable is ignored.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

const APP: &str = "phrack-tui";

fn home_from(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(|k| env(k))
        .map(PathBuf::from)
        .find(|p| p.is_absolute())
}

fn base(env: &dyn Fn(&str) -> Option<OsString>, var: &str, fallback: &str) -> PathBuf {
    env(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home_from(env).map(|h| h.join(fallback)))
        .unwrap_or_else(|| PathBuf::from(".").join(fallback))
}

fn real_env(k: &str) -> Option<OsString> {
    std::env::var_os(k)
}

pub fn config_dir() -> PathBuf {
    base(&real_env, "XDG_CONFIG_HOME", ".config").join(APP)
}

pub fn data_dir() -> PathBuf {
    base(&real_env, "XDG_DATA_HOME", ".local/share").join(APP)
}

pub fn state_dir() -> PathBuf {
    base(&real_env, "XDG_STATE_HOME", ".local/state").join(APP)
}

/// `$PHRACK_TUI_CONFIG` overrides the location (handy for tests and scripts).
pub fn config_file() -> PathBuf {
    std::env::var_os("PHRACK_TUI_CONFIG").map(PathBuf::from).unwrap_or_else(|| config_dir().join("config.toml"))
}

pub fn default_issues_dir() -> PathBuf {
    data_dir().join("issues")
}

/// `~/…` for paths under $HOME, for display.
pub fn tilde(p: &Path) -> String {
    if let Some(h) = home_from(&real_env) {
        if let Ok(rest) = p.strip_prefix(&h) {
            return format!("~/{}", rest.display());
        }
    }
    p.display().to_string()
}

/// Expand a leading `~/` in a user-supplied path.
pub fn expand_tilde(p: &Path) -> PathBuf {
    match (p.strip_prefix("~"), home_from(&real_env)) {
        (Ok(rest), Some(h)) => h.join(rest),
        _ => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).map(OsString::from)
    }

    #[test]
    fn follows_xdg_spec() {
        let e = env(&[("HOME", "/home/u")]);
        assert_eq!(base(&e, "XDG_DATA_HOME", ".local/share"), PathBuf::from("/home/u/.local/share"));
        let e = env(&[("HOME", "/home/u"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(base(&e, "XDG_DATA_HOME", ".local/share"), PathBuf::from("/data"));
        // empty and relative values are ignored
        let e = env(&[("HOME", "/home/u"), ("XDG_CONFIG_HOME", ""), ("XDG_DATA_HOME", "rel/dir")]);
        assert_eq!(base(&e, "XDG_CONFIG_HOME", ".config"), PathBuf::from("/home/u/.config"));
        assert_eq!(base(&e, "XDG_DATA_HOME", ".local/share"), PathBuf::from("/home/u/.local/share"));
    }
}
