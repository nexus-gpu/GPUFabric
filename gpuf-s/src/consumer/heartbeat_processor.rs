use anyhow::Result;
use chrono::{TimeZone, Utc};
use rdkafka::message::Timestamp;
use rdkafka::message::{Message, OwnedMessage};
use sqlx::{Pool, Postgres};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::db::stats::{insert_heartbeat, ClientDailyStats, DeviceDailyStats};
use crate::util::protoc;
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

                if let Err(e) = process_batch(messages, pool).await {
                    error!("Error processing batch: {}", e);
                }

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
    for message in messages {
        match message.key() {
            Some(_key) => {
                let event_ts = match message.timestamp() {
                    Timestamp::NotAvailable => Utc::now(),
                    Timestamp::CreateTime(ms) | Timestamp::LogAppendTime(ms) => Utc
                        .timestamp_millis_opt(ms)
                        .single()
                        .unwrap_or_else(Utc::now),
                };

                // Parse the message payload
                let payload = match message.payload() {
                    Some(p) => p,
                    None => {
                        error!("Message has no payload, skipping");
                        continue;
                    }
                };

                debug!("Heartbeat payload received ({} bytes)", payload.len());
                let cfg = bincode::config::standard()
                    .with_fixed_int_encoding()
                    .with_little_endian();
                // Try to deserialize as JSON first
                let (heartbeat, _): (protoc::HeartbeatMessage, _) =
                    match bincode::decode_from_slice(payload, cfg) {
                        Ok(v) => v,
                        Err(e) => {
                            error!("Failed to deserialize heartbeat: {}", e);
                            continue;
                        }
                    };

                let mut transaction = match db_pool.begin().await {
                    Ok(tx) => tx,
                    Err(e) => {
                        error!("Failed to start DB transaction: {}", e);
                        continue;
                    }
                };

                info!("Heartbeat received from client {} total_tflops {} cpu_usage {}% memory_usage {}% disk_usage {}% network_up {} network_down {}", heartbeat.client_id.log_label(), heartbeat.total_tflops, heartbeat.system_info.cpu_usage, heartbeat.system_info.memory_usage, heartbeat.system_info.disk_usage, format_bytes!(heartbeat.system_info.network_tx), format_bytes!(heartbeat.system_info.network_rx));
                // Update last seen timestamp with safe type conversion
                if let Err(e) = insert_heartbeat(
                    &mut transaction,
                    &heartbeat.client_id,
                    &heartbeat.system_info,
                    &heartbeat.devices_info,
                    heartbeat.device_memtotal_gb.try_into().unwrap_or(0),
                    heartbeat.device_count.try_into().unwrap_or(0),
                    heartbeat.total_tflops.try_into().unwrap_or(0),
                    Some(event_ts),
                )
                .await
                {
                    error!(
                        "Failed to update heartbeat for client {}: {}",
                        heartbeat.client_id.log_label(),
                        e
                    );
                    let _ = transaction.rollback().await;
                    continue;
                }

                if let Err(e) = ClientDailyStats::upsert(
                    &mut transaction,
                    &heartbeat.client_id,
                    Some(heartbeat.system_info.cpu_usage as f64),
                    Some(heartbeat.system_info.memory_usage as f64),
                    Some(heartbeat.system_info.disk_usage as f64),
                    Some(heartbeat.system_info.network_rx.try_into().unwrap_or(0)),
                    Some(heartbeat.system_info.network_tx.try_into().unwrap_or(0)),
                    event_ts,
                )
                .await
                {
                    error!(
                        "Failed to update client heartbeat for client {}: {}",
                        heartbeat.client_id.log_label(),
                        e
                    );
                    let _ = transaction.rollback().await;
                    continue;
                }
                if let Err(e) = DeviceDailyStats::upsert_batch(
                    &mut transaction,
                    &heartbeat.client_id,
                    &heartbeat.devices_info,
                    event_ts,
                )
                .await
                {
                    error!(
                        "Failed to update device heartbeat for client {}: {}",
                        heartbeat.client_id.log_label(),
                        e
                    );
                    let _ = transaction.rollback().await;
                    continue;
                }

                if let Err(e) = transaction.commit().await {
                    error!(
                        "Failed to commit transaction for client {}: {}",
                        heartbeat.client_id.log_label(),
                        e
                    );
                    continue;
                }

                debug!(
                    "Successfully processed heartbeat for client: {}",
                    heartbeat.client_id.log_label()
                );
            }
            None => {
                debug!("Received message with no key, skipping");
                continue;
            }
        }
    }

    Ok(())
}
