use anyhow::Result;
use rdkafka::consumer::stream_consumer::StreamConsumer;
use rdkafka::message::OwnedMessage;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[allow(dead_code)] // Heartbeat consumer service
pub async fn start_consumer(
    consumer: Arc<StreamConsumer>,
    tx: mpsc::Sender<Vec<OwnedMessage>>,
    batch_size: usize,
) -> Result<()> {
    info!(
        "Starting heartbeat consumer with batch size: {}",
        batch_size
    );
    let mut message_buffer = Vec::with_capacity(batch_size);
    let mut last_flush = tokio::time::Instant::now();
    let flush_interval = Duration::from_secs(1);

    'consumer_loop: loop {
        match tokio::time::timeout(flush_interval, consumer.as_ref().recv()).await {
            Ok(Ok(borrowed_message)) => {
                // Convert BorrowedMessage to OwnedMessage using detach()
                let message = borrowed_message.detach();
                message_buffer.push(message);

                if message_buffer.len() >= batch_size {
                    if let Err(e) = tx.send(message_buffer.drain(..).collect()).await {
                        error!("Failed to send batch to processor: {}", e);
                        break 'consumer_loop;
                    }
                    last_flush = tokio::time::Instant::now();
                }
            }
            Ok(Err(e)) => {
                error!("Error receiving message: {}", e);
                continue;
            }
        
            Err(_) => {
                debug!("Heartbeat consumer timeout");
                if !message_buffer.is_empty() && last_flush.elapsed() >= flush_interval {
                    if let Err(e) = tx.send(message_buffer.drain(..).collect()).await {
                        error!("Failed to send batch to processor: {}", e);
                        break 'consumer_loop;
                    }
                    last_flush = tokio::time::Instant::now();
                }
            }
        }
    }
    
    if !message_buffer.is_empty() {
        if let Err(e) = tx.send(message_buffer).await {
            error!("Failed to send final batch to processor: {}", e);
        }
    }

    info!("Heartbeat consumer shutting down");
    Ok(())
}
