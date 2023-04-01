use super::document::Document;

pub struct Text {
    pub content: ropey::Rope,
}

impl Text {
    pub fn new(text: impl AsRef<str>) -> anyhow::Result<Self> {
        let text = text.as_ref();
        let content = ropey::Rope::from_str(text);
        Ok(Text { content })
    }
}

impl From<Document> for Text {
    fn from(value: Document) -> Self {
        value.text()
    }
}
