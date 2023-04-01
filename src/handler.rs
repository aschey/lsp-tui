use std::sync::Arc;

use lsp_text::RopeExt;
use tower_lsp::lsp_types::*;
use tracing::warn;
use tree_sitter::{Query, QueryCursor};

use crate::core::{document::Document, session::Session, text::Text};

pub async fn did_open(
    session: Arc<Session>,
    params: DidOpenTextDocumentParams,
) -> anyhow::Result<()> {
    let uri = params.text_document.uri.clone();

    if let Some(document) = Document::open(session.clone(), params).await? {
        session.insert_document(uri.clone(), document)?;
    } else {
        warn!("'textDocument/didOpen' failed :: uri: {:#?}", uri);
    }

    Ok(())
}

pub async fn did_change(
    session: Arc<Session>,
    params: DidChangeTextDocumentParams,
) -> anyhow::Result<()> {
    let uri = &params.text_document.uri;
    let mut text = session.get_mut_text(uri).await?;
    *text = Text::new(params.content_changes[0].text.clone())?;
    Document::change(session.clone(), uri, &text.content).await?;
    Ok(())
}

pub async fn did_close(
    session: Arc<Session>,
    params: DidCloseTextDocumentParams,
) -> anyhow::Result<()> {
    let uri = params.text_document.uri;
    session.remove_document(&uri)?;
    let diagnostics = Default::default();
    let version = Default::default();
    session
        .client()?
        .publish_diagnostics(uri, diagnostics, version)
        .await;
    Ok(())
}

pub async fn document_symbol(
    session: Arc<Session>,
    params: DocumentSymbolParams,
) -> anyhow::Result<Option<DocumentSymbolResponse>> {
    fn make_symbol(
        uri: &Url,
        content: &ropey::Rope,
        declaration: tree_sitter::Node,
        identifier: tree_sitter::Node,
        kind: SymbolKind,
    ) -> SymbolInformation {
        let name = content.utf8_text_for_tree_sitter_node(&identifier).into();
        let range = content.tree_sitter_range_to_lsp_range(declaration.range());
        #[allow(deprecated)]
        SymbolInformation {
            name,
            kind,
            tags: Default::default(),
            deprecated: Default::default(),
            location: Location::new(uri.clone(), range),
            container_name: Default::default(),
        }
    }

    let uri = &params.text_document.uri;

    let text = session.get_text(uri).await?;
    let content = &text.content;

    let tree = session.get_tree(uri).await?;
    let tree = tree.lock().await.clone();

    let node = tree.root_node();

    let language = session.language;

    static QUERY: &str = indoc::indoc! {r"
      (function_declaration
        name: (identifier) @identifier) @function_declaration
      (lexical_declaration
        (variable_declarator
          name: (identifier) @identifier)) @class_declaration
      (variable_declaration
        (variable_declarator
          name: (identifier) @identifier)) @variable_declaration
      (class_declaration
        name: (identifier) @identifier) @class_declaration
    "};
    let query = Query::new(language, QUERY)?;
    let mut cursor = QueryCursor::new();

    let content_str = text.content.to_string();
    let matches = cursor.matches(&query, node, content_str.as_bytes());

    let mut symbols = vec![];

    for r#match in matches {
        let captures = r#match.captures.to_vec();
        if let [declaration, identifier] = captures.as_slice() {
            let declaration_node = declaration.node;
            let identifier_node = identifier.node;

            match declaration.node.kind() {
                "function_declaration" => {
                    symbols.push(make_symbol(
                        uri,
                        content,
                        declaration_node,
                        identifier_node,
                        SymbolKind::FUNCTION,
                    ));
                }
                "lexical_declaration" => {
                    symbols.push(make_symbol(
                        uri,
                        content,
                        declaration_node,
                        identifier_node,
                        SymbolKind::VARIABLE,
                    ));
                }
                "variable_declaration" => {
                    symbols.push(make_symbol(
                        uri,
                        content,
                        declaration_node,
                        identifier_node,
                        SymbolKind::VARIABLE,
                    ));
                }
                "class_declaration" => {
                    symbols.push(make_symbol(
                        uri,
                        content,
                        declaration_node,
                        identifier_node,
                        SymbolKind::VARIABLE,
                    ));
                }
                _ => {}
            }
        }
    }

    Ok(Some(DocumentSymbolResponse::Flat(symbols)))
}
