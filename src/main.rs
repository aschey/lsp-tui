use std::time::Duration;

use bytes::BytesMut;
use client::Client;
use futures::{SinkExt, StreamExt};
use server::Server;
use tokio::io::{split, AsyncReadExt, AsyncWriteExt};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};
use tower_lsp::{
    jsonrpc::Request,
    lsp_types::{request::Initialize, *},
    LspService,
};
use tracing::info;
mod client;
mod core;
mod handler;
mod server;

#[tokio::main]
pub async fn main() {
    tracing_subscriber::fmt::init();
    let language = tree_sitter_javascript::language();

    let (req_client, req_server) = tokio::io::duplex(1024);
    let (resp_server, resp_client) = tokio::io::duplex(1024);
    let (server_service, server_socket) =
        LspService::new_server(|client| Server::new(client, language));
    tokio::spawn(
        tower_lsp::Server::new(req_server, resp_server, server_socket).serve(server_service),
    );

    let (client_service, client_socket) = LspService::new_client(Client::new);
    let inner_client = client_service.inner().server_client();
    tokio::spawn(
        tower_lsp::Server::new(resp_client, req_client, client_socket).serve(client_service),
    );

    let initialize_result = inner_client
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    info!("Initialize result {initialize_result:?}");
    inner_client.initialized().await;
    inner_client
        .did_open(TextDocumentItem {
            uri: "local://test".parse().unwrap(),
            language_id: "javascript".to_owned(),
            version: 0,
            text: "var i = 0;".to_owned(),
        })
        .await;
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
