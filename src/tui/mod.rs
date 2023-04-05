use crate::client::Client;
use crate::server::Server;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, EventStream};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use lsp_positions::Offset;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Span, Spans};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Terminal;
use serde_json::json;
use std::process::Stdio;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::{cmp, io};
use tokio::io::{BufReader, BufWriter, DuplexStream};
use tokio::sync::mpsc;
use tower_lsp::{lsp_types::*, ClientToServer, LspService};
use tui_textarea::{EditKind, Input, Key, TextArea};

pub struct App<'a> {
    msg_tx: mpsc::Sender<LspMessage>,
    capabilities: ServerCapabilities,
    text_area: TextArea<'a>,
    response_rx: mpsc::Receiver<LspResponse>,
}

impl<'a> App<'a> {
    pub async fn initialize() -> App<'a> {
        let (client_service, client_socket) = LspService::new_client(Client::new);
        let inner_client = client_service.inner().server_client();

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
            inner_client.initialize(initialize_params()).await.unwrap();

        let (msg_tx, msg_rx) = mpsc::channel(32);
        let (response_tx, response_rx) = mpsc::channel(32);
        run_lsp_task(inner_client, msg_rx, response_tx);

        let text_area = TextArea::default();

        Self {
            capabilities,
            msg_tx,
            text_area,
            response_rx,
        }
    }

    pub async fn run(mut self) -> io::Result<()> {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        enable_raw_mode()?;
        crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;

        let mut events = EventStream::default();

        term.draw(|f| {
            const MIN_HEIGHT: usize = 1;
            let height = cmp::max(1, MIN_HEIGHT) as u16 + 2; // + 2 for borders
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(height), Constraint::Min(0)].as_slice())
                .split(f.size());
            f.render_widget(self.text_area.widget(), chunks[0]);
        })?;

        let mut completions = vec![];

        loop {
            tokio::select! {
                Some(Ok(event)) = events.next() => {
                    match event.into() {
                        Input { key: Key::Esc, .. } => break,
                        input => {
                            let old_cursor = self.text_area.cursor();
                            let changed = self.text_area.input(input);
                            let new_cursor = self.text_area.cursor();

                            if changed {
                                let change_event = self.get_change_event();
                                self.msg_tx
                                    .try_send(LspMessage::Change(change_event))
                                    .unwrap();
                            }

                            if changed || old_cursor != new_cursor {
                                let (row, col) = new_cursor;
                                self.msg_tx
                                    .try_send(LspMessage::Completion(Position {
                                        line: row as u32,
                                        character: self.get_lsp_position("", row, col),
                                    }))
                                    .unwrap();
                            }
                        }
                    }
                }
                Some(response) = self.response_rx.recv() => {
                    match response {
                        LspResponse::Completions(list) => {
                            completions = list.clone();
                        }
                    }
                }
            }

            term.draw(|f| {
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
                let spans: Vec<_> = completions
                    .iter()
                    .map(|c| Spans::from(Span::raw(c)))
                    .collect();
                f.render_widget(Paragraph::new(spans), overlay);
            })?;
        }

        disable_raw_mode()?;
        crossterm::execute!(
            term.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        term.show_cursor()?;

        Ok(())
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
        let incremental = match self.capabilities.text_document_sync {
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL)) => true,
            Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                ..
            })) => true,
            _ => false,
        };
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

#[derive(Debug)]
enum LspMessage {
    Change(TextDocumentContentChangeEvent),
    Completion(Position),
}

#[derive(Debug)]
enum LspResponse {
    Completions(Vec<String>),
}

fn run_lsp_task(
    lsp_client: Arc<tower_lsp::Client<ClientToServer>>,
    mut message_rx: mpsc::Receiver<LspMessage>,
    response_tx: mpsc::Sender<LspResponse>,
) {
    let document_version = AtomicI32::new(0);
    let document_uri: Url = "file://temp".parse().unwrap();
    tokio::task::spawn(async move {
        lsp_client.initialized().await;

        lsp_client
            .did_open(TextDocumentItem {
                uri: document_uri.clone(),
                language_id: "typescript".to_owned(),
                version: document_version.fetch_add(1, Ordering::SeqCst),
                text: "".to_owned(),
            })
            .await;

        while let Some(msg) = message_rx.recv().await {
            match msg {
                LspMessage::Change(event) => {
                    lsp_client
                        .did_change(DidChangeTextDocumentParams {
                            text_document: VersionedTextDocumentIdentifier {
                                uri: document_uri.clone(),
                                version: document_version.fetch_add(1, Ordering::SeqCst),
                            },
                            content_changes: vec![event],
                        })
                        .await;
                }
                LspMessage::Completion(position) => {
                    let completions = lsp_client
                        .completion(CompletionParams {
                            text_document_position: TextDocumentPositionParams {
                                text_document: TextDocumentIdentifier {
                                    uri: document_uri.clone(),
                                },
                                position,
                            },
                            work_done_progress_params: Default::default(),
                            partial_result_params: Default::default(),
                            context: Default::default(),
                        })
                        .await
                        .unwrap();
                    if let Some(completions) = completions {
                        let completion_list = match completions {
                            CompletionResponse::Array(items) => {
                                items.iter().map(|i| i.label.clone()).collect()
                            }
                            CompletionResponse::List(list) => {
                                list.items.iter().map(|i| i.label.clone()).collect()
                            }
                        };
                        response_tx
                            .send(LspResponse::Completions(completion_list))
                            .await
                            .unwrap();
                    }
                }
            }
        }
    });
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
        initialization_options: Some(json!(
            {
                "tsserver": {
                    "path": "/home/aschey/.nvm/versions/node/v18.12.1/lib/tsserver.js"
                }
            }
        )),
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
