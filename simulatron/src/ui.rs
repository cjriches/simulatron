use serde_json;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use web_view::{self, Content};

use crate::keyboard::KeyMessage;

#[derive(Debug, PartialEq, Eq)]
enum InternalUICommand {
    SetChar {row: u32, col: u32, character: char},
    SetFg {row: u32, col: u32, r: u8, g: u8, b: u8},
    SetBg {row: u32, col: u32, r: u8, g: u8, b: u8},
    SetEnabled(bool),
    JoinThread,  // This is not exposed by UICommand; only this module can use it.
}

#[derive(Debug, PartialEq, Eq)]
pub struct UICommand(InternalUICommand);

#[allow(non_snake_case)]  // We're breaking method naming conventions to simulate the enum names.
impl UICommand {
    pub fn SetChar(row: u32, col: u32, character: char) -> Self {
        UICommand(InternalUICommand::SetChar {row, col, character})
    }

    pub fn SetFg(row: u32, col: u32, r: u8, g: u8, b: u8) -> Self {
        UICommand(InternalUICommand::SetFg {row, col, r, g, b})
    }

    pub fn SetBg(row: u32, col: u32, r: u8, g: u8, b: u8) -> Self {
        UICommand(InternalUICommand::SetBg {row, col, r, g, b})
    }

    pub fn SetEnabled(enabled: bool) -> Self {
        UICommand(InternalUICommand::SetEnabled(enabled))
    }

    fn JoinThread() -> Self {
        UICommand(InternalUICommand::JoinThread)
    }

    fn internal(&self) -> &InternalUICommand {
        &self.0
    }
}

pub struct UI {
    ui_tx: Sender<UICommand>,
    ui_rx: Option<Receiver<UICommand>>,
    keyboard_tx: Option<Sender<KeyMessage>>,
}

impl UI {
        pub fn new(display_tx: Sender<UICommand>,
               display_rx: Receiver<UICommand>,
               keyboard_tx: Sender<KeyMessage>) -> Self {
        UI {
            ui_tx: display_tx,
            ui_rx: Some(display_rx),
            keyboard_tx: Some(keyboard_tx),
        }
    }

    const TITLE_DISABLED: &'static str = "Simulatron 2.0 Standard Terminal [PROCESSOR HALTED]";
    const TITLE_ENABLED: &'static str = "Simulatron 2.0 Standard Terminal";

    pub fn run(&mut self) {
        // Take temporary ownership of the channels.
        // The unwrap is safe, as the only place where those variables are None
        // is within this method itself.
        let display_rx = self.ui_rx.take().unwrap();
        let keyboard_tx = self.keyboard_tx.take().unwrap();

        // The frontend gets included in the binary at compile time. However, format! must
        // wait till run-time.
        let frontend = format!(include_str!("terminal.html"),
                               css = include_str!("terminal.css"),
                               js = include_str!("terminal.js"));

        // Construct the UI. The WebView struct temporarily takes ownership of the keyboard_tx,
        // it will give it back at the end.
        // The mut is only necessary in release mode, so disable the warning if in debug mode.
        #[cfg_attr(debug_assertions, allow(unused_mut))]
        let mut wv = web_view::builder()
            .title(UI::TITLE_DISABLED)
            .content(Content::Html(&frontend))
            .size(1060, 600)
            .resizable(false)
            .user_data(keyboard_tx)
            .invoke_handler(|web_view, json| {
                // Parse the JSON message.
                let (key, ctrl, alt) = serde_json::from_str(json).ok()
                        .and_then(|value: serde_json::Value| {
                    let obj = value.as_object()?;
                    let key = obj.get("key")?.as_str()?;
                    let ctrl = obj.get("ctrl")?.as_bool()?;
                    let alt = obj.get("alt")?.as_bool()?;
                    Some((String::from(key), ctrl, alt))
                }).ok_or(web_view::Error::Custom(Box::new("Failed to parse JSON")))?;
                // Inform the keyboard controller.
                let key_message = KeyMessage::Key(&key, ctrl, alt)
                    .ok_or(web_view::Error::Custom(Box::new("Unrecognised key.")))?;
                web_view.user_data().send(key_message).unwrap();
                Ok(())
            })
            .build()
            .expect("UI failed to load.");

        // Disable right-click and select unless running in debug mode.
        #[cfg(not(debug_assertions))]
        {
            wv.eval("prevent_interaction()").unwrap();
        }

        // Set up listener thread for display changes. This thread temporarily takes ownership
        // of display_rx, it will give it back at the end.
        let wv_handle = wv.handle();
        let thread_handle = thread::spawn(move || loop {
            // Receive the next command.
            let command = display_rx.recv().unwrap();
            // Match it to the action, executing the corresponding Javascript function.
            match *command.internal() {
                InternalUICommand::SetChar {row, col, character} => {
                    // Turn spaces into nbsps so the cells are properly filled.
                    let character = if character == ' '
                        {String::from("&nbsp;")} else {character.to_string()};
                    wv_handle.dispatch(move |web_view|
                        web_view.eval(&format!("set_char({},{},'{}')", row, col, character)))
                            .unwrap();
                }
                InternalUICommand::SetFg {row, col, r, g, b} => {
                    wv_handle.dispatch(move |web_view|
                        web_view.eval(&format!("set_fg({},{},'rgb({},{},{})')",
                                               row, col, r, g, b)))
                            .unwrap();
                }
                InternalUICommand::SetBg {row, col, r, g, b} => {
                    wv_handle.dispatch(move |web_view|
                        web_view.eval(&format!("set_bg({},{},'rgb({},{},{})')",
                                               row, col, r, g, b)))
                            .unwrap();
                }
                InternalUICommand::SetEnabled(enable) => {
                    if enable {
                        wv_handle.dispatch(|web_view| {
                            web_view.eval("enable()")?;
                            web_view.set_title(UI::TITLE_ENABLED)
                        }).unwrap();
                    } else {
                        wv_handle.dispatch(|web_view| {
                            web_view.eval("disable()")?;
                            web_view.set_title(UI::TITLE_DISABLED)
                        }).unwrap();
                    }
                }
                InternalUICommand::JoinThread => {
                    return display_rx;
                }
            }
        });

        // Run the UI and wait for it to exit.
        let keyboard_tx = wv.run().unwrap();

        // Join the listener thread.
        self.ui_tx.send(UICommand::JoinThread()).unwrap();
        let display_rx = thread_handle.join().unwrap();

        // Re-acquire ownership of resources.
        self.keyboard_tx = Some(keyboard_tx);
        self.ui_rx = Some(display_rx);
    }
}
