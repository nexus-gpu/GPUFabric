use crate::handle::{WSWorker, WorkerHandle};
use crate::util::cmd::Args;

use anyhow::Result;
use futures_util::StreamExt;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;

//use futures_util::SinkExt;
//use tokio_tungstenite::{WebSocketStream,tungstenite::protocol::Message};

impl WSWorker {
    pub async fn new(args: Args) -> Result<Self> {
        let url = "ws://example.com/ws";
        let (ws_stream, _) = connect_async(url).await?;
        let (write, read) = ws_stream.split();
        Ok(Self {
            reader: Arc::new(Mutex::new(read)),
            writer: Arc::new(Mutex::new(write)),
            args,
        })
    }
}

impl WorkerHandle for WSWorker {
    fn login(&self) -> impl Future<Output = Result<()>> + Send {
        async move { todo!() }
    }

    fn handler(&self) -> impl Future<Output = Result<()>> + Send {
        async move { todo!() }
    }

    fn model_task(&self, _get_last_models: &str) -> impl Future<Output = Result<()>> + Send {
        async move { todo!() }
    }

    fn heartbeat_task(&self) -> impl Future<Output = Result<()>> + Send {
        async move { todo!() }
    }
}
