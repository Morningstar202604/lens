//! 单写入者队列:代理热路径只投递,落库由专用线程串行完成。
//!
//! 目的:不再让 tokio worker 在请求路径上持有 SQLite 锁做序列化写入;
//! 队列满时由调用方回退为同步写入(不丢数据)。

use tokio::sync::mpsc;
use tracing::debug;

use crate::engine::Store;
use crate::proxy::parser::{LlmRequest, LlmResponse};

pub struct QueuedTurnData {
    pub agent: String,
    pub source: String,
    pub request: LlmRequest,
    pub response: LlmResponse,
}

pub enum QueuedTurn {
    Turn(Box<QueuedTurnData>),
    Barrier(tokio::sync::oneshot::Sender<()>),
}

#[derive(Clone, Debug)]
pub struct WriterHandle {
    tx: mpsc::Sender<QueuedTurn>,
}

impl WriterHandle {
    /// 启动写入线程(脱离 tokio runtime,阻塞读队列)。
    pub fn spawn(store: std::sync::Arc<Store>) -> Self {
        let (tx, mut rx) = mpsc::channel::<QueuedTurn>(512);
        std::thread::spawn(move || {
            while let Some(item) = rx.blocking_recv() {
                match item {
                    QueuedTurn::Turn(data) => {
                        if let Err(e) = store.record_turn_with_source(
                            &data.agent,
                            &data.source,
                            data.request,
                            data.response,
                        ) {
                            tracing::warn!("落库失败: {}", e);
                        }
                    }
                    QueuedTurn::Barrier(ack) => {
                        debug!("写入队列已排空(barrier)");
                        let _ = ack.send(());
                    }
                }
            }
        });
        Self { tx }
    }

    /// 非阻塞投递;队列满时把条目原样退回给调用方(由调用方同步写入)。
    pub fn record(&self, item: QueuedTurn) -> Option<QueuedTurn> {
        match self.tx.try_send(item) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Full(item)) => Some(item),
            Err(mpsc::error::TrySendError::Closed(_)) => None,
        }
    }

    /// 等待队列中已有条目全部落库(测试与关停前使用)。
    pub async fn barrier(&self) {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(QueuedTurn::Barrier(ack_tx)).await;
        let _ = ack_rx.await;
    }
}

/// 统一记录入口:有队列走队列,队列满(或未启用队列)则同步写入。
pub fn record_turn(
    store: &Store,
    writer: Option<&WriterHandle>,
    agent: &str,
    source: &str,
    request: LlmRequest,
    response: LlmResponse,
) {
    if let Some(writer) = writer {
        let item = QueuedTurn::Turn(Box::new(QueuedTurnData {
            agent: agent.to_string(),
            source: source.to_string(),
            request: request.clone(),
            response: response.clone(),
        }));
        if writer.record(item).is_none() {
            return;
        }
        tracing::warn!("写入队列已满,回退同步落库");
    }
    if let Err(e) = store.record_turn_with_source(agent, source, request, response) {
        tracing::warn!("记录失败: {}", e);
    }
}
