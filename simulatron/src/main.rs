use web_view::{self, Content::Html};

fn main() {
    // Load the frontend from file (gets included in the binary at compile-time).
    let html = format!(include_str!("terminal.html"),
                       css = include_str!("terminal.css"),
                       js = include_str!("terminal.js"));

    // Construct the app.
    let mut app = web_view::builder()
        .title("Simulatron 2.0 Standard Terminal")
        .content(Html(html))
        .size(1060, 600)
        .resizable(false)
        .user_data(())
        .invoke_handler(|_webview, key| {
            println!("{}", key);
            Ok(())
        })
        .build()
        .expect("App failed to load.");

    // Disable right-click and select unless running in debug mode.
    #[cfg(not(debug_assertions))]
    {
        app.eval("disable_interaction()").expect("Failed to disable interaction.");
    }

    // Attach to the keyboard.
    app.eval("add_keyboard_listener()").expect("Failed to attach to the keyboard.");

    // Run the app.
    app.run().expect("App exited with error.");
}
