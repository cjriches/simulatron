use std::sync::{Arc, Mutex, mpsc::{Receiver, Sender}};
use std::thread;

use crate::cpu::INTERRUPT_KEYBOARD;

enum InternalKeyMessage {
    Key {
        key: u8,
        ctrl: bool,
        alt: bool,
    },
    JoinThread,  // This is not exposed by KeyMessage; only this module can use it.
}

pub struct KeyMessage(InternalKeyMessage);

#[allow(non_snake_case)]  // We're breaking method naming conventions to simulate the enum names.
impl KeyMessage {
    pub fn Key(key: &str, ctrl: bool, alt: bool) -> Option<Self> {
        Some(KeyMessage(InternalKeyMessage::Key{
            key: key_str_to_u8(key)?,
            ctrl,
            alt,
        }))
    }

    fn JoinThread() -> Self {
        KeyMessage(InternalKeyMessage::JoinThread)
    }

    fn internal(&self) -> &InternalKeyMessage {
        &self.0
    }
}

struct SharedData {
    key_buffer: u8,
    metadata_buffer: u8,
}

pub struct KeyboardController {
    keyboard_tx: Sender<KeyMessage>,
    keyboard_rx: Option<Receiver<KeyMessage>>,
    interrupt_channel: Option<Sender<u32>>,
    thread_handle: Option<thread::JoinHandle<(Receiver<KeyMessage>, Sender<u32>)>>,
    shared_data: Arc<Mutex<SharedData>>,
}

impl KeyboardController {
    pub fn new(keyboard_tx: Sender<KeyMessage>,
               keyboard_rx: Receiver<KeyMessage>,
               interrupt_channel: Sender<u32>) -> Self {
        KeyboardController {
            keyboard_tx,
            keyboard_rx: Some(keyboard_rx),
            interrupt_channel: Some(interrupt_channel),
            thread_handle: None,
            shared_data: Arc::new(Mutex::new(SharedData {key_buffer: 0, metadata_buffer: 0}))
        }
    }

    pub fn start(&mut self) {
        // Take temporary ownership of the channels.
        // Note that if ui works, interrupt should also, so we do a bare unwrap.
        let keyboard_rx = self.keyboard_rx.take()
            .expect("KeyboardController was already running.");
        let interrupt_channel = self.interrupt_channel.take().unwrap();

        // Start the listener thread.
        let shared_data = Arc::clone(&self.shared_data);
        let thread_handle = thread::spawn(move || loop {
            // Receive the next key.
            let key_message = keyboard_rx.recv()
                .expect("Failed to receive key from UI.");
            match *key_message.internal() {
                InternalKeyMessage::Key {key, ctrl, alt} => {
                    // Record it in the buffer and send an interrupt.
                    {
                        let mut sd = shared_data.lock().unwrap();
                        sd.key_buffer = key;
                        sd.metadata_buffer = (if ctrl {0b1} else {0}) | (if alt {0b10} else {0});
                    }
                    interrupt_channel.send(INTERRUPT_KEYBOARD)
                        .expect("Failed to send keyboard interrupt to CPU.");
                }
                InternalKeyMessage::JoinThread => {
                    // Terminate the thread.
                    return (keyboard_rx, interrupt_channel);
                }
            }
        });
        self.thread_handle = Some(thread_handle);
    }

    pub fn stop(&mut self) {
        // Join the listener thread.
        self.keyboard_tx.send(KeyMessage::JoinThread())
            .expect("Failed to send JoinThread to keyboard listener thread.");
        let thread_handle = self.thread_handle.take()
            .expect("KeyboardController was already stopped.");
        let (keyboard_rx, interrupt_channel) = thread_handle.join()
            .expect("Keyboard listener thread terminated with error.");
        // Re-acquire ownership of resources.
        self.keyboard_rx = Some(keyboard_rx);
        self.interrupt_channel = Some(interrupt_channel);
    }

    pub fn load(&self, address: u32) -> u8 {
        match address {
            0 => self.shared_data.lock().unwrap().key_buffer,
            1 => self.shared_data.lock().unwrap().metadata_buffer,
            _ => panic!("Invalid address in keyboard::load.")
        }
    }
}

fn key_str_to_u8(key: &str) -> Option<u8> {
    match key {
        // 0 is NULL
        "F1" => Some(1),
        "F2" => Some(2),
        "F3" => Some(3),
        "F4" => Some(4),
        "F5" => Some(5),
        "F6" => Some(6),
        "F7" => Some(7),
        "F8" => Some(8),
        "F9" => Some(9),
        "F10" => Some(10),
        "F11" => Some(11),
        "F12" => Some(12),
        "Escape" => Some(13),
        "Backspace" => Some(14),
        "Enter" => Some(15),
        "Insert" => Some(16),
        "Delete" => Some(17),
        "Home" => Some(18),
        "End" => Some(19),
        "PageUp" => Some(20),
        "PageDown" => Some(21),
        "Tab" => Some(22),
        "ArrowUp" => Some(23),
        "ArrowDown" => Some(24),
        "ArrowLeft" => Some(25),
        "ArrowRight" => Some(26),
        // 27 is N/A
        // 28 is N/A
        // 29 is N/A
        // 30 is N/A
        "£" => Some(31),
        " " => Some(32),
        "!" => Some(33),
        "\"" => Some(34),
        "#" => Some(35),
        "$" => Some(36),
        "%" => Some(37),
        "&" => Some(38),
        "'" => Some(39),
        "(" => Some(40),
        ")" => Some(41),
        "*" => Some(42),
        "+" => Some(43),
        "," => Some(44),
        "-" => Some(45),
        "." => Some(46),
        "/" => Some(47),
        "0" => Some(48),
        "1" => Some(49),
        "2" => Some(50),
        "3" => Some(51),
        "4" => Some(52),
        "5" => Some(53),
        "6" => Some(54),
        "7" => Some(55),
        "8" => Some(56),
        "9" => Some(57),
        ":" => Some(58),
        ";" => Some(59),
        "<" => Some(60),
        "=" => Some(61),
        ">" => Some(62),
        "?" => Some(63),
        "@" => Some(64),
        "A" => Some(65),
        "B" => Some(66),
        "C" => Some(67),
        "D" => Some(68),
        "E" => Some(69),
        "F" => Some(70),
        "G" => Some(71),
        "H" => Some(72),
        "I" => Some(73),
        "J" => Some(74),
        "K" => Some(75),
        "L" => Some(76),
        "M" => Some(77),
        "N" => Some(78),
        "O" => Some(79),
        "P" => Some(80),
        "Q" => Some(81),
        "R" => Some(82),
        "S" => Some(83),
        "T" => Some(84),
        "U" => Some(85),
        "V" => Some(86),
        "W" => Some(87),
        "X" => Some(88),
        "Y" => Some(89),
        "Z" => Some(90),
        "[" => Some(91),
        "\\" => Some(92),
        "]" => Some(93),
        "^" => Some(94),
        "_" => Some(95),
        "`" => Some(96),
        "a" => Some(97),
        "b" => Some(98),
        "c" => Some(99),
        "d" => Some(100),
        "e" => Some(101),
        "f" => Some(102),
        "g" => Some(103),
        "h" => Some(104),
        "i" => Some(105),
        "j" => Some(106),
        "k" => Some(107),
        "l" => Some(108),
        "m" => Some(109),
        "n" => Some(110),
        "o" => Some(111),
        "p" => Some(112),
        "q" => Some(113),
        "r" => Some(114),
        "s" => Some(115),
        "t" => Some(116),
        "u" => Some(117),
        "v" => Some(118),
        "w" => Some(119),
        "x" => Some(120),
        "y" => Some(121),
        "z" => Some(122),
        "{" => Some(123),
        "|" => Some(124),
        "}" => Some(125),
        "~" => Some(126),
        "¬" => Some(127),
         _  => None
    }
}
