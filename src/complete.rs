//! Tab completion for the `:` command line.

use crate::{collection::Collection, data::Library, theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arg {
    None,
    Theme,
    SetOpt,
    Issue,
    Author,
    SnippetId,
    BookmarkId,
}

/// Every command the executor understands, with what its argument is.
/// (One- and two-letter aliases still work when typed; they just aren't offered.)
const COMMANDS: &[(&str, Arg)] = &[
    ("author", Arg::Author),
    ("authors", Arg::Author),
    ("bm", Arg::None),
    ("bookmark", Arg::None),
    ("bookmarks", Arg::None),
    ("colorscheme", Arg::Theme),
    ("delbm", Arg::BookmarkId),
    ("delmarks", Arg::None),
    ("delsnip", Arg::SnippetId),
    ("find", Arg::None),
    ("grep", Arg::None),
    ("help", Arg::None),
    ("intro", Arg::None),
    ("issue", Arg::Issue),
    ("linenoise", Arg::None),
    ("loopback", Arg::None),
    ("markread", Arg::None),
    ("marks", Arg::None),
    ("markunread", Arg::None),
    ("noh", Arg::None),
    ("nohlsearch", Arg::None),
    ("open", Arg::Issue),
    ("outline", Arg::None),
    ("prophile", Arg::None),
    ("pwn", Arg::None),
    ("q", Arg::None),
    ("qa", Arg::None),
    ("quit", Arg::None),
    ("random", Arg::None),
    ("sections", Arg::None),
    ("set", Arg::SetOpt),
    ("snippets", Arg::None),
    ("theme", Arg::Theme),
    ("toc", Arg::Issue),
    ("unread", Arg::None),
    ("worldnews", Arg::None),
    ("wq", Arg::None),
    ("write", Arg::None),
];

pub const SET_OPTIONS: &[&str] = &[
    "crt", "nocrt", "flicker", "noflicker", "glow", "noglow", "hlsearch", "nohlsearch", "number", "nonumber",
    "relativenumber", "norelativenumber", "rnu", "nornu", "scan", "noscan", "vignette", "novignette",
];

fn arg_kind(cmd: &str) -> Arg {
    match cmd {
        "theme" | "colorscheme" | "colo" => Arg::Theme,
        "set" | "se" => Arg::SetOpt,
        "issue" | "i" | "toc" | "t" | "open" | "o" | "e" | "edit" => Arg::Issue,
        "author" | "au" | "authors" => Arg::Author,
        "delsnip" | "delsnippet" => Arg::SnippetId,
        "delbm" | "delbookmark" => Arg::BookmarkId,
        _ => Arg::None,
    }
}

/// Whether a command name takes an argument worth completing.
pub fn takes_arg(cmd: &str) -> bool {
    arg_kind(cmd) != Arg::None
}

fn starts(item: &str, tok: &str) -> bool {
    item.to_lowercase().starts_with(&tok.to_lowercase())
}

/// Returns (byte offset where the completed token starts, candidates).
pub fn candidates(text: &str, lib: &Library, coll: &Collection) -> (usize, Vec<String>) {
    // a visual-mode range prefix is not part of the command
    let base = if text.starts_with("'<,'>") { 5 } else { 0 };
    let rest = &text[base..];
    let lead = rest.len() - rest.trim_start().len();
    let body = &rest[lead..];
    let body_off = base + lead;

    let Some(sp) = body.find(char::is_whitespace) else {
        // completing the command name itself
        let mut v: Vec<String> = COMMANDS.iter().filter(|(n, _)| starts(n, body)).map(|(n, _)| n.to_string()).collect();
        v.dedup();
        return (body_off, v);
    };
    let cmd = &body[..sp];
    let args = &body[sp..];
    let args_off = body_off + sp + (args.len() - args.trim_start().len());
    let args = args.trim_start();
    let kind = arg_kind(cmd);

    // last whitespace-separated word (or the whole rest for names with spaces)
    let (tok_off, tok) = if kind == Arg::Author {
        (args_off, args)
    } else {
        let i = args.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        (args_off + i, &args[i..])
    };

    let v: Vec<String> = match kind {
        Arg::None => Vec::new(),
        Arg::Theme => theme::all().iter().map(|t| t.name.to_string()).filter(|n| starts(n, tok)).collect(),
        Arg::SetOpt => {
            let mut all: Vec<String> = SET_OPTIONS.iter().map(|s| s.to_string()).collect();
            all.extend(theme::all().iter().map(|t| format!("theme={}", t.name)));
            all.into_iter().filter(|n| starts(n, tok)).collect()
        }
        Arg::Issue => match tok.split_once('/') {
            Some((i, a)) => match i.parse::<u32>().ok().and_then(|n| lib.issue(n)) {
                Some(iss) => iss
                    .articles
                    .iter()
                    .map(|x| format!("{}/{}", iss.num, x.num))
                    .filter(|c| c.starts_with(&format!("{i}/{a}")))
                    .collect(),
                None => Vec::new(),
            },
            None => lib.issues.iter().map(|i| i.num.to_string()).filter(|n| n.starts_with(tok)).collect(),
        },
        Arg::Author => {
            let mut v: Vec<String> = lib.authors().into_iter().map(|a| a.0).filter(|n| starts(n, tok)).collect();
            v.truncate(500);
            v
        }
        Arg::SnippetId => coll.snippets.iter().map(|s| s.id.to_string()).filter(|n| n.starts_with(tok)).collect(),
        Arg::BookmarkId => coll.bookmarks.iter().map(|s| s.id.to_string()).filter(|n| n.starts_with(tok)).collect(),
    };
    (tok_off, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Article, Issue};

    fn lib() -> Library {
        let a = |i: u32, n: u32, au: &str| Article { issue: i, num: n, title: "t".into(), author: au.into(), path: "x".into(), size: 0 };
        Library {
            issues: vec![
                Issue { num: 7, articles: vec![a(7, 1, "Taran King"), a(7, 2, "Knight Lightning")] },
                Issue { num: 25, articles: vec![a(25, 1, "Taran King")] },
            ],
        }
    }

    #[test]
    fn names_and_args() {
        let (l, c) = (lib(), Collection::default());
        let (off, v) = candidates("th", &l, &c);
        assert_eq!((off, v), (0, vec!["theme".to_string()]));
        let (off, v) = candidates("theme ", &l, &c);
        assert_eq!(off, 6);
        assert_eq!(v, vec!["amber", "crt", "dracula"]);
        assert_eq!(candidates("theme d", &l, &c).1, vec!["dracula"]);
        let (off, v) = candidates("set nos", &l, &c);
        assert_eq!((off, v), (4, vec!["noscan".to_string()]));
        assert!(candidates("set theme=a", &l, &c).1.contains(&"theme=amber".to_string()));
        assert_eq!(candidates("open 2", &l, &c).1, vec!["25"]);
        assert_eq!(candidates("open 7/", &l, &c).1, vec!["7/1", "7/2"]);
        assert_eq!(candidates("author kn", &l, &c).1, vec!["Knight Lightning"]);
        assert_eq!(candidates("author Taran K", &l, &c).1, vec!["Taran King"]);
        assert!(candidates("grep x", &l, &c).1.is_empty());
        // visual range prefix is skipped
        assert_eq!(candidates("'<,'>th", &l, &c), (5, vec!["theme".to_string()]));
    }
}
