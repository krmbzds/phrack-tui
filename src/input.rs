//! Backend-neutral key representation.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum K {
    Char(char),
    Ctrl(char),
    Enter,
    Esc,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    F1,
}

impl K {
    pub fn from_event(ev: KeyEvent) -> Option<K> {
        if ev.kind == KeyEventKind::Release {
            return None;
        }
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        Some(match ev.code {
            KeyCode::Char(c) if ctrl => K::Ctrl(c.to_ascii_lowercase()),
            KeyCode::Char(c) => K::Char(c),
            KeyCode::Enter => K::Enter,
            KeyCode::Esc => K::Esc,
            KeyCode::Backspace => K::Backspace,
            KeyCode::Tab => K::Tab,
            KeyCode::BackTab => K::Tab,
            KeyCode::Up => K::Up,
            KeyCode::Down => K::Down,
            KeyCode::Left => K::Left,
            KeyCode::Right => K::Right,
            KeyCode::PageUp => K::PageUp,
            KeyCode::PageDown => K::PageDown,
            KeyCode::Home => K::Home,
            KeyCode::End => K::End,
            KeyCode::F(1) => K::F1,
            _ => return None,
        })
    }
}
