pub mod heartbeat_consumer;
pub mod heartbeat_processor;

use anyhow::Result;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::stream_consumer::StreamConsumer;
use rdkafka::consumer::Consumer;
use rdkafka::message::OwnedMessage;
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::error;

#[allow(dead_code)] // Consumer service management
pub async fn start_consumer_services(
    bootstrap_servers: &str,
    group_id: &str,
    topic: &str,
    db_pool: Pool<Postgres>,
    batch_size: usize,
    batch_timeout_secs: u64,
) -> Result<()> {
    // Create Kafka consumer with Arc for shared ownership
    let consumer: Arc<StreamConsumer> = Arc::new(
        ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("group.id", group_id)
            .set("enable.partition.eof", "false")
            .set("enable.auto.commit", "false")
            .set("session.timeout.ms", "30000") // Increase session timeout
            .set("max.poll.interval.ms", "300000") // Increase max poll interval
            .set("fetch.max.bytes", "1048576") // Max bytes per fetch
            .set("max.partition.fetch.bytes", "1048576") // Max bytes per partition fetch
            .create()?,
    );

    // Subscribe to the topic
    consumer.subscribe(&[topic])?;

    // Create channel for batching
    let (tx, rx) = mpsc::channel::<Vec<OwnedMessage>>(32);

    // Start the processor
    let processor_handle = tokio::spawn(heartbeat_processor::start_processor(
        rx,
        db_pool.clone(),
        batch_size,
        batch_timeout_secs,
    ));

    // Clone the Arc for the consumer task
    let consumer_clone = consumer.clone();

    // Start the consumer
    let consumer_handle = tokio::spawn(heartbeat_consumer::start_consumer(
        consumer_clone,
        tx,
        batch_size,
    ));

    // Wait for either task to complete
    tokio::select! {
        res = consumer_handle => {
            if let Err(e) = res {
                error!("Consumer task failed: {}", e);
            }
        }
        res = processor_handle => {
            if let Err(e) = res {
                error!("Processor task failed: {}", e);
            }
        }
    }

    Ok(())
}
