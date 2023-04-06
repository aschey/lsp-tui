mod client;
mod core;
mod handler;
mod server;
mod tui;

#[tokio::main]
pub async fn main() {
    let writer = tracing_appender::rolling::never("./logs", "log");
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .init();
    crate::tui::run().await;
}
