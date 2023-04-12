use tower_lsp::lsp_types::*;

pub enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

pub struct LspCapabilities {
    pub trigger_characters: Vec<String>,
    pub encoding: Encoding,
}

impl From<ServerCapabilities> for LspCapabilities {
    fn from(capabilities: ServerCapabilities) -> Self {
        Self {
            trigger_characters: capabilities
                .completion_provider
                .map(|p| p.trigger_characters.unwrap_or_default())
                .unwrap_or_default(),
            encoding: if capabilities.position_encoding == Some(PositionEncodingKind::UTF8) {
                Encoding::Utf8
            } else if capabilities.position_encoding == Some(PositionEncodingKind::UTF32) {
                Encoding::Utf32
            } else {
                Encoding::Utf16
            },
        }
    }
}
