use std::sync::Arc;

use tower_lsp::{jsonrpc, lsp_types::*, ClientToServer, LanguageClient};
use tracing::info;

pub struct Client {
    client: Arc<tower_lsp::Client<ClientToServer>>,
}

impl Client {
    pub fn new(client: tower_lsp::Client<ClientToServer>) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    pub fn server_client(&self) -> Arc<tower_lsp::Client<ClientToServer>> {
        self.client.clone()
    }
}

#[tower_lsp::async_trait]
impl LanguageClient for Client {
    async fn register_capability(&self, params: RegistrationParams) -> jsonrpc::Result<()> {
        info!("{params:?}");
        Ok(())
    }

    async fn log_message(&self, params: LogMessageParams) {
        info!("Log message {params:?}");
    }
}
