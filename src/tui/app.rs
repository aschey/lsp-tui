use super::lsp_capabilities::{Encoding, LspCapabilities};
use crate::client::Client;
use crate::server::Server;
use crate::tui::text_area::TextArea;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use elm_ui::{Message, Model, OptionalCommand};
use kaolinite::event::EventMgmt;
use kaolinite::map::CharMap;
use kaolinite::{Document, Loc, Size};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Clear, List, ListItem, ListState};
use ratatui::{Frame, Terminal};
use ropey::Rope;
use std::io::Stdout;
use std::process::Stdio;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::{cmp, io};
use tokio::io::{BufReader, BufWriter, DuplexStream};
use tower_lsp::{lsp_types::*, ClientToServer, LspService};

#[derive(Debug)]
enum LspResponse {
    Completions(Vec<String>),
}

pub struct App {
    capabilities: LspCapabilities,
    docs: Vec<Document>,
    doc_index: usize,
    completions: Vec<String>,
    lsp_client: Arc<tower_lsp::Client<ClientToServer>>,
    document_uri: Url,
    document_version: AtomicI32,
    completion_menu_state: ListState,
    show_completions: bool,
    width: usize,
    height: usize,
}

impl Model for App {
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
            Message::TermEvent(event) => match event {
                Event::Resize(width, height) => {
                    self.width = *width as usize;
                    self.height = *height as usize;
                    for doc in self.docs.iter_mut() {
                        doc.size.w = self.width;
                        doc.size.h = self.height;
                    }
                }
                Event::Key(key_event) => {
                    return Ok(self.handle_key_event(key_event));
                }
                _ => {}
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

impl App {
    pub async fn initialize() -> App {
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

        let (width, height) = terminal::size().unwrap();

        Self {
            lsp_client,
            document_uri,
            document_version,
            capabilities: capabilities.into(),
            docs: vec![Document {
                file: Rope::default(),
                lines: vec![],
                dbl_map: CharMap::default(),
                tab_map: CharMap::default(),
                loaded_to: 0,
                file_name: "".to_owned(),
                cursor: Loc::default(),
                offset: Loc::default(),
                size: Size {
                    w: width as usize,
                    h: height as usize,
                },
                char_ptr: 0,
                event_mgmt: EventMgmt::default(),
                modified: false,
                tab_width: 4,
            }],
            doc_index: 0,
            completions: vec![],
            completion_menu_state: ListState::default(),
            show_completions: false,
            width: width as usize,
            height: height as usize,
        }
    }

    fn ui(&self, f: &mut Frame<CrosstermBackend<Stdout>>) {
        const MIN_HEIGHT: usize = 1;
        let height = cmp::max(self.current_doc().len_lines(), MIN_HEIGHT) as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(height), Constraint::Min(0)].as_slice())
            .split(f.size());
        f.render_widget(
            TextArea {
                doc: self.current_doc(),
            },
            chunks[0],
        );
        let Loc {
            x: cursor_col,
            y: cursor_row,
        } = self.current_doc().cursor;
        let overlay_vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(cursor_row as u16 + 1),
                Constraint::Length(self.completions.len().min(6) as u16),
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

        if self.show_completions && !self.completions.is_empty() {
            f.render_widget(Clear, overlay);

            let list_items: Vec<_> = self
                .completions
                .iter()
                .map(|c| ListItem::new(Span::raw(c)))
                .collect();

            f.render_stateful_widget(
                List::new(list_items).style(Style::default().fg(Color::DarkGray).bg(Color::Cyan)),
                overlay,
                &mut self.completion_menu_state.clone(),
            );
        }
        let Loc { x, y } = self.current_doc().cursor;
        f.set_cursor(x as u16, y as u16);
    }

    fn current_doc(&self) -> &Document {
        &self.docs[self.doc_index]
    }

    fn current_doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.doc_index]
    }

    fn handle_key_event(&mut self, event: &KeyEvent) -> Option<elm_ui::Command> {
        let mut changes = vec![];
        let cursor = self.current_doc().cursor;
        match (event.modifiers, event.code) {
            (KeyModifiers::NONE, KeyCode::Up) => {
                self.current_doc_mut().move_up();
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                self.current_doc_mut().move_down();
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                self.current_doc_mut().move_left();
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                self.current_doc_mut().move_right();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => return Some(elm_ui::Command::quit()),
            (KeyModifiers::SHIFT | KeyModifiers::NONE, KeyCode::Char(c)) => {
                changes.extend(self.character(c));
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                changes.extend(self.character('\t'));
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if let Some(change) = self.backspace() {
                    changes.push(change);
                }
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if let Some(change) = self.enter() {
                    changes.push(change);
                }
            }
            _ => {}
        }

        self.show_completions = false;
        let new_cursor = self.current_doc().cursor;
        let mut commands = vec![];
        let mut is_trigger = false;
        if self.current_doc().cursor != cursor || !changes.is_empty() {
            if new_cursor.x > 0 {
                let previous_char = &self
                    .current_doc()
                    .line(new_cursor.y)
                    .unwrap()
                    .chars()
                    .nth(new_cursor.x - 1)
                    .unwrap();

                is_trigger = self
                    .capabilities
                    .trigger_characters
                    .iter()
                    .any(|t| t == &previous_char.to_string());
                if previous_char.is_alphanumeric() || *previous_char == '_' || is_trigger {
                    self.show_completions = true;
                }
            }

            if !changes.is_empty() {
                commands.push(self.get_change_command(changes));
            }

            if self.show_completions {
                let lsp_pos = self.get_lsp_position(&new_cursor);
                let word_under_cursor: String = self.current_doc().line(new_cursor.y).unwrap()
                    [..new_cursor.x]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect::<Vec<_>>()
                    .iter()
                    .rev()
                    .collect();

                let min_completion_length = 2;
                if !is_trigger && word_under_cursor.len() < min_completion_length {
                    self.show_completions = false;
                } else {
                    let lsp_client = self.lsp_client.clone();
                    let document_uri = self.document_uri.clone();

                    commands.push(elm_ui::Command::new_async(move |_, _| async move {
                        let completions = lsp_client
                            .completion(CompletionParams {
                                text_document_position: TextDocumentPositionParams {
                                    text_document: TextDocumentIdentifier {
                                        uri: document_uri.clone(),
                                    },
                                    position: lsp_pos,
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
            }
        }
        if !self.show_completions {
            self.completions = vec![];
        }
        Some(elm_ui::Command::simple(Message::Sequence(commands)))
    }

    fn get_change_command(&self, changes: Vec<(Range, String)>) -> elm_ui::Command {
        let lsp_client = self.lsp_client.clone();
        let document_uri = self.document_uri.clone();
        let document_version = self.document_version.fetch_add(1, Ordering::SeqCst);
        elm_ui::Command::new_async(move |_, _| async move {
            lsp_client
                .did_change(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: document_uri,
                        version: document_version,
                    },
                    content_changes: changes
                        .into_iter()
                        .map(|(range, text)| TextDocumentContentChangeEvent {
                            range: Some(range),
                            text,
                            range_length: None,
                        })
                        .collect(),
                })
                .await;

            None
        })
    }

    fn enter(&mut self) -> Option<(Range, String)> {
        if self.current_doc().loc().y != self.current_doc().len_lines() {
            // Enter pressed in the middle or end of the line
            let loc = self.current_doc().char_loc();
            let lsp_pos = self.get_lsp_position(&loc);
            self.current_doc_mut()
                .exe(kaolinite::event::Event::SplitDown(loc))
                .unwrap();
            Some((
                Range {
                    start: lsp_pos,
                    end: Position {
                        line: lsp_pos.line + 1,
                        character: 0,
                    },
                },
                "\r\n".to_owned(),
            ))
        } else {
            // Enter pressed on the empty line at the bottom of the document
            self.new_row()
        }
    }

    fn backspace(&mut self) -> Option<(Range, String)> {
        let mut c = self.current_doc().char_ptr;
        let on_first_line = self.current_doc().loc().y == 0;
        let out_of_range = self
            .current_doc()
            .out_of_range(0, self.current_doc().loc().y)
            .is_err();
        if c == 0 && !on_first_line && !out_of_range {
            // Backspace was pressed on the start of the line, move line to the top
            self.new_row();
            let mut loc = self.current_doc().char_loc();
            loc.y -= 1;
            loc.x = self.current_doc().line(loc.y).unwrap().chars().count();
            let lsp_pos = self.get_lsp_position(&loc);
            self.current_doc_mut()
                .exe(kaolinite::event::Event::SpliceUp(loc))
                .unwrap();
            return Some((
                Range {
                    start: lsp_pos,
                    end: Position {
                        line: lsp_pos.line + 1,
                        character: 0,
                    },
                },
                "".to_owned(),
            ));
        } else if c > 0 {
            // Backspace was pressed in the middle of the line, delete the character
            c -= 1;
            if let Some(line) = self.current_doc().line(self.current_doc().loc().y) {
                if let Some(ch) = line.chars().nth(c) {
                    let loc = Loc {
                        x: c,
                        y: self.current_doc().loc().y,
                    };
                    let lsp_pos = self.get_lsp_position(&loc);
                    self.current_doc_mut()
                        .exe(kaolinite::event::Event::Delete(loc, ch.to_string()))
                        .unwrap();
                    return Some((
                        Range {
                            start: lsp_pos,
                            end: Position {
                                line: lsp_pos.line,
                                character: lsp_pos.character + 1,
                            },
                        },
                        "".to_owned(),
                    ));
                }
            }
        }
        None
    }

    fn character(&mut self, ch: char) -> Vec<(Range, String)> {
        let mut changes = vec![];
        if let Some(change) = self.new_row() {
            changes.push(change);
        }

        let loc = self.current_doc().char_loc();
        let lsp_pos = self.get_lsp_position(&loc);
        self.current_doc_mut()
            .exe(kaolinite::event::Event::Insert(loc, ch.to_string()))
            .unwrap();
        changes.push((
            Range {
                start: lsp_pos,
                end: lsp_pos,
            },
            ch.to_string(),
        ));
        changes
    }

    fn new_row(&mut self) -> Option<(Range, String)> {
        if self.current_doc().loc().y == self.current_doc().len_lines() {
            let loc = self.current_doc().loc();
            self.current_doc_mut()
                .exe(kaolinite::event::Event::InsertLine(loc.y, "".to_string()))
                .unwrap();
            let lsp_pos = self.get_lsp_position(&Loc {
                x: self.current_doc().line(loc.y).unwrap().len(),
                y: loc.y,
            });
            Some((
                Range {
                    start: lsp_pos,
                    end: Position {
                        line: lsp_pos.line + 1,
                        character: 0,
                    },
                },
                "\r\n".to_string(),
            ))
        } else {
            None
        }
    }

    fn get_lsp_position(&self, loc: &Loc) -> Position {
        let new_loc = match self.capabilities.encoding {
            Encoding::Utf8 => self.current_doc().to_utf8_loc(loc),
            Encoding::Utf16 => self.current_doc().to_utf16_loc(loc),
            Encoding::Utf32 => *loc,
        };
        Position {
            line: new_loc.y as u32,
            character: new_loc.x as u32,
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
                .filter(|i| {
                    if let Some(filter_text) = &i.filter_text {
                        filter_text.starts_with(word_under_cursor)
                    } else {
                        i.label.starts_with(word_under_cursor)
                    }
                })
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
                    PositionEncodingKind::UTF32,
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
