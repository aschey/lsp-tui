use kaolinite::Document;
use ratatui::widgets::{Paragraph, Widget};

use super::highlight::highlight;

pub struct TextArea<'a> {
    pub(crate) doc: &'a Document,
}

impl<'a> Widget for TextArea<'a> {
    fn render(self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        let text = highlight(self.doc.rope(), 0, 0);
        Paragraph::new(text).render(area, buf);
    }
}
