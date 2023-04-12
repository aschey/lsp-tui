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
            list: List::new(list_items)
                .style(Style::default().fg(Color::DarkGray).bg(Color::Cyan))
                .highlight_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray)),
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
    completions: Vec<String>,
}

impl CompletionMenuState {
    pub fn next(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected < self.completions.len() - 1 {
                self.list_state.select(Some(selected + 1));
            } else {
                self.list_state.select(Some(0));
            }
        }
    }

    pub fn previous(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected > 0 {
                self.list_state.select(Some(selected - 1));
            } else {
                self.list_state.select(Some(self.completions.len() - 1));
            }
        }
    }

    pub fn completions(&self) -> &Vec<String> {
        &self.completions
    }

    pub fn is_empty(&self) -> bool {
        self.completions.is_empty()
    }

    pub fn set_completions(&mut self, completions: Vec<String>) {
        self.completions = completions;
        if self.completions.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }
}
