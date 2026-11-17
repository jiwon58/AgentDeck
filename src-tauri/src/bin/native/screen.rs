use agent_deck::{
    model::Session,
    terminal::{Payload, Terminal},
};
use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point},
    term::{cell::Flags, Config, TermMode},
    vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb},
    Term,
};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Key, Modifiers, Rect, Sense};
use std::sync::mpsc;

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

struct Listener(mpsc::Sender<Event>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

pub struct Screen {
    pub session: Session,
    term: Term<Listener>,
    parser: Processor,
    events: mpsc::Receiver<Event>,
    size: Size,
    pub status: String,
    live: bool,
    anchor: Option<Point>,
    cursor: Option<Point>,
    ime: String,
    work: Option<&'static str>,
    native_status: bool,
    osc_tail: Vec<u8>,
}

impl Screen {
    pub fn new(session: Session) -> Self {
        let (sender, events) = mpsc::channel();
        let size = Size {
            cols: 100,
            rows: 30,
        };
        let term = Term::new(
            Config {
                scrolling_history: 3000,
                ..Config::default()
            },
            &size,
            Listener(sender),
        );
        Self {
            session,
            term,
            parser: Processor::new(),
            events,
            size,
            status: "Running".into(),
            live: true,
            anchor: None,
            cursor: None,
            ime: String::new(),
            work: None,
            native_status: false,
            osc_tail: Vec::new(),
        }
    }

    pub fn drain(&mut self, terminal: &Terminal) {
        let mut changed = false;
        if let Ok(events) = terminal.drain(&self.session.id) {
            for event in events {
                match event.payload {
                    Payload::Bytes(bytes) => {
                        if self.session.agent == agent_deck::model::Agent::Gajae
                            && self.session.version != "0.10.2"
                        {
                            self.host_status(&bytes);
                        }
                        self.parser.advance(&mut self.term, &bytes);
                        changed = true;
                    }
                    Payload::Json(value) => match event.event_type {
                        "terminal_exited" => {
                            self.live = false;
                            self.status = format!("Exited · {}", value["code"]);
                        }
                        "terminal_error" => {
                            self.status = value.as_str().unwrap_or("Terminal error").into()
                        }
                        _ => {}
                    },
                }
            }
        }
        if changed
            && self.live
            && self.session.agent == agent_deck::model::Agent::Gajae
            && self.session.version == "0.10.2"
            && !self.native_status
        {
            self.legacy_status();
        }
        if self.live {
            if let Some(work) = self.work {
                self.status = work.into();
            }
        }
        for event in self.events.try_iter() {
            let reply = match event {
                Event::PtyWrite(bytes) => bytes,
                Event::TextAreaSizeRequest(reply) => reply(WindowSize {
                    num_lines: self.size.rows as u16,
                    num_cols: self.size.cols as u16,
                    cell_width: 9,
                    cell_height: 18,
                }),
                Event::ColorRequest(index, reply) => {
                    let [r, g, b, _] = color(Color::Indexed(index as u8)).to_array();
                    reply(Rgb { r, g, b })
                }
                _ => continue,
            };
            let _ = terminal.write(&self.session.id, reply);
        }
    }

    fn host_status(&mut self, bytes: &[u8]) {
        const PREFIX: &[u8] = b"\x1b]777;notify;Terax;gjc;";
        self.osc_tail.extend_from_slice(bytes);
        for (index, _) in self
            .osc_tail
            .windows(PREFIX.len())
            .enumerate()
            .filter(|(_, value)| *value == PREFIX)
        {
            let tail = &self.osc_tail[index + PREFIX.len()..];
            let Some(end) = tail
                .iter()
                .take(12)
                .position(|byte| matches!(byte, b'\x07' | b'\x1b'))
            else {
                continue;
            };
            match &tail[..end] {
                b"working" => self.work = Some("Work in progress"),
                b"attention" => self.work = Some("Your response needed"),
                b"finished" => self.work = Some("Agent run finished"),
                b"idle" => self.work = Some("Ready"),
                _ => continue,
            }
            self.native_status = true;
        }
        let keep = (PREFIX.len() + 12).min(self.osc_tail.len());
        self.osc_tail.drain(..self.osc_tail.len() - keep);
    }

    fn legacy_status(&mut self) {
        let rows = self.size.rows;
        let lines: Vec<_> = (rows.saturating_sub(22)..rows)
            .map(|row| {
                (0..self.size.cols)
                    .map(|column| self.term.grid()[Line(row as i32)][Column(column)].c)
                    .collect::<String>()
            })
            .collect();
        if matches!(self.work, Some("Work in progress" | "Your response needed"))
            && lines.iter().any(|line| {
                let line = line.trim();
                line.starts_with("up/down navigate")
                    && line.contains("enter select")
                    && line.ends_with("esc cancel")
            })
        {
            self.work = Some("Your response needed");
            return;
        }
        let composer = lines.iter().rposition(|line| {
            line.trim()
                .strip_prefix('╭')
                .and_then(|line| line.strip_suffix('╮'))
                .is_some_and(|line| !line.is_empty() && line.chars().all(|ch| ch == '─'))
        });
        let footer = lines
            .iter()
            .rposition(|line| line.trim_end().ends_with("v0.10.2"));
        if let (Some(composer), Some(footer)) = (composer, footer) {
            if composer.abs_diff(footer) <= 8 {
                let start = footer.saturating_sub(7).min(composer);
                if lines[start..composer].iter().any(|line| {
                    let line = line.trim_start_matches(|ch: char| {
                        ch.is_whitespace() || ('\u{2800}'..='\u{28ff}').contains(&ch)
                    });
                    line.starts_with("Working…") || line.starts_with("Thinking…")
                }) {
                    self.work = Some("Work in progress");
                } else if matches!(self.work, Some("Work in progress" | "Your response needed")) {
                    self.work = Some("Agent run finished");
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, terminal: &Terminal, focus: bool) {
        let font = FontId::monospace(14.0);
        let cw = ui.fonts(|fonts| fonts.glyph_width(&font, 'M'));
        let ch = 18.0;
        let size = ui.available_size();
        let cols = (((size.x - 26.0) / cw) as usize).clamp(2, 450);
        let rows = ((size.y / ch) as usize).clamp(2, 240);
        if cols != self.size.cols || rows != self.size.rows {
            self.size = Size { cols, rows };
            self.term.resize(self.size_for_term());
            self.anchor = None;
            self.cursor = None;
            if self.live {
                let _ = terminal.resize(&self.session.id, cols as u16, rows as u16);
            }
        }
        ui.horizontal_top(|ui| {
            let (rect, response) = ui.allocate_exact_size(
                vec2(cw * cols as f32, ch * rows as f32),
                Sense::click_and_drag(),
            );
            let origin = rect.min;
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_rgb(17, 20, 26));
            if focus || response.clicked() || response.drag_started() {
                response.request_focus();
            }
            let history = self.term.grid().history_size();
            let mut offset = self.term.grid().display_offset();
            if history > 0
                && ui
                    .add(
                        egui::Slider::new(&mut offset, 0..=history)
                            .vertical()
                            .show_value(false),
                    )
                    .changed()
            {
                self.term.scroll_display(Scroll::Delta(
                    offset as i32 - self.term.grid().display_offset() as i32,
                ));
            }
            if response.hovered() {
                let wheel = ui.input(|input| input.smooth_scroll_delta.y);
                if wheel.abs() >= 1.0 {
                    self.term.scroll_display(Scroll::Delta(
                        (wheel / ch).round().clamp(-20.0, 20.0) as i32,
                    ));
                }
            }
            let offset = self.term.grid().display_offset() as i32;
            if let Some(position) = response.interact_pointer_pos() {
                let col = (((position.x - origin.x) / cw) as usize).min(cols - 1);
                let row = (((position.y - origin.y) / ch) as usize).min(rows - 1);
                let point = Point::new(Line(row as i32 - offset), Column(col));
                if response.drag_started() {
                    self.anchor = Some(point);
                    self.cursor = Some(point);
                } else if response.dragged() {
                    self.cursor = Some(point);
                } else if response.clicked() {
                    self.anchor = None;
                    self.cursor = None;
                }
            }
            if response.has_focus() {
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: true,
                        },
                    )
                });
                self.input(ui, terminal);
            }
            let rendered = self.term.renderable_content();
            let mut selection = self.anchor.zip(self.cursor);
            if let Some((a, b)) = selection.as_mut() {
                if a > b {
                    std::mem::swap(a, b);
                }
            }
            for cell in rendered.display_iter {
                let pos = pos2(
                    origin.x + cw * cell.point.column.0 as f32,
                    origin.y + ch * (cell.point.line.0 + offset) as f32,
                );
                let fg = rendered_color(cell.fg, rendered.colors);
                let bg = rendered_color(cell.bg, rendered.colors);
                let (fg, bg) = if cell.flags.contains(Flags::INVERSE) {
                    (bg, fg)
                } else {
                    (fg, bg)
                };
                let bg = if selection.is_some_and(|(a, b)| cell.point >= a && cell.point <= b) {
                    Color32::from_rgb(58, 74, 105)
                } else {
                    bg
                };
                if bg != Color32::from_rgb(17, 20, 26) {
                    painter.rect_filled(Rect::from_min_size(pos, vec2(cw, ch)), 0.0, bg);
                }
                if cell.c != ' '
                    && !cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::HIDDEN)
                {
                    let mut glyph = cell.c.to_string();
                    if let Some(extra) = cell.zerowidth() {
                        glyph.extend(extra);
                    }
                    painter.text(pos, Align2::LEFT_TOP, glyph, font.clone(), fg);
                }
                if cell.flags.contains(Flags::UNDERLINE) {
                    painter.line_segment(
                        [pos + vec2(0.0, ch - 2.0), pos + vec2(cw, ch - 2.0)],
                        (1.0, fg),
                    );
                }
            }
            let point = rendered.cursor.point;
            let line = point.line.0 + offset;
            if (0..rows as i32).contains(&line) && rendered.cursor.shape != CursorShape::Hidden {
                let pos = pos2(
                    origin.x + point.column.0 as f32 * cw,
                    origin.y + line as f32 * ch,
                );
                painter.line_segment([pos, pos + vec2(0.0, ch)], (2.0, Color32::LIGHT_BLUE));
                if response.has_focus() {
                    ui.output_mut(|output| {
                        output.ime = Some(egui::output::IMEOutput {
                            rect,
                            cursor_rect: Rect::from_min_size(pos, vec2(1.0, ch)),
                        })
                    });
                    if !self.ime.is_empty() {
                        painter.text(pos, Align2::LEFT_TOP, &self.ime, font, Color32::LIGHT_BLUE);
                    }
                }
            }
        });
    }

    fn size_for_term(&self) -> Size {
        Size {
            cols: self.size.cols,
            rows: self.size.rows,
        }
    }

    fn input(&mut self, ui: &egui::Ui, terminal: &Terminal) {
        let events = ui.input(|input| input.events.clone());
        let shift = ui.input(|input| input.modifiers.shift);
        let mut data = String::new();
        let mut alt_text = None;
        for event in events {
            match event {
                egui::Event::Copy => {
                    if let Some((a, b)) = self.anchor.zip(self.cursor).filter(|_| shift) {
                        let (a, b) = if a <= b { (a, b) } else { (b, a) };
                        ui.ctx().copy_text(self.term.bounds_to_string(a, b));
                    } else {
                        data.push('\x03');
                    }
                }
                egui::Event::Cut => data.push('\x18'),
                egui::Event::Paste(text) => {
                    let text = text.replace('\x1b', "");
                    if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
                        data.push_str("\x1b[200~");
                        data.push_str(&text);
                        data.push_str("\x1b[201~");
                    } else {
                        data.push_str(&text.replace("\r\n", "\r").replace('\n', "\r"));
                    }
                }
                egui::Event::Text(text) if self.ime.is_empty() => {
                    if alt_text.take().as_deref() != Some(text.as_str()) {
                        data.push_str(&text);
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Preedit(text)) => self.ime = text,
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    self.ime.clear();
                    data.push_str(&text);
                }
                egui::Event::Ime(egui::ImeEvent::Disabled) => self.ime.clear(),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if self.ime.is_empty() => {
                    if let Some(bytes) = key_bytes(key, modifiers, *self.term.mode()) {
                        alt_text = modifiers
                            .alt
                            .then(|| bytes.strip_prefix('\x1b'))
                            .flatten()
                            .filter(|text| text.chars().count() == 1)
                            .map(str::to_owned);
                        data.push_str(&bytes);
                    }
                }
                _ => {}
            }
        }
        if self.live && !data.is_empty() {
            self.term.scroll_display(Scroll::Bottom);
            if let Err(error) = terminal.write(&self.session.id, data) {
                self.status = error;
            }
        }
    }
}

fn key_bytes(key: Key, modifiers: Modifiers, mode: TermMode) -> Option<String> {
    let final_byte = match key {
        Key::ArrowUp => 'A',
        Key::ArrowDown => 'B',
        Key::ArrowRight => 'C',
        Key::ArrowLeft => 'D',
        Key::Home => 'H',
        Key::End => 'F',
        _ => ' ',
    };
    let modifier =
        1 + u8::from(modifiers.shift) + 2 * u8::from(modifiers.alt) + 4 * u8::from(modifiers.ctrl);
    if final_byte != ' ' {
        return Some(if modifier > 1 {
            format!("\x1b[1;{modifier}{final_byte}")
        } else if mode.contains(TermMode::APP_CURSOR) {
            format!("\x1bO{final_byte}")
        } else {
            format!("\x1b[{final_byte}")
        });
    }
    let bytes = match key {
        Key::Enter => "\r",
        Key::Escape => "\x1b",
        Key::Backspace => "\x7f",
        Key::Tab if modifiers.shift => "\x1b[Z",
        Key::Tab => "\t",
        Key::Delete => "\x1b[3~",
        Key::Insert => "\x1b[2~",
        Key::PageUp => "\x1b[5~",
        Key::PageDown => "\x1b[6~",
        Key::F1 => "\x1bOP",
        Key::F2 => "\x1bOQ",
        Key::F3 => "\x1bOR",
        Key::F4 => "\x1bOS",
        Key::F5 => "\x1b[15~",
        Key::F6 => "\x1b[17~",
        Key::F7 => "\x1b[18~",
        Key::F8 => "\x1b[19~",
        Key::F9 => "\x1b[20~",
        Key::F10 => "\x1b[21~",
        Key::F11 => "\x1b[23~",
        Key::F12 => "\x1b[24~",
        _ if modifiers.ctrl => {
            let name = key.name().as_bytes();
            if let [byte @ b'A'..=b'Z'] = name {
                if matches!(key, Key::C | Key::V) {
                    return None;
                }
                let code = char::from(byte & 0x1f);
                return Some(if modifiers.alt {
                    format!("\x1b{code}")
                } else {
                    code.to_string()
                });
            }
            return None;
        }
        _ if modifiers.alt => {
            let name = key.name();
            if name.len() != 1 {
                return None;
            }
            return Some(format!(
                "\x1b{}",
                if modifiers.shift {
                    name.to_owned()
                } else {
                    name.to_ascii_lowercase()
                }
            ));
        }
        _ => return None,
    };
    Some(bytes.into())
}

fn rendered_color(value: Color, colors: &alacritty_terminal::term::color::Colors) -> Color32 {
    let index = match value {
        Color::Indexed(index) => index as usize,
        Color::Named(name) => name as usize,
        Color::Spec(rgb) => return Color32::from_rgb(rgb.r, rgb.g, rgb.b),
    };
    colors[index].map_or_else(
        || color(value),
        |rgb| Color32::from_rgb(rgb.r, rgb.g, rgb.b),
    )
}

fn color(value: Color) -> Color32 {
    let index = match value {
        Color::Spec(rgb) => return Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        Color::Named(NamedColor::Background) => return Color32::from_rgb(17, 20, 26),
        Color::Named(name) => name as usize,
        Color::Indexed(i) => i as usize,
    };
    const COLORS: [u32; 16] = [
        0x20242c, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, 0x5c6370,
        0xff7b86, 0xb7e29a, 0xf7d99f, 0x89caff, 0xe1a2f4, 0x8bdde8, 0xf0f3f9,
    ];
    match index {
        0..=15 => {
            let hex = COLORS[index];
            Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
        }
        16..=231 => {
            let n = index - 16;
            let step = |i| [0, 95, 135, 175, 215, 255][i];
            Color32::from_rgb(step(n / 36), step(n / 6 % 6), step(n % 6))
        }
        232..=255 => Color32::from_gray(8 + (index as u8 - 232) * 10),
        _ => Color32::from_rgb(220, 226, 237),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gjc_queue_navigation_keeps_terminal_modifier_encoding() {
        assert_eq!(
            key_bytes(Key::ArrowUp, Modifiers::ALT, TermMode::APP_CURSOR).as_deref(),
            Some("\x1b[1;3A")
        );
        assert_eq!(
            key_bytes(Key::Q, Modifiers::ALT, TermMode::empty()).as_deref(),
            Some("\x1bq")
        );
        assert_eq!(
            key_bytes(Key::Tab, Modifiers::SHIFT, TermMode::empty()).as_deref(),
            Some("\x1b[Z")
        );
    }
}
