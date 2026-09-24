//! The wrapper's main loop: child output, keyboard input, session events and drawing.

use std::ffi::OsString;
use std::io;
use std::io::Read;
use std::io::Write;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::time::Duration;
use std::time::Instant;

use ratatui::Terminal;
use ratatui::TerminalOptions;
use ratatui::Viewport;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;

use crate::circle::style::MagicStyle;
use crate::input::InputParser;
use crate::input::Token;
use crate::launch;
use crate::launch::Options;
use crate::magic::Magic;
use crate::magic::PickerKey;
use crate::magic::typed_command;
use crate::magic::typing_command;
use crate::pty::Console;
use crate::pty::Pty;
use crate::render;
use crate::screen::ChildScreen;
use crate::screen::InputModes;
use crate::session;
use crate::session::Tracker;

/// Animation frame interval.
const FRAME: Duration = Duration::from_millis(50);
/// Shortest gap between frames drawn for child output.
const MIN_FRAME: Duration = Duration::from_millis(8);
/// A synchronized update that never ends is drawn anyway after this long.
const SYNC_LIMIT: Duration = Duration::from_millis(150);
const EVENT_POLL: Duration = Duration::from_millis(120);
/// Child output is drawn once it pauses this long, so frames are not drawn half-written.
const QUIET: Duration = Duration::from_millis(4);

/// Variables describing an outer Copilot CLI process, not user configuration.
const NESTED: [&str; 6] = [
    "COPILOT_AGENT_SESSION_ID",
    "COPILOT_LOADER_PID",
    "COPILOT_RUN_APP",
    "COPILOT_CLI",
    "COPILOT_CLI_BINARY_VERSION",
    "COPILOT_CLI_RESOLVED_DIST_DIR",
];

/// Leaves the real terminal as it was, including after a panic.
const RESTORE: &str = concat!(
    "\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l",
    "\x1b[?2004l\x1b[?1004l\x1b[?1l\x1b>\x1b[0 q",
    "\x1b]110\x1b\\\x1b]111\x1b\\\x1b]104\x1b\\",
    "\x1b[?25h\x1b[?1049l"
);

/// Appends a diagnostic line to the file named by `MAGICOPILOT_LOG`, if set.
pub(crate) fn log(message: impl AsRef<str>) {
    use std::sync::Mutex;
    use std::sync::OnceLock;
    static LOG: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    let file = LOG.get_or_init(|| {
        let path = std::env::var_os("MAGICOPILOT_LOG")?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    });
    if let Some(file) = file
        && let Ok(mut file) = file.lock()
    {
        let _ = writeln!(
            file,
            "{:?} {}",
            std::time::SystemTime::now(),
            message.as_ref()
        );
    }
}

pub(crate) enum Msg {
    Input(String),
    Resize,
    Focus(bool),
    Output(Vec<u8>),
    OutputClosed,
    Exited(u32),
}

struct RestoreGuard;

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = out.write_all(RESTORE.as_bytes());
        let _ = out.flush();
    }
}

/// Runs Copilot with this process's console and no circle.
pub(crate) fn pass_through(options: &Options) -> i32 {
    let target = match launch::resolve(options.copilot.as_deref()) {
        Ok(target) => target,
        Err(err) => {
            eprintln!("magicopilot: {err}");
            return 1;
        }
    };
    match std::process::Command::new(&target.program)
        .args(&target.prefix)
        .args(&options.args)
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!(
                "magicopilot: could not start {}: {err}",
                target.program.display()
            );
            1
        }
    }
}

struct Session {
    magic: Magic,
    child: ChildScreen,
    parser: InputParser,
    to_child: Sender<Vec<u8>>,
    /// Height of the real terminal.
    rows: u16,
    region: u16,
    dirty: bool,
    /// The real terminal reported synchronized output, so frames can be drawn atomically.
    terminal_sync: bool,
}

impl Session {
    fn send(&self, bytes: impl Into<Vec<u8>>) {
        let _ = self.to_child.send(bytes.into());
    }

    fn picker_token(&mut self, token: &Token, now: Instant) {
        let keys: Vec<PickerKey> = match token {
            Token::Key(key) => match key.as_str() {
                "\x1b[A" | "\x1bOA" => vec![PickerKey::Up],
                "\x1b[B" | "\x1bOB" => vec![PickerKey::Down],
                "\x1b" => vec![PickerKey::Cancel],
                _ => Vec::new(),
            },
            Token::Text(text) => text
                .chars()
                .filter_map(|c| match c {
                    'k' => Some(PickerKey::Up),
                    'j' => Some(PickerKey::Down),
                    '\r' | '\n' => Some(PickerKey::Accept),
                    'q' => Some(PickerKey::Cancel),
                    '0'..='9' => Some(PickerKey::Digit(c as u8 - b'0')),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for key in keys {
            self.magic.picker_key(key, now);
            if self.magic.picker.is_none() {
                break;
            }
        }
        self.dirty = true;
    }

    /// Copilot's input row with the rows around it, and the cursor's index in that row.
    fn input_rows(&self) -> Option<(String, String, String, usize)> {
        let screen = self.child.screen();
        if !screen.alternate_screen() {
            return None;
        }
        let (row, col) = screen.cursor_position();
        if row == 0 {
            return None;
        }
        Some((
            self.child.row_text(row - 1),
            self.child.row_text(row),
            self.child.row_text(row + 1),
            self.child.char_index(row, col),
        ))
    }

    /// Whether Copilot's input box holds the start of a `/magic` command.
    fn typing_magic(&self) -> bool {
        self.input_rows()
            .is_some_and(|(above, line, below, caret)| typing_command(&above, &line, &below, caret))
    }

    /// Checks Copilot's input box when Enter is pressed; runs `/magic ...` locally.
    fn intercept_enter(&mut self, now: Instant) -> bool {
        let Some((above, line, below, caret)) = self.input_rows() else {
            return false;
        };
        let Some((args, count)) = typed_command(&above, &line, &below, caret) else {
            return false;
        };
        // Clear the typed command: to the end of the line, then one Backspace per character.
        let mut erase = String::from("\x1b[F");
        erase.extend(std::iter::repeat_n('\x7f', count));
        self.send(erase);
        self.magic.command(&args, self.rows, now);
        self.dirty = true;
        true
    }

    fn token(&mut self, token: Token, now: Instant) {
        if self.magic.picker.is_some() {
            return self.picker_token(&token, now);
        }
        match token {
            Token::Text(text) => {
                let mut pending = String::new();
                for c in text.chars() {
                    if c == '\r' {
                        if !pending.is_empty() {
                            self.send(std::mem::take(&mut pending));
                        }
                        if !self.intercept_enter(now) {
                            pending.push(c);
                        }
                    } else {
                        pending.push(c);
                    }
                }
                if !pending.is_empty() {
                    self.send(pending);
                }
            }
            Token::Key(key) => {
                // The real terminal's answer to the child's synchronized-output query.
                if key == "\x1b[?2026;1$y" || key == "\x1b[?2026;2$y" {
                    self.terminal_sync = true;
                }
                self.send(key);
            }
            Token::Paste(key) => self.send(key),
            Token::Mouse(mouse) => {
                if let Some(event) = mouse.shifted(self.region) {
                    self.send(event);
                }
            }
            Token::Focus(focused) => {
                if self.child.hooks().focus_reporting {
                    self.send(if focused { "\x1b[I" } else { "\x1b[O" });
                }
            }
            // Answers to the child's terminal queries.
            Token::Reply(reply) => {
                log(format!("reply to child: {reply:?}"));
                self.send(reply);
            }
        }
    }
}

pub(crate) fn run(options: &Options) -> io::Result<i32> {
    let target = launch::resolve(options.copilot.as_deref()).map_err(io::Error::other)?;
    let mut args = options.args.clone();
    let session_id = (!launch::names_session(&args)).then(launch::new_session_id);
    if let Some(id) = &session_id {
        args.push("--session-id".into());
        args.push(id.into());
    }
    let command_line = target.command_line(&args)?;
    let Ok(console) = Console::enter() else {
        return Ok(pass_through(options));
    };
    let (tx, rx) = mpsc::channel();
    console.spawn_reader(tx.clone());
    let mut out = io::stdout();
    out.write_all(b"\x1b[?1049h\x1b[H\x1b[2J\x1b[?25l")?;
    out.flush()?;
    let restore = RestoreGuard;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        let _ = out.write_all(RESTORE.as_bytes());
        let _ = out.flush();
        previous_hook(info);
    }));

    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| {
            let name = name.to_string_lossy().to_ascii_uppercase();
            !NESTED.contains(&name.as_str())
        })
        .collect();
    env.push(("MAGICOPILOT".into(), env!("CARGO_PKG_VERSION").into()));

    let (mut cols, mut rows) = console.size();
    let now = Instant::now();
    let mut magic = Magic::new(options.enabled, MagicStyle::Classic, options.animations);
    magic.set_choice(options.style);
    let region = magic.region_rows(rows, now);
    let spawned = Pty::spawn(&command_line, None, &env, (cols, rows - region))?;
    let pty = spawned.pty;
    {
        let tx = tx.clone();
        let mut output = spawned.output;
        std::thread::spawn(move || {
            let mut buf = vec![0; 1 << 16];
            loop {
                match output.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Msg::OutputClosed);
                        break;
                    }
                    Ok(read) => {
                        if tx.send(Msg::Output(buf[..read].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    {
        let tx = tx.clone();
        let process = spawned.process;
        std::thread::spawn(move || {
            let code = process.wait();
            let _ = tx.send(Msg::Exited(code));
        });
    }
    drop(tx);

    let home = session::copilot_home(&env);
    log(format!("child: {command_line}"));
    log(format!(
        "sideloaded conpty: {}",
        crate::pty::sideloaded_conpty()
    ));
    log(format!(
        "session root: {}",
        session::session_root(&home).display()
    ));
    let mut tracker = Tracker::new(session::session_root(&home), pty.pid(), session_id);
    let mut state = Session {
        magic,
        child: ChildScreen::new(rows - region, cols),
        parser: InputParser::default(),
        to_child: crate::pty::spawn_writer(spawned.input),
        rows,
        region,
        dirty: true,
        terminal_sync: false,
    };
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, cols, rows)),
        },
    )?;
    let mut modes = InputModes::default();
    let mut last_draw = Instant::now() - FRAME;
    let mut next_poll = Instant::now();
    let mut sync_since: Option<Instant> = None;
    let mut last_output = Instant::now();
    let mut exited: Option<(u32, Instant)> = None;
    let mut output_closed = false;
    let mut progress = None;
    // The greeting waits until Copilot has drawn something, so it is not missed.
    let mut greeted = !options.enabled;

    let code = loop {
        let now = Instant::now();
        let mut deadline = next_poll;
        for extra in [state.parser.deadline(), state.magic.deadline(now)]
            .into_iter()
            .flatten()
        {
            deadline = deadline.min(extra);
        }
        if state.magic.animating(now) {
            deadline = deadline.min(last_draw + FRAME);
        }
        if state.dirty {
            deadline = deadline.min((last_draw + MIN_FRAME).max(last_output + QUIET));
        }
        if exited.is_some() {
            deadline = deadline.min(now + Duration::from_millis(20));
        }
        let mut first = match rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(msg) => Some(msg),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break exited.map_or(1, |(code, _)| code as i32),
        };
        let mut resized = false;
        while let Some(msg) = first.take().or_else(|| rx.try_recv().ok()) {
            let now = Instant::now();
            match msg {
                Msg::Output(bytes) => {
                    state.child.process(&bytes);
                    let hooks = state.child.hooks_mut();
                    if !hooks.to_terminal.is_empty() {
                        // Queries are answered by the real terminal; send them without delay.
                        let relayed = std::mem::take(&mut hooks.to_terminal);
                        log(format!(
                            "to terminal: {:?}",
                            String::from_utf8_lossy(&relayed)
                        ));
                        let backend = terminal.backend_mut();
                        backend.write_all(&relayed)?;
                        backend.flush()?;
                    }
                    if !hooks.to_child.is_empty() {
                        let replies = std::mem::take(&mut hooks.to_child);
                        state.send(replies);
                    }
                    sync_since = match (state.child.hooks().sync, sync_since) {
                        (true, None) => Some(now),
                        (true, since) => since,
                        (false, _) => None,
                    };
                    state.dirty = true;
                    last_output = now;
                    if !greeted {
                        let rows = state.child.screen().size().0;
                        if (0..rows).any(|row| !state.child.row_text(row).trim().is_empty()) {
                            state.magic.greet(now);
                            greeted = true;
                        }
                    }
                }
                Msg::Input(text) => {
                    for token in state.parser.push(&text, now) {
                        state.token(token, now);
                    }
                }
                Msg::Resize => resized = true,
                Msg::Focus(focused) => state.token(Token::Focus(focused), now),
                Msg::OutputClosed => {
                    log("output closed");
                    output_closed = true;
                }
                Msg::Exited(code) => {
                    log(format!("child exited: {code}"));
                    exited = Some((code, now));
                }
            }
        }
        let now = Instant::now();
        for token in state.parser.flush(now) {
            state.token(token, now);
        }
        if now >= next_poll {
            next_poll = now + EVENT_POLL;
            for event in tracker.poll(now) {
                log(format!("event: {event:?}"));
                state.magic.handle(event, now);
                state.dirty = true;
            }
        }
        let busy = state.child.hooks().progress;
        if busy != progress {
            state.magic.progress(busy, now);
            progress = busy;
        }
        state.magic.tick(now);

        if resized {
            let (new_cols, new_rows) = console.size();
            if (new_cols, new_rows) != (cols, rows) {
                (cols, rows) = (new_cols, new_rows);
                terminal.resize(Rect::new(0, 0, cols, rows))?;
                terminal.clear()?;
                state.rows = rows;
                state.region = u16::MAX;
            }
        }
        state.magic.fit(rows, now);
        let region = state.magic.region_rows(rows, now);
        if region != state.region {
            state.region = region;
            state.child.set_size(rows - region, cols);
            pty.resize((cols, rows - region));
            state.dirty = true;
        }
        let wanted = InputModes::of(&state.child);
        if wanted != modes {
            let backend = terminal.backend_mut();
            backend.write_all(wanted.transition(&modes).as_bytes())?;
            backend.flush()?;
            modes = wanted;
        }

        let waiting_for_sync =
            sync_since.is_some_and(|since| now.duration_since(since) < SYNC_LIMIT);
        let due = if state.dirty {
            now.duration_since(last_draw) >= MIN_FRAME
                && (now.duration_since(last_output) >= QUIET
                    || now.duration_since(last_draw) >= FRAME)
        } else {
            state.magic.animating(now) && now.duration_since(last_draw) >= FRAME
        };
        if due && !waiting_for_sync {
            // Copilot's command list, open while a command is typed, cannot show `/magic`.
            let typing = state.typing_magic();
            state.magic.set_command_hint(typing);
            let magic = &state.magic;
            let child = &state.child;
            let region = state.region;
            if state.terminal_sync {
                terminal.backend_mut().write_all(b"\x1b[?2026h")?;
            }
            terminal.draw(|frame| {
                let area = frame.area();
                let region = region.min(area.height);
                let buf = frame.buffer_mut();
                render::region(magic, Rect::new(0, 0, area.width, region), buf, now);
                let child_area = Rect::new(0, region, area.width, area.height - region);
                let cursor = render::child(child, child_area, buf);
                if let Some(cursor) = cursor.filter(|_| magic.picker.is_none()) {
                    frame.set_cursor_position(cursor);
                }
            })?;
            if state.terminal_sync {
                terminal.backend_mut().write_all(b"\x1b[?2026l")?;
                terminal.backend_mut().flush()?;
            }
            last_draw = now;
            state.dirty = false;
        }
        if let Some((code, at)) = exited
            && (output_closed || now.duration_since(at) > Duration::from_millis(400))
        {
            log("closing pseudo console");
            drop(pty);
            log("closed pseudo console");
            break code as i32;
        }
    };
    // Leave the circle's screen, then keep what Copilot printed on the normal screen, as a
    // direct run does: its exit summary names the command that resumes the session.
    let output = state.child.normal_screen_output();
    drop(terminal);
    drop(restore);
    let mut out = io::stdout();
    out.write_all(&output)?;
    out.flush()?;
    Ok(code)
}
