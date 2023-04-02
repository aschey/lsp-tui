use crate::client::Client;
use crate::server::Server;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, EventStream};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::{Block, Borders};
use ratatui::Terminal;
use serde_json::json;
use std::io;
use std::process::Stdio;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use tokio::io::{BufReader, BufWriter, DuplexStream};
use tower_lsp::{lsp_types::*, ClientToServer, LspService};
use tui_textarea::{Input, Key, TextArea};

pub struct App {
    client: Arc<tower_lsp::Client<ClientToServer>>,
    document_version: AtomicI32,
    document_uri: Url,
    capabilities: ServerCapabilities,
}

impl App {
    pub async fn initialize() -> Self {
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
        Self {
            client: inner_client,
            capabilities,
            document_version: AtomicI32::new(0),
            document_uri: "file://temp".parse().unwrap(),
        }
    }

    pub async fn run(self) -> io::Result<()> {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        enable_raw_mode()?;
        crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;

        let mut textarea = TextArea::default();
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Crossterm Minimal Example"),
        );

        let mut events = EventStream::default();

        term.draw(|f| {
            f.render_widget(textarea.widget(), f.size());
        })?;

        self.client.initialized().await;

        self.client
            .did_open(TextDocumentItem {
                uri: self.document_uri.clone(),
                language_id: "typescript".to_owned(),
                version: self.document_version.fetch_add(1, Ordering::SeqCst),
                text: "".to_owned(),
            })
            .await;

        while let Some(Ok(event)) = events.next().await {
            match event.into() {
                Input { key: Key::Esc, .. } => break,
                input => {
                    let (old_pos, old_line) = textarea.cursor();
                    if textarea.input(input) {
                        let (new_pos, new_line) = textarea.cursor();
                        self.client
                            .did_change(DidChangeTextDocumentParams {
                                text_document: VersionedTextDocumentIdentifier {
                                    uri: self.document_uri.clone(),
                                    version: self.document_version.fetch_add(1, Ordering::SeqCst),
                                },
                                content_changes: vec![TextDocumentContentChangeEvent {
                                    range: Some(Range {
                                        start: Position {
                                            line: old_line as u32,
                                            character: old_pos as u32,
                                        },
                                        end: Position {
                                            line: new_line as u32,
                                            character: new_pos as u32,
                                        },
                                    }),
                                    text: textarea.lines().join("\n"),
                                    range_length: None,
                                }],
                            })
                            .await;
                    }
                }
            }
            term.draw(|f| {
                f.render_widget(textarea.widget(), f.size());
            })?;
        }

        disable_raw_mode()?;
        crossterm::execute!(
            term.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        term.show_cursor()?;

        let symbol_result = self
            .client
            .document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier::new(self.document_uri),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap();
        println!("Symbol result {symbol_result:?}");
        Ok(())
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
        initialization_options: Some(json!(
            {
                "tsserver": {
                    "path": "/home/aschey/.nvm/versions/node/v18.12.1/lib/tsserver.js"
                }
            }
        )),
        capabilities: ClientCapabilities {
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
