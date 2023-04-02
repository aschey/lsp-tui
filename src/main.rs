mod client;
mod core;
mod handler;
mod server;
mod tui;

#[tokio::main]
pub async fn main() {
    crate::tui::App::initialize().await.run().await.unwrap();
}
