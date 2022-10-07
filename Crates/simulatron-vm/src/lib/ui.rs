use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    queue,
    style::{self, Color},
    terminal, QueueableCommand,
};
use log::info;
use std::io::{self, Stdout, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
    Arc,
};
use std::thread;

use crate::keyboard::KeyMessage;

// UI Constants.
const TITLE: &str =
    "                             Simulatron 2.0 Terminal                              ";
const TOP_BORDER: &str =
    "┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓";
const SIDE_BORDER: &str = "┃";
const EMPTY_ROW: &str =
    "                                                                                ";
const BOTTOM_BORDER: &str =
    "┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛";

// Simulatron constants.
const ROWS: u16 = 25;
const COLS: u16 = 80;
const BUF_LEN: usize = ROWS as usize * COLS as usize;

/// Commands that get sent to the UI listener thread.
#[derive(Debug, PartialEq, Eq)]
pub enum UICommand {
    SetChar {
        row: u16,
        col: u16,
        character: char,
    },
    SetFg {
        row: u16,
        col: u16,
        r: u8,
        g: u8,
        b: u8,
    },
    SetBg {
        row: u16,
        col: u16,
        r: u8,
        g: u8,
        b: u8,
    },
    CPUHalted,
}

/// The UI state.
pub struct UI {
    ui_tx: Option<Sender<UICommand>>,
    ui_rx: Receiver<UICommand>,
    keyboard_tx: Option<Sender<KeyMessage>>,
    char_buf: Vec<char>,
    fg_buf: Vec<Color>,
    bg_buf: Vec<Color>,
}

impl UI {
    /// Construct a new UI state. Nothing happens till it is run.
    pub fn new(
        ui_tx: Sender<UICommand>,
        ui_rx: Receiver<UICommand>,
        keyboard_tx: Sender<KeyMessage>,
    ) -> Self {
        Self {
            ui_tx: Some(ui_tx),
            ui_rx,
            keyboard_tx: Some(keyboard_tx),
            char_buf: vec![' '; BUF_LEN],
            fg_buf: vec![Color::from((255, 255, 255)); BUF_LEN],
            bg_buf: vec![Color::from((0, 0, 0)); BUF_LEN],
        }
    }

    /// Run the UI, blocking the current thread till it exits.
    pub fn run(&mut self) -> crossterm::Result<()> {
        info!("Initialising UI.");
        // Initial setup.
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        // Move to alternate screen and clear it.
        queue!(
            stdout,
            terminal::EnterAlternateScreen,
            style::SetForegroundColor(Color::White),
            style::SetBackgroundColor(Color::Black),
            terminal::Clear(terminal::ClearType::All),
            cursor::Hide,
            cursor::MoveTo(0, 0),
        )?;
        // Draw the empty screen including border.
        write!(stdout, "{}", TITLE)?;
        stdout.queue(cursor::MoveTo(0, 1))?;
        write!(stdout, "{}", TOP_BORDER)?;
        stdout.queue(cursor::MoveTo(0, 2))?;
        for i in 0..ROWS {
            write!(stdout, "{}", SIDE_BORDER)?;
            // Terminal "black" may not be simulatron "black".
            stdout.queue(style::SetBackgroundColor(Color::from((0, 0, 0))))?;
            write!(stdout, "{}", EMPTY_ROW)?;
            stdout.queue(style::SetBackgroundColor(Color::Black))?;
            write!(stdout, "{}", SIDE_BORDER)?;
            stdout.queue(cursor::MoveTo(0, i + 3))?;
        }
        write!(stdout, "{}", BOTTOM_BORDER)?;
        stdout.flush()?;

        // Launch the keyboard listener thread.
        let join = Arc::new(AtomicBool::new(false));
        let join1 = join.clone();
        let ui_tx = self.ui_tx.take().unwrap();
        let keyboard_tx = self.keyboard_tx.take().unwrap();

        let join_handle = thread::spawn(move || loop {
            match event::read().unwrap() {
                Event::Key(key) => {
                    // Quit on Alt+Shift+Q.
                    if key.code == KeyCode::Char('Q')
                        && key
                            .modifiers
                            .contains(KeyModifiers::union(KeyModifiers::ALT, KeyModifiers::SHIFT))
                    {
                        ui_tx.send(UICommand::CPUHalted).unwrap();
                    } else {
                        // Send the key to the keyboard controller.
                        if let Some(k) = key_to_u8(key.code) {
                            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                            let alt = key.modifiers.contains(KeyModifiers::ALT);
                            let msg = KeyMessage::Key(k, ctrl, alt);
                            keyboard_tx.send(msg).unwrap();
                        }
                    }
                }
                _ => {} // Ignore non-keyboard events.
            }
            // Check if we should join the thread.
            if join1.load(Ordering::Relaxed) {
                return (ui_tx, keyboard_tx);
            }
        });

        // Listen for UICommands.
        info!("UI online.");
        loop {
            match self.ui_rx.recv().unwrap() {
                UICommand::SetChar {
                    row,
                    col,
                    character,
                } => {
                    let index = usize::from(row * COLS + col);
                    self.char_buf[index] = character;
                    self.redraw_char(&mut stdout, col, row)?;
                }
                UICommand::SetFg { row, col, r, g, b } => {
                    let index = usize::from(row * COLS + col);
                    self.fg_buf[index] = Color::from((r, g, b));
                    self.redraw_char(&mut stdout, col, row)?;
                }
                UICommand::SetBg { row, col, r, g, b } => {
                    let index = usize::from(row * COLS + col);
                    self.bg_buf[index] = Color::from((r, g, b));
                    self.redraw_char(&mut stdout, col, row)?;
                }
                UICommand::CPUHalted => break,
            }
        }

        // Join the keyboard listener thread.
        join.store(true, Ordering::Relaxed);
        queue!(
            stdout,
            cursor::MoveTo(20, ROWS + 3),
            style::SetForegroundColor(Color::White),
            style::SetBackgroundColor(Color::DarkRed),
        )?;
        write!(stdout, "Processor halted. Press any key to exit.")?;
        stdout.flush()?;
        let (ui_tx, keyboard_tx) = join_handle.join().unwrap();
        self.ui_tx = Some(ui_tx);
        self.keyboard_tx = Some(keyboard_tx);

        // Cleanup.
        queue!(
            stdout,
            terminal::Clear(terminal::ClearType::All),
            style::ResetColor,
            cursor::Show,
            terminal::LeaveAlternateScreen,
        )?;
        stdout.flush()?;
        terminal::disable_raw_mode()?;

        info!("UI exited.");
        Ok(())
    }

    /// Redraw the given character.
    fn redraw_char(&self, stdout: &mut Stdout, col: u16, row: u16) -> crossterm::Result<()> {
        let index = usize::from(row * COLS + col);
        let fg = self.fg_buf[index];
        let bg = self.bg_buf[index];
        let character = self.char_buf[index];
        queue!(
            stdout,
            cursor::MoveTo(col + 1, row + 2), // Account for border.
            style::SetForegroundColor(fg),
            style::SetBackgroundColor(bg),
        )?;
        write!(stdout, "{}", character)?;
        stdout.flush()
    }
}

/// Try to convert a `KeyCode` to the Simulatron character set representation
/// of that key.
fn key_to_u8(key: KeyCode) -> Option<u8> {
    match key {
        // 0 is NULL
        KeyCode::F(1) => Some(1),
        KeyCode::F(2) => Some(2),
        KeyCode::F(3) => Some(3),
        KeyCode::F(4) => Some(4),
        KeyCode::F(5) => Some(5),
        KeyCode::F(6) => Some(6),
        KeyCode::F(7) => Some(7),
        KeyCode::F(8) => Some(8),
        KeyCode::F(9) => Some(9),
        KeyCode::F(10) => Some(10),
        KeyCode::F(11) => Some(11),
        KeyCode::F(12) => Some(12),
        KeyCode::Esc => Some(13),
        KeyCode::Backspace => Some(14),
        KeyCode::Enter => Some(15),
        KeyCode::Insert => Some(16),
        KeyCode::Delete => Some(17),
        KeyCode::Home => Some(18),
        KeyCode::End => Some(19),
        KeyCode::PageUp => Some(20),
        KeyCode::PageDown => Some(21),
        KeyCode::Tab => Some(22),
        KeyCode::Up => Some(23),
        KeyCode::Down => Some(24),
        KeyCode::Left => Some(25),
        KeyCode::Right => Some(26),
        // 27 is N/A
        // 28 is N/A
        // 29 is N/A
        // 30 is N/A
        KeyCode::Char(c) => match c {
            '£' => Some(31),
            ' ' => Some(32),
            '!' => Some(33),
            '"' => Some(34),
            '#' => Some(35),
            '$' => Some(36),
            '%' => Some(37),
            '&' => Some(38),
            '\'' => Some(39),
            '(' => Some(40),
            ')' => Some(41),
            '*' => Some(42),
            '+' => Some(43),
            ',' => Some(44),
            '-' => Some(45),
            '.' => Some(46),
            '/' => Some(47),
            '0' => Some(48),
            '1' => Some(49),
            '2' => Some(50),
            '3' => Some(51),
            '4' => Some(52),
            '5' => Some(53),
            '6' => Some(54),
            '7' => Some(55),
            '8' => Some(56),
            '9' => Some(57),
            ':' => Some(58),
            ';' => Some(59),
            '<' => Some(60),
            '=' => Some(61),
            '>' => Some(62),
            '?' => Some(63),
            '@' => Some(64),
            'A' => Some(65),
            'B' => Some(66),
            'C' => Some(67),
            'D' => Some(68),
            'E' => Some(69),
            'F' => Some(70),
            'G' => Some(71),
            'H' => Some(72),
            'I' => Some(73),
            'J' => Some(74),
            'K' => Some(75),
            'L' => Some(76),
            'M' => Some(77),
            'N' => Some(78),
            'O' => Some(79),
            'P' => Some(80),
            'Q' => Some(81),
            'R' => Some(82),
            'S' => Some(83),
            'T' => Some(84),
            'U' => Some(85),
            'V' => Some(86),
            'W' => Some(87),
            'X' => Some(88),
            'Y' => Some(89),
            'Z' => Some(90),
            '[' => Some(91),
            '\\' => Some(92),
            ']' => Some(93),
            '^' => Some(94),
            '_' => Some(95),
            '`' => Some(96),
            'a' => Some(97),
            'b' => Some(98),
            'c' => Some(99),
            'd' => Some(100),
            'e' => Some(101),
            'f' => Some(102),
            'g' => Some(103),
            'h' => Some(104),
            'i' => Some(105),
            'j' => Some(106),
            'k' => Some(107),
            'l' => Some(108),
            'm' => Some(109),
            'n' => Some(110),
            'o' => Some(111),
            'p' => Some(112),
            'q' => Some(113),
            'r' => Some(114),
            's' => Some(115),
            't' => Some(116),
            'u' => Some(117),
            'v' => Some(118),
            'w' => Some(119),
            'x' => Some(120),
            'y' => Some(121),
            'z' => Some(122),
            '{' => Some(123),
            '|' => Some(124),
            '}' => Some(125),
            '~' => Some(126),
            '¬' => Some(127),
            _ => None,
        },
        _ => None,
    }
}
