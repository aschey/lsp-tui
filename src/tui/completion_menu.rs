use kaolinite::Loc;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Clear, List, ListItem, ListState, StatefulWidget, Widget},
};

pub struct CompletionMenu<'a> {
    list: List<'a>,
    num_items: usize,
    cursor: Loc,
}

impl<'a> CompletionMenu<'a> {
    pub fn new(items: &'a [String], cursor: Loc) -> Self {
        let num_items = items.len();
        let list_items: Vec<_> = items.iter().map(|c| ListItem::new(Span::raw(c))).collect();
        Self {
            num_items,
            cursor,
            list: List::new(list_items).style(Style::default().fg(Color::DarkGray).bg(Color::Cyan)),
        }
    }
}

impl<'a> StatefulWidget for CompletionMenu<'a> {
    type State = CompletionMenuState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let overlay_vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(self.cursor.y as u16 + 1),
                Constraint::Length(self.num_items.min(6) as u16),
                Constraint::Min(0),
            ])
            .split(area)[1];
        let overlay = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.cursor.x as u16),
                Constraint::Length(20),
                Constraint::Min(0),
            ])
            .split(overlay_vertical)[1];

        Clear.render(overlay, buf);
        StatefulWidget::render(self.list, overlay, buf, &mut state.list_state);
    }
}

#[derive(Default, Clone)]
pub struct CompletionMenuState {
    list_state: ListState,
}
