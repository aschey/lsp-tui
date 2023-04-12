use crossterm::event::DisableMouseCapture;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use elm_ui::Program;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;

use self::app::App;

mod app;
mod completion_menu;
mod lsp_capabilities;
mod text_area;

pub async fn run() {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Clear(ClearType::All)).unwrap();
    enable_raw_mode().unwrap();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let program = Program::new(App::initialize().await);
    program.run(&mut terminal).await;

    disable_raw_mode().unwrap();
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
    )
    .unwrap();
}
