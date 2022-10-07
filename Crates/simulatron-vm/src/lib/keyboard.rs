use log::info;
use std::sync::{
    mpsc::{Receiver, Sender},
    Arc, Mutex,
};
use std::thread;

use crate::cpu::INTERRUPT_KEYBOARD;

/// Commands that can be sent to the keyboard controller.
enum InternalKeyMessage {
    Key { key: u8, ctrl: bool, alt: bool },
    JoinThread, // This is not exposed by KeyMessage; only this module can use it.
}

/// A public wrapper that doesn't allow join thread messages.
pub struct KeyMessage(InternalKeyMessage);

#[allow(non_snake_case)] // We're breaking method naming conventions to simulate the enum names.
impl KeyMessage {
    pub fn Key(key: u8, ctrl: bool, alt: bool) -> Self {
        KeyMessage(InternalKeyMessage::Key { key, ctrl, alt })
    }

    fn JoinThread() -> Self {
        KeyMessage(InternalKeyMessage::JoinThread)
    }

    fn internal(&self) -> &InternalKeyMessage {
        &self.0
    }
}

/// Data shared between the controller and CPU threads.
struct SharedData {
    key_buffer: u8,
    metadata_buffer: u8,
}

/// A keyboard controller.
pub struct KeyboardController {
    keyboard_tx: Sender<KeyMessage>,
    keyboard_rx: Option<Receiver<KeyMessage>>,
    interrupt_tx: Option<Sender<u32>>,
    thread_handle: Option<thread::JoinHandle<(Receiver<KeyMessage>, Sender<u32>)>>,
    shared_data: Arc<Mutex<SharedData>>,
}

impl KeyboardController {
    /// Construct a new keyboard controller with the given interrupt channel
    /// and key channel.
    pub fn new(
        keyboard_tx: Sender<KeyMessage>,
        keyboard_rx: Receiver<KeyMessage>,
        interrupt_tx: Sender<u32>,
    ) -> Self {
        KeyboardController {
            keyboard_tx,
            keyboard_rx: Some(keyboard_rx),
            interrupt_tx: Some(interrupt_tx),
            thread_handle: None,
            shared_data: Arc::new(Mutex::new(SharedData {
                key_buffer: 0,
                metadata_buffer: 0,
            })),
        }
    }

    /// Start the keyboard controller thread. Panics if already running.
    pub fn start(&mut self) {
        // Take temporary ownership of the channels.
        let keyboard_rx = self
            .keyboard_rx
            .take()
            .expect("KeyboardController was already running.");
        let interrupt_channel = self.interrupt_tx.take().unwrap();
        info!("Keyboard controller starting.");

        // Start the listener thread.
        let shared_data = Arc::clone(&self.shared_data);
        let thread_handle = thread::spawn(move || loop {
            // Receive the next key.
            let key_message = keyboard_rx.recv().expect("Failed to receive key from UI.");
            match *key_message.internal() {
                InternalKeyMessage::Key { key, ctrl, alt } => {
                    // Record it in the buffer and send an interrupt.
                    let mut sd = shared_data.lock().unwrap();
                    sd.key_buffer = key;
                    sd.metadata_buffer =
                        (if ctrl { 0b1 } else { 0 }) | (if alt { 0b10 } else { 0 });
                    interrupt_channel.send(INTERRUPT_KEYBOARD).unwrap();
                }
                InternalKeyMessage::JoinThread => {
                    return (keyboard_rx, interrupt_channel);
                }
            }
        });
        self.thread_handle = Some(thread_handle);
    }

    /// Stop the keyboard controller thread. Panics if not running.
    pub fn stop(&mut self) {
        // Join the listener thread.
        self.keyboard_tx
            .send(KeyMessage::JoinThread())
            .expect("Failed to send JoinThread to keyboard listener thread.");
        let thread_handle = self
            .thread_handle
            .take()
            .expect("KeyboardController was already stopped.");
        let (keyboard_rx, interrupt_channel) = thread_handle
            .join()
            .expect("Keyboard listener thread terminated with error.");
        // Re-acquire ownership of the channels.
        self.keyboard_rx = Some(keyboard_rx);
        self.interrupt_tx = Some(interrupt_channel);
        info!("Keyboard Controller stopping.");
    }

    /// Handle a memory-mapped status request.
    pub fn load(&self, address: u32) -> u8 {
        match address {
            0 => self.shared_data.lock().unwrap().key_buffer,
            1 => self.shared_data.lock().unwrap().metadata_buffer,
            _ => unreachable!(),
        }
    }
}
