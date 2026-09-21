//! Shared fixed-screen UI for the leader and follower examples; intentionally separate from the library.
use chrono::Timelike;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{
        self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use libethersync::{MonotonicClock, Reading, TimecodeReader};
use std::{
    collections::VecDeque,
    io::{self, Write},
    time::Duration,
};

// A 20 Hz UI can visually differ by 1.5 frames at 30 fps even with perfect sync.
pub const REFRESH_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

pub struct Panel {
    pub title: &'static str,
    pub address: String,
    pub certificate: String,
    pub commands: [&'static str; 2],
    pub diagnostics: bool,
}

pub struct Terminal {
    output: io::Stdout,
    frame: Vec<u8>,
    wall_reference: Option<String>,
    previous: Vec<String>,
    size: (u16, u16),
    input: String,
    cursor: usize,
    history: VecDeque<String>,
    history_index: Option<usize>,
    draft: String,
    pub message: String,
}

impl Terminal {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        // Construct the guard before fallible setup so errors also restore the terminal.
        let mut screen = Self {
            output: io::stdout(),
            frame: Vec::with_capacity(8192),
            wall_reference: None,
            previous: Vec::new(),
            size: (0, 0),
            input: String::new(),
            cursor: 0,
            history: VecDeque::new(),
            history_index: None,
            draft: String::new(),
            message: "Ready. Type a command below; Ctrl-C or quit exits.".into(),
        };
        execute!(
            screen.output,
            EnterAlternateScreen,
            EnableBracketedPaste,
            Clear(ClearType::All)
        )?;
        Ok(screen)
    }

    /// Refresh at the next timeline boundary instead of letting a periodic poll
    /// sample a new frame almost one refresh late. Keep the cap for input/status.
    pub fn draw_live(
        &mut self,
        panel: &Panel,
        reader: &mut TimecodeReader,
        clock: MonotonicClock,
    ) -> io::Result<Duration> {
        let before = clock.now_ns();
        let wall = chrono::Local::now();
        let after = clock.now_ns();
        let sampled = before + (after - before) / 2;
        let reading = reader.read_at(sampled);
        let wall_ms = f64::from(wall.num_seconds_from_midnight()) * 1000.
            + f64::from(wall.nanosecond()) / 1e6;
        let delta = (reading.position.as_frames() / reading.format.fps() * 1000. - wall_ms
            + 43_200_000.)
            .rem_euclid(86_400_000.)
            - 43_200_000.;
        self.wall_reference = Some(format!(
            "System {}   TOD check {:+.3} ms (after tod, +1x)",
            wall.format("%H:%M:%S%.3f"),
            delta
        ));
        let boundary = reader.next_boundary_at(sampled);
        self.draw(panel, reading)?;
        Ok(boundary.map_or(REFRESH_INTERVAL, |b| {
            REFRESH_INTERVAL.min(Duration::from_nanos(
                b.local_deadline_ns.saturating_sub(clock.now_ns()),
            ))
        }))
    }

    pub fn draw(&mut self, panel: &Panel, reading: Reading) -> io::Result<()> {
        let size = terminal::size()?;
        let (width, height) = size;
        if width == 0 || height == 0 {
            return Ok(());
        }
        // Leave the last column unused to prevent wrapping/scrolling at the bottom-right cell.
        let columns = width.saturating_sub(1) as usize;
        let mut rows = vec![String::new(); height as usize];
        let center = |text: String| -> String {
            format!(
                "{}{}",
                " ".repeat(columns.saturating_sub(text.len()) / 2),
                text
            )
        };
        if height >= 12 {
            rows[0] = format!(" ETHERSYNC / {}", panel.title);
            rows[1] = format!(" {}", panel.address);
            rows[2] = format!(" {}", panel.certificate);
            let mut body = vec![
                reading.label().to_string(),
                format!(
                    "{:+.3}x   {} / {} fps{}",
                    reading.rate.as_f64(),
                    reading.format.numerator(),
                    reading.format.denominator(),
                    if reading.format.drop_frame() {
                        " DF"
                    } else {
                        ""
                    }
                ),
            ];
            if let Some(reference) = &self.wall_reference {
                body.push(reference.clone());
            }
            if panel.diagnostics {
                body.push(format!(
                    "{:?} / {:?}",
                    reading.status.connection, reading.status.synchronization
                ));
            }
            body.push(format!(
                "{:?} / {:?}   frame {:.3}",
                reading.status.source_kind,
                reading.status.source_health,
                reading.position.as_frames()
            ));
            if panel.diagnostics {
                body.push(format!(
                    "Clock uncertainty {:.3} ms   RTT {:.3} ms",
                    reading.status.uncertainty_ns / 1e6,
                    reading.status.rtt_ns as f64 / 1e6
                ));
                body.push(format!(
                    "Timeline adjustment {:+.6} frames",
                    reading.status.correction_frames
                ));
                body.push(format!(
                    "Clock offset {:+.3} ms   Drift {:+.1} ppm",
                    reading.status.offset_ns / 1e6,
                    reading.status.drift_ppm
                ));
                body.push(format!(
                    "Sample age {:.2} s   Lost packets {}",
                    reading.status.sample_age_ns as f64 / 1e9,
                    reading.status.lost_packets
                ));
            }
            let available = height as usize - 8;
            body.truncate(available);
            let middle = 3 + (available - body.len()) / 2;
            for (i, text) in body.into_iter().enumerate() {
                rows[middle + i] = center(text);
            }
            rows[height as usize - 5] = format!(" {}", panel.commands[0]);
            rows[height as usize - 4] = format!(" {}", panel.commands[1]);
        } else if height >= 3 {
            rows[0] = format!("{}  {:+.3}x", reading.label(), reading.rate.as_f64());
        }
        if height >= 2 {
            rows[height as usize - 2] = format!(" {}", self.message);
        }
        let prefix = if columns >= 2 { "> " } else { "" };
        let room = columns.saturating_sub(prefix.len());
        let start = self.cursor.saturating_sub(room.saturating_sub(1));
        rows[height as usize - 1] = format!("{prefix}{}", &self.input[start..]);
        for row in &mut rows {
            // Commands and labels are ASCII. Sanitize feedback before sending it to a terminal.
            *row = row
                .chars()
                .map(|c| {
                    if c.is_ascii() && !c.is_control() {
                        c
                    } else {
                        '?'
                    }
                })
                .take(columns)
                .collect();
        }
        // Build the entire update before touching the terminal. DEC 2026 tells a
        // supporting renderer (including Ghostty) when the completed frame is ready.
        self.frame.clear();
        queue!(self.frame, BeginSynchronizedUpdate, Hide)?;
        if self.size != size {
            queue!(self.frame, Clear(ClearType::All))?;
            self.previous.clear();
            self.size = size;
        }
        for (y, row) in rows.iter().enumerate() {
            if self.previous.get(y) == Some(row) {
                continue;
            }
            let color = if y == height as usize - 1 {
                Color::Cyan
            } else {
                Color::Reset
            };
            queue!(
                self.frame,
                MoveTo(0, y as u16),
                SetForegroundColor(color),
                Print(row),
                Clear(ClearType::UntilNewLine),
                ResetColor
            )?;
        }
        self.previous = rows;
        let x = (prefix.len() + self.cursor - start).min(columns) as u16;
        queue!(
            self.frame,
            MoveTo(x, height - 1),
            Show,
            EndSynchronizedUpdate
        )?;
        let mut output = self.output.lock();
        output.write_all(&self.frame)?;
        output.flush()
    }

    pub fn poll(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') => return Ok(Some("quit".into())),
                        KeyCode::Char('d') if self.input.is_empty() => {
                            return Ok(Some("quit".into()));
                        }
                        KeyCode::Char('u') => {
                            self.input.drain(..self.cursor);
                            self.cursor = 0;
                        }
                        KeyCode::Char('a') => self.cursor = 0,
                        KeyCode::Char('e') => self.cursor = self.input.len(),
                        _ => {}
                    }
                    return Ok(None);
                }
                match key.code {
                    KeyCode::Enter => {
                        let command = std::mem::take(&mut self.input);
                        self.cursor = 0;
                        self.history_index = None;
                        self.draft.clear();
                        if command.trim().is_empty() {
                            return Ok(None);
                        }
                        if self.history.back() != Some(&command) {
                            self.history.push_back(command.clone());
                            if self.history.len() > 32 {
                                self.history.pop_front();
                            }
                        }
                        return Ok(Some(command));
                    }
                    KeyCode::Char(c)
                        if c.is_ascii()
                            && !c.is_control()
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        self.insert(&c.to_string())
                    }
                    KeyCode::Backspace if self.cursor > 0 => {
                        self.cursor -= 1;
                        self.input.remove(self.cursor);
                    }
                    KeyCode::Delete if self.cursor < self.input.len() => {
                        self.input.remove(self.cursor);
                    }
                    KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
                    KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.len()),
                    KeyCode::Home => self.cursor = 0,
                    KeyCode::End => self.cursor = self.input.len(),
                    KeyCode::Up if !self.history.is_empty() => {
                        let index = self.history_index.map_or_else(
                            || {
                                self.draft = self.input.clone();
                                self.history.len() - 1
                            },
                            |i| i.saturating_sub(1),
                        );
                        self.history_index = Some(index);
                        self.input = self.history[index].clone();
                        self.cursor = self.input.len();
                    }
                    KeyCode::Down => {
                        if let Some(i) = self.history_index {
                            if i + 1 < self.history.len() {
                                self.history_index = Some(i + 1);
                                self.input = self.history[i + 1].clone();
                            } else {
                                self.history_index = None;
                                self.input = self.draft.clone();
                            }
                            self.cursor = self.input.len();
                        }
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => self.insert(&text),
            _ => {}
        }
        Ok(None)
    }

    fn insert(&mut self, text: &str) {
        // A paste edits the command line; pasted newlines never execute commands.
        let text: String = text
            .chars()
            .filter(|c| c.is_ascii() && !c.is_control())
            .take(256 - self.input.len())
            .collect();
        self.input.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(
            self.output,
            EndSynchronizedUpdate,
            ResetColor,
            Show,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}
