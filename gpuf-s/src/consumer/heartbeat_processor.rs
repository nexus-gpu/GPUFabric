use anyhow::{anyhow, Result};
use rdkafka::message::{Message, OwnedMessage};
use sqlx::{Pool, Postgres};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::util::{protoc};
use crate::db::stats::{insert_heartbeat, ClientDailyStats, DeviceDailyStats};
use common::format_bytes;

#[allow(dead_code)]
pub async fn start_processor(
    mut rx: mpsc::Receiver<Vec<OwnedMessage>>,
    db_pool: Pool<Postgres>,
    batch_size: usize,
    batch_timeout_secs: u64,
) -> Result<()> {
    info!(
        "Starting heartbeat processor with batch size: {}, timeout: {}s",
        batch_size, batch_timeout_secs
    );

    loop {
        match rx.recv().await {
            Some(messages) => {
                let pool = db_pool.clone();
                let message_count = messages.len();

                // Process messages in a blocking task to avoid blocking the async runtime
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    if let Err(e) = rt.block_on(process_batch(messages, pool)) {
                        error!("Error processing batch: {}", e);
                    }
                });

                debug!("Processed batch of {} messages", message_count);
            }
            None => {
                info!("No more messages to process, shutting down processor");
                break;
            }
        }
    }

    Ok(())
}

#[allow(dead_code)]
async fn process_batch(messages: Vec<OwnedMessage>, db_pool: Pool<Postgres>) -> Result<()> {
    let mut transaction = db_pool.begin().await?;

    for message in messages {
        match message.key() {
            Some(_key) => {
                // Parse the message payload
                let payload = match message.payload() {
                    Some(p) => p,
                    None => {
                        error!("Message has no payload, skipping");
                        continue;
                    }
                };

                // Log raw payload for debugging
                debug!(
                    "Raw payload: {:?}",
                    std::str::from_utf8(payload).unwrap_or("[invalid utf8]")
                );
                let cfg = bincode::config::standard()
                .with_fixed_int_encoding()
                .with_little_endian();
                // Try to deserialize as JSON first
                let (heartbeat, _): (protoc::HeartbeatMessage, _) =
                    bincode::decode_from_slice(payload, cfg).map_err(|e| anyhow!("Failed to deserialize heartbeat: {}", e))?;
                
                info!("Heartbeat received from client {} cpu_usage {}% memory_usage {}% disk_usage {}% network_up {} network_down {}", heartbeat.client_id, heartbeat.system_info.cpu_usage, heartbeat.system_info.memory_usage, heartbeat.system_info.disk_usage,  format_bytes!(heartbeat.system_info.network_tx),format_bytes!(heartbeat.system_info.network_rx));
                // Update last seen timestamp
                if let Err(e) = insert_heartbeat(
                    &mut transaction,
                    &heartbeat.client_id,
                    &heartbeat.system_info,
                    &heartbeat.devices_info,
                    heartbeat.device_memtotal_gb.try_into().unwrap(),
                    heartbeat.device_count.try_into().unwrap(),
                    heartbeat.total_tflops.try_into().unwrap(),
                    None,
                )
                .await
                {
                    error!(
                        "Failed to update heartbeat for client {:?}: {}",
                        heartbeat.client_id, e
                    );
                    continue;
                }

                if let Err(e) = ClientDailyStats::upsert(
                    &mut transaction,
                    &heartbeat.client_id,
                    Some(heartbeat.system_info.cpu_usage as f64),
                    Some(heartbeat.system_info.memory_usage as f64),
                    Some(heartbeat.system_info.disk_usage as f64),
                    Some(heartbeat.system_info.network_rx.try_into().unwrap()),
                    Some(heartbeat.system_info.network_tx.try_into().unwrap()),
                )
                .await
                {
                    error!(
                        "Failed to update client heartbeat for client {}: {}",
                        heartbeat.client_id, e
                    );
                    continue;
                }
                if let Err(e) = DeviceDailyStats::upsert_batch(
                    &mut transaction,
                    &heartbeat.client_id,
                    &heartbeat.devices_info,
                )
                .await
                {
                    error!(
                        "Failed to update device heartbeat for client {}: {}",
                        heartbeat.client_id, e
                    );
                    continue;
                }
                
                debug!(
                    "Successfully processed heartbeat for client: {}",
                    heartbeat.client_id
                );
            }
            None => {
                debug!("Received message with no key, skipping");
                continue;
            }
        }
    }

    // Commit the transaction if all updates were successful
    transaction.commit().await?;

    Ok(())
}
