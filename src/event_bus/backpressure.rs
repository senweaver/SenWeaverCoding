// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use tokio::sync::{Notify, broadcast, mpsc};

use crate::event_bus::types::Event;
use crate::runtime::TaskHandle;

pub struct BoundedSubscriber {
    queue: Arc<Mutex<VecDeque<Event>>>,
    notify: Arc<Notify>,
    closed: Arc<AtomicBool>,
    forwarder: TaskHandle,
}

impl BoundedSubscriber {

    pub fn new(mut broadcast_rx: broadcast::Receiver<Event>, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let queue: Arc<Mutex<VecDeque<Event>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
        let notify = Arc::new(Notify::new());
        let closed = Arc::new(AtomicBool::new(false));

        let queue_task = Arc::clone(&queue);
        let notify_task = Arc::clone(&notify);
        let closed_task = Arc::clone(&closed);
        let forwarder =
            crate::runtime::spawn_supervised("event_bus.backpressure.forwarder", async move {
                let mut dropped: u64 = 0;
                loop {
                    match broadcast_rx.recv().await {
                        Ok(event) => {
                            {
                                let mut q = queue_task.lock();
                                while q.len() >= capacity {
                                    q.pop_front();
                                    dropped += 1;
                                    if dropped == 1 || dropped.is_multiple_of(100) {
                                        tracing::warn!(
                                            dropped_total = dropped,
                                            "bounded subscriber queue full  -  dropping oldest event to keep the newest"
                                        );
                                    }
                                }
                                q.push_back(event);
                            }
                            notify_task.notify_one();
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(
                                skipped,
                                "bounded subscriber lagged behind  -  events dropped"
                            );
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                closed_task.store(true, Ordering::Release);
                notify_task.notify_waiters();
            });

        Self {
            queue,
            notify,
            closed,
            forwarder,
        }
    }

    pub async fn recv(&mut self) -> Option<Event> {
        loop {
            let notified = self.notify.notified();
            if let Some(event) = self.queue.lock().pop_front() {
                return Some(event);
            }
            if self.closed.load(Ordering::Acquire) {
                return self.queue.lock().pop_front();
            }
            notified.await;
        }
    }

    pub fn try_recv(&mut self) -> Result<Event, mpsc::error::TryRecvError> {
        if let Some(event) = self.queue.lock().pop_front() {
            return Ok(event);
        }
        if self.closed.load(Ordering::Acquire) {
            Err(mpsc::error::TryRecvError::Disconnected)
        } else {
            Err(mpsc::error::TryRecvError::Empty)
        }
    }
}

impl Drop for BoundedSubscriber {
    fn drop(&mut self) {
        self.forwarder.abort();
    }
}
