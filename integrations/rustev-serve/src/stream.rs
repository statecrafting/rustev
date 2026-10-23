//! Bounded progress transport with a separately reserved terminal event.
use hyper::body::{Body, Bytes, Frame, SizeHint};
use rustev_contract::{
    Document,
    serve::{EventKind, EventSchema, FinalResult, ServeEvent},
};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};
use tokio::sync::{mpsc, oneshot};

pub struct Events {
    queue: mpsc::Sender<EventKind>,
    dropped: Arc<AtomicU64>,
    total_dropped: Arc<AtomicU64>,
}
impl Events {
    /// A bounded, non-waiting offer. The decision never awaits a consumer.
    pub fn offer(&self, event: EventKind) {
        if self.queue.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.total_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
pub struct Finish {
    tx: oneshot::Sender<FinalResult>,
}
impl Finish {
    pub fn send(self, result: FinalResult) {
        let _ = self.tx.send(result);
    }
}
pub struct EventBody {
    queue: mpsc::Receiver<EventKind>,
    final_result: oneshot::Receiver<FinalResult>,
    dropped: Arc<AtomicU64>,
    sequence: u64,
    ended: bool,
}
pub fn channel(capacity: usize, total_dropped: Arc<AtomicU64>) -> (Events, Finish, EventBody) {
    let (tx, rx) = mpsc::channel(capacity);
    let (final_tx, final_rx) = oneshot::channel();
    let dropped = Arc::new(AtomicU64::new(0));
    (
        Events {
            queue: tx,
            dropped: dropped.clone(),
            total_dropped,
        },
        Finish { tx: final_tx },
        EventBody {
            queue: rx,
            final_result: final_rx,
            dropped,
            sequence: 0,
            ended: false,
        },
    )
}
impl Body for EventBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        if self.ended {
            return Poll::Ready(None);
        }
        let kind = match self.queue.poll_recv(cx) {
            Poll::Ready(Some(event)) => event,
            _ => match std::future::Future::poll(Pin::new(&mut self.final_result), cx) {
                Poll::Ready(Ok(result)) => {
                    self.ended = true;
                    EventKind::Final {
                        result,
                        events_dropped: self.dropped.load(Ordering::Relaxed),
                    }
                }
                Poll::Ready(Err(_)) => {
                    self.ended = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            },
        };
        let name = match &kind {
            EventKind::Admitted => "admitted",
            EventKind::Step { .. } => "step",
            EventKind::Field(_) => "field",
            EventKind::Final { .. } => "final",
        };
        let event = ServeEvent {
            schema: EventSchema::V1,
            sequence: self.sequence,
            kind,
        };
        self.sequence += 1;
        let mut bytes = format!("event: {name}\ndata: ").into_bytes();
        bytes.extend(event.record_canonical().expect("bounded progress event"));
        bytes.extend(b"\n\n");
        Poll::Ready(Some(Ok(Frame::data(Bytes::from(bytes)))))
    }
    fn is_end_stream(&self) -> bool {
        self.ended
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use rustev_contract::{
        scope::Handle,
        serve::{Embedded, List, RefusalCode, StepOutcome},
    };
    #[tokio::test]
    async fn stalled_consumer_drops_progress_but_preserves_final() {
        let total = Arc::new(AtomicU64::new(0));
        let (events, finish, body) = channel(16, total.clone());
        for _ in 0..100 {
            events.offer(EventKind::Step {
                step_id: Handle::new("step").unwrap(),
                instance_ids: List::new(vec![]).unwrap(),
                outcome: StepOutcome::Supplied,
                derivation: rustev_contract::judgment::Derivation::ExactDerived,
            });
        }
        let refusal = crate::Reply::refusal(RefusalCode::Overloaded);
        finish.send(FinalResult::Refusal(
            Embedded::parse(&refusal.body).unwrap(),
        ));
        // Keep the producer alive: final must not wait for it to be dropped.
        let bytes = tokio::time::timeout(std::time::Duration::from_secs(1), body.collect())
            .await
            .unwrap()
            .unwrap()
            .to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(text.matches("event: step\n").count(), 16);
        assert_eq!(text.matches("event: final\n").count(), 1);
        let last = text.split("data: ").last().unwrap().trim();
        let event = ServeEvent::parse(last.as_bytes()).unwrap();
        assert_eq!(event.sequence, 16);
        assert!(matches!(
            event.kind,
            EventKind::Final {
                events_dropped: 84,
                ..
            }
        ));
        assert_eq!(total.load(Ordering::Relaxed), 84);
    }
}
