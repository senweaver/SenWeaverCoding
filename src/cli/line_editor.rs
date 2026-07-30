// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::io::{self, IsTerminal, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::editor_core::TextBuffer;
use crate::keybindings::parser::ParsedKey;
use crate::keybindings::schema::{KeyAction, KeyModifier};

pub enum ReplRead {
    Line(String),
    Eof,
    Interrupted,
}

struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> Option<Self> {
        if crossterm::terminal::enable_raw_mode().is_err() {
            return None;
        }
        let _ = crossterm::execute!(io::stdout(), event::EnableBracketedPaste);
        Some(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), event::DisableBracketedPaste);
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub fn read_line_interactive(prompt: &str, history: &[String]) -> Option<io::Result<ReplRead>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return None;
    }
    let guard = RawModeGuard::enter()?;
    let result = run_editor(prompt, history);
    drop(guard);
    Some(result)
}

struct EditorState {
    buffer: TextBuffer,
    cursor: usize,
    window_start: usize,
    hist_idx: Option<usize>,
    saved_draft: String,
}

impl EditorState {
    fn new() -> Self {
        Self {
            buffer: TextBuffer::new(),
            cursor: 0,
            window_start: 0,
            hist_idx: None,
            saved_draft: String::new(),
        }
    }

    fn set_text(&mut self, text: &str) {
        self.cursor = text.chars().count();
        self.buffer = TextBuffer::from_text(text);
        self.window_start = 0;
    }

    fn insert_str(&mut self, text: &str) {
        self.buffer.insert_at(self.cursor, text);
        self.cursor += text.chars().count();
        self.hist_idx = None;
    }
}

fn run_editor(prompt: &str, history: &[String]) -> io::Result<ReplRead> {
    let resolver = crate::keybindings::install_global_resolver_from_disk();
    let mut st = EditorState::new();
    redraw(prompt, &mut st)?;
    loop {
        match event::read()? {
            Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if let Some(parsed) = to_parsed_key(&k) {
                    if let Some(action) = resolver.resolve(&parsed) {
                        match action {
                            KeyAction::Submit => {
                                finish_visual_line()?;
                                return Ok(ReplRead::Line(st.buffer.as_string()));
                            }
                            KeyAction::NewLine => {
                                st.insert_str("\n");
                                redraw(prompt, &mut st)?;
                                continue;
                            }
                            KeyAction::HistoryPrev => {
                                history_prev(&mut st, history);
                                redraw(prompt, &mut st)?;
                                continue;
                            }
                            KeyAction::HistoryNext => {
                                history_next(&mut st, history);
                                redraw(prompt, &mut st)?;
                                continue;
                            }
                            KeyAction::Interrupt | KeyAction::Cancel => {
                                if st.buffer.char_count() == 0 {
                                    finish_visual_line()?;
                                    return Ok(ReplRead::Interrupted);
                                }
                                print_raw("^C\r\n")?;
                                st = EditorState::new();
                                redraw(prompt, &mut st)?;
                                continue;
                            }
                            KeyAction::Exit => {
                                if st.buffer.char_count() == 0 {
                                    finish_visual_line()?;
                                    return Ok(ReplRead::Eof);
                                }
                                if st.cursor < st.buffer.char_count() {
                                    st.buffer.delete_range(st.cursor, st.cursor + 1);
                                    st.hist_idx = None;
                                }
                                redraw(prompt, &mut st)?;
                                continue;
                            }
                            KeyAction::Clear => {
                                crossterm::execute!(
                                    io::stdout(),
                                    crossterm::terminal::Clear(
                                        crossterm::terminal::ClearType::All
                                    ),
                                    crossterm::cursor::MoveTo(0, 0),
                                )?;
                                redraw(prompt, &mut st)?;
                                continue;
                            }
                            _ => continue,
                        }
                    }
                }
                if handle_edit_key(&mut st, &k) {
                    redraw(prompt, &mut st)?;
                }
            }
            Event::Paste(text) => {
                let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                st.insert_str(&normalized);
                redraw(prompt, &mut st)?;
            }
            Event::Resize(_, _) => redraw(prompt, &mut st)?,
            _ => {}
        }
    }
}

fn to_parsed_key(key: &event::KeyEvent) -> Option<ParsedKey> {
    let key_str = match key.code {
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "escape".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "backtab".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return None,
    };
    let mut modifiers = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers.push(KeyModifier::Ctrl);
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        modifiers.push(KeyModifier::Alt);
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        modifiers.push(KeyModifier::Shift);
    }
    Some(ParsedKey {
        key: key_str,
        modifiers,
    })
}

fn handle_edit_key(st: &mut EditorState, k: &event::KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    match k.code {
        KeyCode::Char(c) if !ctrl && !alt => {
            st.buffer.insert_at(st.cursor, &c.to_string());
            st.cursor += 1;
            st.hist_idx = None;
            true
        }
        KeyCode::Char('a') if ctrl => {
            st.cursor = 0;
            true
        }
        KeyCode::Char('e') if ctrl => {
            st.cursor = st.buffer.char_count();
            true
        }
        KeyCode::Char('u') if ctrl => {
            if st.cursor > 0 {
                st.buffer.delete_range(0, st.cursor);
                st.cursor = 0;
                st.hist_idx = None;
            }
            true
        }
        KeyCode::Char('k') if ctrl => {
            let end = st.buffer.char_count();
            if st.cursor < end {
                st.buffer.delete_range(st.cursor, end);
                st.hist_idx = None;
            }
            true
        }
        KeyCode::Char('w') if ctrl => {
            delete_word_before(st);
            true
        }
        KeyCode::Backspace => {
            if st.cursor > 0 {
                st.buffer.delete_range(st.cursor - 1, st.cursor);
                st.cursor -= 1;
                st.hist_idx = None;
            }
            true
        }
        KeyCode::Delete => {
            if st.cursor < st.buffer.char_count() {
                st.buffer.delete_range(st.cursor, st.cursor + 1);
                st.hist_idx = None;
            }
            true
        }
        KeyCode::Left if ctrl || alt => {
            st.cursor = word_boundary_left(st);
            true
        }
        KeyCode::Right if ctrl || alt => {
            st.cursor = word_boundary_right(st);
            true
        }
        KeyCode::Left => {
            st.cursor = st.cursor.saturating_sub(1);
            true
        }
        KeyCode::Right => {
            st.cursor = (st.cursor + 1).min(st.buffer.char_count());
            true
        }
        KeyCode::Home => {
            st.cursor = 0;
            true
        }
        KeyCode::End => {
            st.cursor = st.buffer.char_count();
            true
        }
        _ => false,
    }
}

fn buffer_chars(st: &EditorState) -> Vec<char> {
    st.buffer.as_string().chars().collect()
}

fn word_boundary_left(st: &EditorState) -> usize {
    let chars = buffer_chars(st);
    let mut i = st.cursor;
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

fn word_boundary_right(st: &EditorState) -> usize {
    let chars = buffer_chars(st);
    let len = chars.len();
    let mut i = st.cursor;
    while i < len && chars[i].is_whitespace() {
        i += 1;
    }
    while i < len && !chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn delete_word_before(st: &mut EditorState) {
    let target = word_boundary_left(st);
    if target < st.cursor {
        st.buffer.delete_range(target, st.cursor);
        st.cursor = target;
        st.hist_idx = None;
    }
}

fn history_prev(st: &mut EditorState, history: &[String]) {
    if history.is_empty() {
        return;
    }
    let next_idx = match st.hist_idx {
        None => {
            st.saved_draft = st.buffer.as_string();
            history.len() - 1
        }
        Some(0) => 0,
        Some(i) => i - 1,
    };
    let text = history[next_idx].clone();
    st.set_text(&text);
    st.hist_idx = Some(next_idx);
}

fn history_next(st: &mut EditorState, history: &[String]) {
    match st.hist_idx {
        None => {}
        Some(i) if i + 1 < history.len() => {
            let text = history[i + 1].clone();
            st.set_text(&text);
            st.hist_idx = Some(i + 1);
        }
        Some(_) => {
            let draft = st.saved_draft.clone();
            st.set_text(&draft);
            st.hist_idx = None;
        }
    }
}

fn display_char_width(c: char) -> usize {
    if c == '\n' {
        1
    } else {
        UnicodeWidthChar::width(c).unwrap_or(0)
    }
}

fn redraw(prompt: &str, st: &mut EditorState) -> io::Result<()> {
    let cols = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
        .max(8);
    let prompt_width = UnicodeWidthStr::width(prompt);
    let avail = cols.saturating_sub(prompt_width + 1).max(1);
    let chars = buffer_chars(st);

    if st.cursor < st.window_start {
        st.window_start = st.cursor;
    }
    loop {
        let width_to_cursor: usize = chars[st.window_start..st.cursor]
            .iter()
            .map(|&c| display_char_width(c))
            .sum();
        if width_to_cursor < avail || st.window_start >= st.cursor {
            break;
        }
        st.window_start += 1;
    }

    let mut visible = String::new();
    let mut used = 0usize;
    for &c in &chars[st.window_start..] {
        let cw = display_char_width(c);
        if used + cw > avail {
            break;
        }
        if c == '\n' {
            visible.push('\u{23ce}');
        } else {
            visible.push(c);
        }
        used += cw;
    }

    let cursor_col = prompt_width
        + chars[st.window_start..st.cursor]
            .iter()
            .map(|&c| display_char_width(c))
            .sum::<usize>();

    let mut out = io::stdout();
    crossterm::queue!(
        out,
        crossterm::cursor::MoveToColumn(0),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine),
        crossterm::style::Print(prompt),
        crossterm::style::Print(&visible),
        crossterm::cursor::MoveToColumn(u16::try_from(cursor_col).unwrap_or(u16::MAX)),
    )?;
    out.flush()
}

fn print_raw(text: &str) -> io::Result<()> {
    let mut out = io::stdout();
    out.write_all(text.as_bytes())?;
    out.flush()
}

fn finish_visual_line() -> io::Result<()> {
    print_raw("\r\n")
}
