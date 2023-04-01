use std::sync::Arc;

use tower_lsp::{jsonrpc, lsp_types::*, LanguageServer, ServerToClient};
use tracing::info;

use crate::{
    capabilities,
    core::{error::IntoJsonRpcError, session::Session},
};

pub struct Server {
    pub client: tower_lsp::Client<ServerToClient>,
    pub session: Arc<Session>,
}

impl Server {
    pub fn new(client: tower_lsp::Client<ServerToClient>, language: tree_sitter::Language) -> Self {
        let session = Session::new(Some(client.clone()), language);
        Server { client, session }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Server {
    async fn initialize(&self, params: InitializeParams) -> jsonrpc::Result<InitializeResult> {
        info!("server::initialize");
        *self.session.client_capabilities.write().await = Some(params.capabilities);
        let capabilities = capabilities();
        Ok(InitializeResult {
            capabilities,
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("server::initialized");
        let typ = MessageType::INFO;
        let message = "demo language server initialized!";
        self.client.log_message(typ, message).await;
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        info!("server::shutdown");
        Ok(())
    }
    // FIXME: for some reason this doesn't trigger
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        info!("server::did_open");

        let typ = MessageType::INFO;
        let message = format!("opened document: {}", params.text_document.uri.as_str());
        self.client.log_message(typ, message).await;

        let session = self.session.clone();
        crate::handler::did_open(session, params).await.unwrap();
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        info!("server::did_change");
        let session = self.session.clone();
        crate::handler::did_change(session, params).await.unwrap();
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        info!("server::did_close");
        let session = self.session.clone();
        crate::handler::did_close(session, params).await.unwrap();
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> jsonrpc::Result<Option<DocumentSymbolResponse>> {
        info!("server::document_symbol");
        let session = self.session.clone();
        let result = crate::handler::document_symbol(session, params).await;
        Ok(result.map_err(IntoJsonRpcError)?)
    }
}
