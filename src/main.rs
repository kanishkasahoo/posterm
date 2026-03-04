mod action;
mod app;
mod components;
mod event;
mod highlight;
mod http;
mod state;
mod tui;
mod util;

use app::App;
use tui::Tui;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut tui = Tui::new()?;
    let initial_size = tui.size()?;

    let mut app = App::new(initial_size);
    app.run(&mut tui).await
}
