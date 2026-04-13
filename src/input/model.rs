use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn normalize_app_key_binding(
    code: KeyCode,
    modifiers: KeyModifiers,
    shifted_codepoint: Option<u32>,
) -> (KeyCode, KeyModifiers) {
    let KeyCode::Char(raw_char) = code else {
        return (code, modifiers);
    };

    let shifted_char = shifted_codepoint
        .and_then(char::from_u32)
        .or_else(|| modifiers.contains(KeyModifiers::SHIFT).then_some(raw_char))
        .and_then(shifted_printable_char);

    let normalized_char = shifted_char.unwrap_or(raw_char);
    let mut normalized_modifiers = modifiers;
    if shifted_char.is_some() {
        normalized_modifiers.remove(KeyModifiers::SHIFT);
    }

    (KeyCode::Char(normalized_char), normalized_modifiers)
}

fn shifted_printable_char(ch: char) -> Option<char> {
    match ch {
        'a'..='z' => Some(ch.to_ascii_uppercase()),
        'A'..='Z' => Some(ch),
        '`' | '~' => Some('~'),
        '1' | '!' => Some('!'),
        '2' | '@' => Some('@'),
        '3' | '#' => Some('#'),
        '4' | '$' => Some('$'),
        '5' | '%' => Some('%'),
        '6' | '^' => Some('^'),
        '7' | '&' => Some('&'),
        '8' | '*' => Some('*'),
        '9' | '(' => Some('('),
        '0' | ')' => Some(')'),
        '-' | '_' => Some('_'),
        '=' | '+' => Some('+'),
        '[' | '{' => Some('{'),
        ']' | '}' => Some('}'),
        '\\' | '|' => Some('|'),
        ';' | ':' => Some(':'),
        '\'' | '"' => Some('"'),
        ',' | '<' => Some('<'),
        '.' | '>' => Some('>'),
        '/' | '?' => Some('?'),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalKey {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: crossterm::event::KeyEventKind,
    pub shifted_codepoint: Option<u32>,
}

impl TerminalKey {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            shifted_codepoint: None,
        }
    }

    pub fn with_kind(mut self, kind: crossterm::event::KeyEventKind) -> Self {
        self.kind = kind;
        self
    }

    #[allow(dead_code)] // Reserved for the upcoming raw input parser to preserve shifted/base key pairs.
    pub fn with_shifted_codepoint(mut self, shifted_codepoint: u32) -> Self {
        self.shifted_codepoint = Some(shifted_codepoint);
        self
    }

    pub fn as_key_event(self) -> KeyEvent {
        KeyEvent::new_with_kind(self.code, self.modifiers, self.kind)
    }
}

impl From<KeyEvent> for TerminalKey {
    fn from(value: KeyEvent) -> Self {
        Self::new(value.code, value.modifiers).with_kind(value.kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardProtocol {
    Legacy,
    Kitty { flags: u16 },
}

impl KeyboardProtocol {
    pub fn from_kitty_flags(flags: u16) -> Self {
        if flags == 0 {
            Self::Legacy
        } else {
            Self::Kitty { flags }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseProtocolMode {
    None,
    Press,
    PressRelease,
    ButtonMotion,
    AnyMotion,
}

impl MouseProtocolMode {
    pub fn reporting_enabled(self) -> bool {
        self != Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseProtocolEncoding {
    Default,
    Utf8,
    Sgr,
}
