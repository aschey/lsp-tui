use client::Client;
use server::Server;
use std::{process::Stdio, time::Duration};
use tokio::io::{BufReader, BufWriter, DuplexStream};
use tower_lsp::{lsp_types::*, LspService};
use tracing::info;
mod client;
mod core;
mod handler;
mod server;

#[tokio::main]
pub async fn main() {
    tracing_subscriber::fmt::init();

    let (client_service, client_socket) = LspService::new_client(Client::new);
    let inner_client = client_service.inner().server_client();

    let local = true;
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
        tokio::spawn(tower_lsp::Server::new(stdout, stdin, client_socket).serve(client_service));
    }

    let initialize_result = inner_client.initialize(initialize_params()).await.unwrap();
    info!("Initialize result {initialize_result:?}");
    inner_client.initialized().await;
    inner_client
        .did_open(TextDocumentItem {
            uri: "file://test".parse().unwrap(),
            language_id: "typescript".to_owned(),
            version: 1,
            text: "let i = 0;".to_owned(),
        })
        .await;

    let symbol_result = inner_client
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier::new("file://test".parse().unwrap()),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();
    info!("Symbol result {symbol_result:?}");
    tokio::time::sleep(Duration::from_secs(1)).await;
}

pub fn capabilities() -> ServerCapabilities {
    let document_symbol_provider = Some(OneOf::Left(true));

    let text_document_sync = {
        let options = TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::FULL),
            ..Default::default()
        };
        Some(TextDocumentSyncCapability::Options(options))
    };

    ServerCapabilities {
        text_document_sync,
        document_symbol_provider,
        ..Default::default()
    }
}

fn start_local_server() -> (DuplexStream, DuplexStream) {
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

fn initialize_params() -> InitializeParams {
    InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
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
