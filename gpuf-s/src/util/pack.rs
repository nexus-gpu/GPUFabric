use bytes::BytesMut;
use std::sync::Arc;
use tokio::sync::Mutex;

// Buffer pool structure
#[derive(Clone)]
pub struct BufferPool {
    pool: Arc<Mutex<Vec<BytesMut>>>,
    buffer_size: usize,
}

impl BufferPool {
    // Create a new buffer pool
    pub fn new(buffer_size: usize, initial_capacity: usize) -> Self {
        let mut pool = Vec::with_capacity(initial_capacity);
        for _ in 0..initial_capacity {
            pool.push(BytesMut::with_capacity(buffer_size));
        }

        BufferPool {
            pool: Arc::new(Mutex::new(pool)),
            buffer_size,
        }
    }

    // Get a buffer from the pool
    pub async fn get(&self) -> BytesMut {
        let mut pool = self.pool.lock().await;
        if let Some(mut buf) = pool.pop() {
            buf.clear(); // Clear buffer but retain capacity
            buf
        } else {
            // If pool is empty, create new buffer
            BytesMut::with_capacity(self.buffer_size)
        }
    }

    // Return buffer to the pool
    pub async fn put(&self, mut buf: BytesMut) {
        if buf.capacity() == self.buffer_size {
            let mut pool = self.pool.lock().await;
            if pool.len() < 100 {
                // Limit pool size to avoid excessive memory usage
                buf.clear();
                pool.push(buf);
            }
        }
        // If buffer size doesn't match or pool is full, let buf be dropped
    }
}
