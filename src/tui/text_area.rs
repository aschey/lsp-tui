use kaolinite::Document;
use ratatui::{
    text::Spans,
    widgets::{Paragraph, Widget},
};

pub struct TextArea<'a> {
    pub(crate) doc: &'a Document,
}

impl<'a> Widget for TextArea<'a> {
    fn render(self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        let spans: Vec<_> = (0..area.height)
            .filter_map(|line| {
                let index = line as usize + self.doc.offset.y;
                self.doc
                    .line_trim(index, self.doc.offset.x, area.width.into())
                    .map(Spans::from)
            })
            .collect();
        Paragraph::new(spans).render(area, buf);
    }
}
