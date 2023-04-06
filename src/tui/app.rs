use crate::client::Client;
use crate::server::Server;
use elm_ui::{Message, Model, OptionalCommand};
use lsp_positions::Offset;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Clear, List, ListItem};
use ratatui::{Frame, Terminal};
use std::io::Stdout;
use std::process::Stdio;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::{cmp, io};
use tokio::io::{BufReader, BufWriter, DuplexStream};
use tower_lsp::{lsp_types::*, ClientToServer, LspService};
use tui_textarea::{EditKind, Input, Key, TextArea};

#[derive(Debug)]
enum LspResponse {
    Completions(Vec<String>),
}

pub struct App<'a> {
    // msg_tx: mpsc::Sender<LspMessage>,
    capabilities: ServerCapabilities,
    text_area: TextArea<'a>,
    // response_rx: mpsc::Receiver<LspResponse>,
    completions: Vec<String>,
    lsp_client: Arc<tower_lsp::Client<ClientToServer>>,
    document_uri: Url,
    document_version: AtomicI32,
}

impl<'a> Model for App<'a> {
    type Writer = Terminal<CrosstermBackend<io::Stdout>>;
    type Error = io::Error;

    fn init(&mut self) -> Result<OptionalCommand, Self::Error> {
        let lsp_client = self.lsp_client.clone();
        let document_uri = self.document_uri.clone();
        let document_version = self.document_version.fetch_add(1, Ordering::SeqCst);
        Ok(Some(elm_ui::Command::new_async(move |_, _| async move {
            lsp_client.initialized().await;
            lsp_client
                .did_open(TextDocumentItem {
                    uri: document_uri.clone(),
                    language_id: "typescript".to_owned(),
                    version: document_version,
                    text: "".to_owned(),
                })
                .await;
            None
        })))
    }

    fn update(&mut self, msg: Arc<Message>) -> Result<OptionalCommand, Self::Error> {
        match msg.as_ref() {
            Message::TermEvent(event) => match event.clone().into() {
                Input { key: Key::Esc, .. } => return Ok(Some(elm_ui::Command::quit())),
                input => {
                    return Ok(self.handle_term_event(input));
                }
            },
            Message::Custom(msg) => {
                if let Some(LspResponse::Completions(completions)) = msg.downcast_ref() {
                    self.completions = completions.clone();
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn view(&self, terminal: &mut Self::Writer) -> Result<(), Self::Error> {
        terminal.draw(|f| self.ui(f))?;
        Ok(())
    }
}

impl<'a> App<'a> {
    fn ui(&self, f: &mut Frame<CrosstermBackend<Stdout>>) {
        const MIN_HEIGHT: usize = 1;
        let height = cmp::max(self.text_area.lines().len(), MIN_HEIGHT) as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(height), Constraint::Min(0)].as_slice())
            .split(f.size());
        f.render_widget(self.text_area.widget(), chunks[0]);
        let (cursor_row, cursor_col) = self.text_area.cursor();
        let overlay_vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(cursor_row as u16 + 1),
                Constraint::Length(6),
                Constraint::Min(0),
            ])
            .split(f.size())[1];
        let overlay = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(cursor_col as u16),
                Constraint::Length(20),
                Constraint::Min(0),
            ])
            .split(overlay_vertical)[1];

        f.render_widget(Clear, overlay);

        let list_items: Vec<_> = self
            .completions
            .iter()
            .map(|c| ListItem::new(Span::raw(c)))
            .collect();
        f.render_widget(
            List::new(list_items).style(Style::default().fg(Color::DarkGray).bg(Color::Cyan)),
            overlay,
        );
    }

    fn handle_term_event(&mut self, input: Input) -> Option<elm_ui::Command> {
        let old_cursor = self.text_area.cursor();
        let changed = self.text_area.input(input);
        let new_cursor = self.text_area.cursor();
        let mut commands: Vec<elm_ui::Command> = vec![];
        if changed {
            let change_event = self.get_change_event();

            let lsp_client = self.lsp_client.clone();
            let document_uri = self.document_uri.clone();
            let document_version = self.document_version.fetch_add(1, Ordering::SeqCst);
            commands.push(elm_ui::Command::new_async(move |_, _| async move {
                lsp_client
                    .did_change(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: document_uri,
                            version: document_version,
                        },
                        content_changes: vec![change_event],
                    })
                    .await;

                None
            }));
        }
        if changed || old_cursor != new_cursor {
            let (row, col) = new_cursor;
            let lsp_col = self.get_lsp_position("", row, col);
            let word_under_cursor: String = self.text_area.lines()[row][..col]
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<Vec<_>>()
                .iter()
                .rev()
                .collect();

            let lsp_client = self.lsp_client.clone();
            let document_uri = self.document_uri.clone();

            commands.push(elm_ui::Command::new_async(move |_, _| async move {
                let completions = lsp_client
                    .completion(CompletionParams {
                        text_document_position: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: document_uri.clone(),
                            },
                            position: Position {
                                line: row as u32,
                                character: lsp_col,
                            },
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        context: Default::default(),
                    })
                    .await
                    .unwrap();
                if let Some(completions) = completions {
                    return Some(Message::custom(LspResponse::Completions(
                        handle_completion_response(completions, &word_under_cursor),
                    )));
                }

                None
            }));
        }
        Some(elm_ui::Command::simple(Message::Sequence(commands)))
    }

    pub async fn initialize() -> App<'a> {
        let (client_service, client_socket) = LspService::new_client(Client::new);
        let lsp_client = client_service.inner().server_client();
        let local = false;
        if local {
            let (in_stream, out_stream) = start_local_server();
            tokio::spawn(
                tower_lsp::Server::new(out_stream, in_stream, client_socket).serve(client_service),
            );
        } else {
            let process = tokio::process::Command::new("typescript-language-server")
                .arg("--stdio")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let stdin = BufWriter::new(process.stdin.unwrap());
            let stdout = BufReader::new(process.stdout.unwrap());
            tokio::spawn(
                tower_lsp::Server::new(stdout, stdin, client_socket).serve(client_service),
            );
        }

        let InitializeResult { capabilities, .. } =
            lsp_client.initialize(initialize_params()).await.unwrap();

        let document_version = AtomicI32::new(0);
        let document_uri: Url = "file://temp".parse().unwrap();

        let text_area = TextArea::default();
        Self {
            lsp_client,
            document_uri,
            document_version,
            capabilities,
            text_area,
            completions: vec![],
        }
    }

    fn get_lsp_position(&self, extra: &str, row: usize, col: usize) -> u32 {
        let offset = Offset::all_chars(&format!("{0}{extra}", &self.text_area.lines()[row])[..col])
            .last()
            .unwrap();
        if self.capabilities.position_encoding == Some(PositionEncodingKind::UTF8) {
            offset.utf8_offset as u32
        } else {
            offset.utf16_offset as u32
        }
    }

    fn get_change_event(&self) -> TextDocumentContentChangeEvent {
        let incremental = matches!(
            self.capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(
                TextDocumentSyncKind::INCREMENTAL
            )) | Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    change: Some(TextDocumentSyncKind::INCREMENTAL),
                    ..
                }
            ))
        );
        if !incremental {
            return TextDocumentContentChangeEvent {
                text: self.text_area.lines().join("\r\n"),
                range: None,
                range_length: None,
            };
        }
        let last_edit = self.text_area.edits().back().unwrap();
        let (before_row, before_col) = last_edit.cursor_before();
        let (after_row, after_col) = last_edit.cursor_after();
        match last_edit.kind() {
            EditKind::InsertChar(c, i) => TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: before_row as u32,
                        character: self.get_lsp_position("", before_row, *i),
                    },
                    end: Position {
                        line: after_row as u32,
                        character: self.get_lsp_position("", after_row, *i),
                    },
                }),
                text: c.to_string(),
                range_length: None,
            },
            EditKind::DeleteChar(c, _) => TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: after_row as u32,
                        character: self.get_lsp_position("", after_row, after_col),
                    },
                    end: Position {
                        line: before_row as u32,
                        character: self.get_lsp_position(&c.to_string(), before_row, before_col),
                    },
                }),
                text: "".to_owned(),
                range_length: None,
            },
            EditKind::InsertNewline(i) => TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: before_row as u32,
                        character: self.get_lsp_position("", before_row, *i),
                    },
                    end: Position {
                        line: after_row as u32,
                        character: 0,
                    },
                }),
                text: "\r\n".to_owned(),
                range_length: None,
            },
            EditKind::DeleteNewline(_) => TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: after_row as u32,
                        character: self.get_lsp_position(
                            "",
                            after_row,
                            self.text_area.lines()[after_row].len(),
                        ),
                    },
                    end: Position {
                        line: before_row as u32,
                        character: 0,
                    },
                }),
                text: "".to_owned(),
                range_length: None,
            },
            EditKind::Insert(s, i) => TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: before_row as u32,
                        character: self.get_lsp_position("", before_row, *i),
                    },
                    end: Position {
                        line: after_row as u32,
                        character: self.get_lsp_position("", after_row, *i),
                    },
                }),
                text: s.to_owned(),
                range_length: None,
            },
            EditKind::Remove(s, i) => TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: before_row as u32,
                        character: self.get_lsp_position(s, before_row, *i),
                    },
                    end: Position {
                        line: after_row as u32,
                        character: self.get_lsp_position("", after_row, *i),
                    },
                }),
                text: "".to_owned(),
                range_length: None,
            },
        }
    }
}

fn handle_completion_response(
    completions: CompletionResponse,
    word_under_cursor: &str,
) -> Vec<String> {
    match completions {
        CompletionResponse::Array(items) => {
            let mut filtered: Vec<_> = items
                .iter()
                .filter(|i| i.label.starts_with(word_under_cursor))
                .collect();
            filtered.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            filtered.into_iter().map(|i| i.label.clone()).collect()
        }
        CompletionResponse::List(list) => {
            let mut filtered: Vec<_> = list
                .items
                .iter()
                .filter(|i| i.label.starts_with(word_under_cursor))
                .collect();
            filtered.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            filtered.into_iter().map(|i| i.label.clone()).collect()
        }
    }
}

pub fn start_local_server() -> (DuplexStream, DuplexStream) {
    let language = tree_sitter_javascript::language();
    let (req_client, req_server) = tokio::io::duplex(1024);
    let (resp_server, resp_client) = tokio::io::duplex(1024);
    let (server_service, server_socket) =
        LspService::new_server(|client| Server::new(client, language));
    tokio::spawn(
        tower_lsp::Server::new(req_server, resp_server, server_socket).serve(server_service),
    );
    (req_client, resp_client)
}

pub fn initialize_params() -> InitializeParams {
    InitializeParams {
        // initialization_options: Some(json!({
        //     "tsserver": {
        //         "path": "/home/aschey/.nvm/versions/node/v18.12.1/lib/tsserver.js"
        //     }
        // })),
        capabilities: ClientCapabilities {
            general: Some(GeneralClientCapabilities {
                position_encodings: Some(vec![
                    PositionEncodingKind::UTF8,
                    PositionEncodingKind::UTF16,
                ]),
                ..Default::default()
            }),
            text_document: Some(TextDocumentClientCapabilities {
                synchronization: Some(TextDocumentSyncClientCapabilities {
                    dynamic_registration: Some(true),
                    will_save: Some(false),
                    will_save_wait_until: Some(false),
                    did_save: Some(false),
                }),
                document_symbol: Some(DocumentSymbolClientCapabilities {
                    dynamic_registration: Some(true),
                    hierarchical_document_symbol_support: Some(true),
                    tag_support: Some(TagSupport {
                        value_set: vec![SymbolTag::DEPRECATED],
                    }),
                    symbol_kind: Some(SymbolKindCapability {
                        value_set: Some(vec![
                            SymbolKind::FILE,
                            SymbolKind::MODULE,
                            SymbolKind::NAMESPACE,
                            SymbolKind::PACKAGE,
                            SymbolKind::CLASS,
                            SymbolKind::METHOD,
                            SymbolKind::PROPERTY,
                            SymbolKind::FIELD,
                            SymbolKind::CONSTRUCTOR,
                            SymbolKind::ENUM,
                            SymbolKind::INTERFACE,
                            SymbolKind::FUNCTION,
                            SymbolKind::VARIABLE,
                            SymbolKind::CONSTANT,
                            SymbolKind::STRING,
                            SymbolKind::NUMBER,
                            SymbolKind::BOOLEAN,
                            SymbolKind::ARRAY,
                            SymbolKind::OBJECT,
                            SymbolKind::KEY,
                            SymbolKind::NULL,
                            SymbolKind::ENUM_MEMBER,
                            SymbolKind::STRUCT,
                            SymbolKind::EVENT,
                            SymbolKind::OPERATOR,
                            SymbolKind::TYPE_PARAMETER,
                        ]),
                    }),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}
