use web_view::{self, Content::Html};

fn main() {
    let html = include_str!("index.html");

    web_view::builder()
        .title("Embedded HTML Example")
        .content(Html(html))
        .size(1000, 500)
        .resizable(false)
        .user_data(())
        .invoke_handler(|_webview, _arg| Ok(()))
        .run()
        .expect("App exited with error.");
}
