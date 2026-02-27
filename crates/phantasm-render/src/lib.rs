use std::io::{self, Stdout, Write};

use crossterm::{
    cursor, execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use phantasm_core::World;

pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::DarkGrey,
            bg: Color::Black,
        }
    }
}

pub struct ConsoleRenderer {
    stdout: Stdout,
    width: u16,
    height: u16,
    buffer: Vec<Vec<Cell>>,
    messages: Vec<String>,
}

impl ConsoleRenderer {
    pub fn new() -> io::Result<Self> {
        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
        let (width, height) = terminal::size()?;

        let buffer = (0..height)
            .map(|_| (0..width).map(|_| Cell::default()).collect())
            .collect();

        Ok(Self {
            stdout,
            width,
            height,
            buffer,
            messages: Vec::new(),
        })
    }

    pub fn clear(&mut self) {
        for row in &mut self.buffer {
            for cell in row.iter_mut() {
                *cell = Cell::default();
            }
        }
    }

    pub fn set_cell(&mut self, x: i64, y: i64, ch: char, fg: Color, bg: Color) {
        if x >= 0 && y >= 0 && (x as u16) < self.width && (y as u16) < self.height.saturating_sub(2)
        {
            let cell = &mut self.buffer[y as usize][x as usize];
            cell.ch = ch;
            cell.fg = fg;
            cell.bg = bg;
        }
    }

    pub fn add_message(&mut self, msg: String) {
        self.messages.push(msg);
        if self.messages.len() > 5 {
            self.messages.remove(0);
        }
    }

    pub fn render_world(&mut self, world: &World) {
        self.clear();
        let entities = world.query(&["Position", "Glyph"]);
        for entity in entities {
            if let (Some(pos), Some(glyph)) =
                (world.get(entity, "Position"), world.get(entity, "Glyph"))
            {
                let x = pos.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
                let y = pos.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
                let ch = glyph
                    .get("ch")
                    .and_then(|v| v.as_str())
                    .unwrap_or(" ")
                    .chars()
                    .next()
                    .unwrap_or(' ');
                let fg = parse_color(glyph.get("fg").and_then(|v| v.as_str()).unwrap_or("White"));
                let bg = parse_color(glyph.get("bg").and_then(|v| v.as_str()).unwrap_or("Black"));
                self.set_cell(x, y, ch, fg, bg);
            }
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        queue!(self.stdout, cursor::MoveTo(0, 0))?;

        let render_height = self.height.saturating_sub(2) as usize;
        for (row_idx, row) in self.buffer.iter().enumerate() {
            if row_idx >= render_height {
                break;
            }
            for cell in row {
                queue!(
                    self.stdout,
                    SetForegroundColor(cell.fg),
                    SetBackgroundColor(cell.bg),
                    Print(cell.ch)
                )?;
            }
        }

        let status_y = self.height.saturating_sub(2);
        queue!(self.stdout, cursor::MoveTo(0, status_y), ResetColor)?;
        let status = " Phantasm Engine | WASD: move | Q: quit | Agent: localhost:9000 ".to_string();
        queue!(
            self.stdout,
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::White),
            Print(&status)
        )?;
        let pad = (self.width as usize).saturating_sub(status.len());
        if pad > 0 {
            queue!(self.stdout, Print(" ".repeat(pad)))?;
        }

        let msg_y = self.height.saturating_sub(1);
        queue!(self.stdout, cursor::MoveTo(0, msg_y), ResetColor)?;
        if let Some(msg) = self.messages.last() {
            let truncated: String = msg.chars().take(self.width as usize).collect();
            queue!(
                self.stdout,
                SetForegroundColor(Color::Cyan),
                Print(&truncated)
            )?;
            let pad = (self.width as usize).saturating_sub(truncated.len());
            if pad > 0 {
                queue!(self.stdout, Print(" ".repeat(pad)))?;
            }
        } else {
            queue!(self.stdout, Print(" ".repeat(self.width as usize)))?;
        }

        self.stdout.flush()?;
        Ok(())
    }
}

impl Drop for ConsoleRenderer {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(self.stdout, cursor::Show, LeaveAlternateScreen, ResetColor);
    }
}

pub fn parse_color(s: &str) -> Color {
    match s {
        "Black" => Color::Black,
        "Red" | "DarkRed" => Color::DarkRed,
        "Green" | "DarkGreen" => Color::DarkGreen,
        "Yellow" | "DarkYellow" => Color::DarkYellow,
        "Blue" | "DarkBlue" => Color::DarkBlue,
        "Magenta" | "DarkMagenta" => Color::DarkMagenta,
        "Cyan" | "DarkCyan" => Color::DarkCyan,
        "White" => Color::White,
        "Grey" | "Gray" | "DarkGrey" | "DarkGray" => Color::DarkGrey,
        "BrightRed" => Color::Red,
        "BrightGreen" => Color::Green,
        "BrightYellow" => Color::Yellow,
        "BrightBlue" => Color::Blue,
        "BrightMagenta" => Color::Magenta,
        "BrightCyan" => Color::Cyan,
        "BrightWhite" => Color::White,
        _ => Color::White,
    }
}
