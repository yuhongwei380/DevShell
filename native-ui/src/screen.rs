use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    index::{Column, Line},
    term::{Config, Term, test::TermSize},
    vte::ansi::{ClearMode, Handler, Processor},
};

/// Keep the VT parser between packets: escape sequences and UTF-8 may be split
/// across WebSocket frames. Each session owns a separate screen and parser.
pub struct TerminalScreen {
    term: Term<VoidListener>,
    parser: Processor,
    pub columns: usize,
    pub rows: usize,
}

impl std::fmt::Debug for TerminalScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalScreen").finish_non_exhaustive()
    }
}

impl Default for TerminalScreen {
    fn default() -> Self {
        Self {
            term: Term::new(Config::default(), &TermSize::new(120, 30), VoidListener),
            parser: Processor::new(),
            columns: 120,
            rows: 30,
        }
    }
}

impl TerminalScreen {
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }
    pub fn resize(&mut self, columns: usize, rows: usize) -> bool {
        let columns = columns.clamp(20, 400);
        let rows = rows.clamp(5, 200);
        if (columns, rows) == (self.columns, self.rows) {
            return false;
        }
        self.term.resize(TermSize::new(columns, rows));
        self.columns = columns;
        self.rows = rows;
        true
    }
    pub fn clear(&mut self) {
        self.term.clear_screen(ClearMode::All);
        self.term.clear_screen(ClearMode::Saved);
    }
    #[cfg(test)]
    pub fn text(&self) -> String {
        let grid = self.term.grid();
        let mut output = String::new();
        for row in 0..grid.screen_lines() {
            let mut line = String::new();
            for col in 0..grid.columns() {
                let cell = &grid[Line(row as i32)][Column(col)];
                if !cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER)
                {
                    line.push(cell.c);
                }
            }
            output.push_str(line.trim_end());
            output.push('\n');
        }
        output.trim_end().to_owned()
    }
    pub fn ansi(&self) -> String {
        use alacritty_terminal::{term::cell::Flags, vte::ansi::Color};
        fn color_code(color: Color, background: bool) -> String {
            let selector = if background { 48 } else { 38 };
            match color {
                Color::Spec(rgb) => format!("\x1b[{selector};2;{};{};{}m", rgb.r, rgb.g, rgb.b),
                Color::Indexed(index) => format!("\x1b[{selector};5;{index}m"),
                Color::Named(named) => {
                    let index = named as usize;
                    if index < 16 {
                        format!("\x1b[{selector};5;{index}m")
                    } else {
                        format!("\x1b[{}m", if background { 49 } else { 39 })
                    }
                }
            }
        }
        let grid = self.term.grid();
        let mut output = String::new();
        let mut pen = None;
        for row in 0..grid.screen_lines() {
            for col in 0..grid.columns() {
                let cell = &grid[Line(row as i32)][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let style = (cell.fg, cell.bg, cell.flags);
                if pen != Some(style) {
                    output.push_str("\x1b[0m");
                    output.push_str(&color_code(cell.fg, false));
                    output.push_str(&color_code(cell.bg, true));
                    if cell.flags.contains(Flags::BOLD) {
                        output.push_str("\x1b[1m");
                    }
                    if cell.flags.contains(Flags::INVERSE) {
                        output.push_str("\x1b[7m");
                    }
                    pen = Some(style);
                }
                output.push(cell.c);
                if let Some(chars) = cell.zerowidth() {
                    for ch in chars {
                        output.push(*ch);
                    }
                }
            }
            output.push('\n');
        }
        output.push_str("\x1b[0m");
        output
    }
}

pub fn key_input(key: &gpui_kit::Keystroke) -> Option<String> {
    let special = match key.key.as_str() {
        "enter" => Some("\r"),
        "backspace" => Some("\x7f"),
        "tab" => Some("\t"),
        "escape" => Some("\x1b"),
        "up" => Some("\x1b[A"),
        "down" => Some("\x1b[B"),
        "right" => Some("\x1b[C"),
        "left" => Some("\x1b[D"),
        "home" => Some("\x1b[H"),
        "end" => Some("\x1b[F"),
        "delete" => Some("\x1b[3~"),
        "pageup" => Some("\x1b[5~"),
        "pagedown" => Some("\x1b[6~"),
        _ => None,
    };
    if let Some(special) = special {
        return Some(special.into());
    }
    if key.modifiers.control && key.key.len() == 1 {
        let byte = key.key.as_bytes()[0].to_ascii_uppercase();
        if (b'@'..=b'_').contains(&byte) {
            return Some(((byte & 0x1f) as char).to_string());
        }
    }
    if key.modifiers.platform {
        return None;
    }
    let text = key.key_char.clone().or_else(|| {
        if key.key.len() == 1 {
            Some(key.key.clone())
        } else if key.key == "space" {
            Some(" ".into())
        } else {
            None
        }
    })?;
    Some(if key.modifiers.alt {
        format!("\x1b{text}")
    } else {
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_motion_and_clear_are_screen_operations() {
        let mut screen = TerminalScreen::default();
        screen.feed(b"old\rnew\x1b[2J\x1b[Hhello");
        assert_eq!(screen.text(), "hello");
        screen.feed(b"\r\nworld\x1b[1A\rHELLO");
        assert_eq!(screen.text(), "HELLO\nworld");
    }
    #[test]
    fn fragmented_unicode_and_escape_sequences_survive() {
        let mut screen = TerminalScreen::default();
        for byte in "中文\x1b[31m红色\x1b[0m".as_bytes() {
            screen.feed(&[*byte]);
        }
        assert_eq!(screen.text(), "中文红色");
    }
    #[test]
    fn sessions_do_not_share_screen_state() {
        let mut first = TerminalScreen::default();
        let mut second = TerminalScreen::default();
        first.feed(b"first");
        second.feed(b"second");
        first.clear();
        assert_eq!(first.text(), "");
        assert_eq!(second.text(), "second");
    }
    #[test]
    fn special_keys_reach_remote_terminal() {
        assert_eq!(
            key_input(&gpui_kit::Keystroke::parse("ctrl-c").unwrap()).as_deref(),
            Some("\x03")
        );
        assert_eq!(
            key_input(&gpui_kit::Keystroke::parse("up").unwrap()).as_deref(),
            Some("\x1b[A")
        );
    }
}
