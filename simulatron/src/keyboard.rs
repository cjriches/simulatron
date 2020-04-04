use std::sync::{Arc, Mutex, mpsc::{Receiver, Sender}};
use std::thread;

use crate::char_mapping;
use crate::cpu::INTERRUPT_KEYBOARD;

enum InternalKeyMessage {
    Key(char),
    JoinThread,  // This is not exposed by KeyMessage; only this module can use it.
}

pub struct KeyMessage(InternalKeyMessage);

#[allow(non_snake_case)]  // We're breaking method naming conventions to simulate the enum names.
impl KeyMessage {
    pub fn Key(c: char) -> Self {
        KeyMessage(InternalKeyMessage::Key(c))
    }

    fn JoinThread() -> Self {
        KeyMessage(InternalKeyMessage::JoinThread)
    }

    fn internal(&self) -> &InternalKeyMessage {
        &self.0
    }
}

pub struct KeyboardController {
    key_buffer: Arc<Mutex<u8>>,
    keyboard_tx: Sender<KeyMessage>,
    keyboard_rx: Option<Receiver<KeyMessage>>,
    interrupt_channel: Option<Sender<u32>>,
    thread_handle: Option<thread::JoinHandle<(Receiver<KeyMessage>, Sender<u32>)>>,
}

impl KeyboardController {
    pub fn new(keyboard_tx: Sender<KeyMessage>,
               keyboard_rx: Receiver<KeyMessage>,
               interrupt_channel: Sender<u32>) -> Self {
        KeyboardController {
            key_buffer: Arc::new(Mutex::new(0)),
            keyboard_tx,
            keyboard_rx: Some(keyboard_rx),
            interrupt_channel: Some(interrupt_channel),
            thread_handle: None,
        }
    }

    pub fn start(&mut self) {
        // Take temporary ownership of the channels.
        // Note that if ui works, interrupt should also, so we do a bare unwrap.
        let keyboard_rx = self.keyboard_rx.take()
            .expect("KeyboardController was already running.");
        let interrupt_channel = self.interrupt_channel.take().unwrap();

        // Make a new reference to the mutex-protected buffer.
        let key_buffer = Arc::clone(&self.key_buffer);

        // Start the listener thread.
        let thread_handle = thread::spawn(move || loop {
            // Receive the next key.
            let key_message = keyboard_rx.recv()
                .expect("Failed to receive key from UI.");
            match *key_message.internal() {
                InternalKeyMessage::Key(key) => {
                    // Record it in the buffer and send an interrupt.
                    {
                        let mut kb = key_buffer.lock()
                            .expect("Failed to acquire key_buffer lock.");
                        *kb = char_mapping::char_to_u8(key);
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

    pub fn load(&self) -> u8 {
        let kb = self.key_buffer.lock()
            .expect("Failed to acquire key_buffer lock.");
        *kb
    }
}
