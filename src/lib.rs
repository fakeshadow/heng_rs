//! A simple schedule task runner on tokio runtime that support one way message pushing.
//!
//! # example:
//! ```rust
//! use std::time::Duration;
//! use heng_rs::{Scheduler, Time, ToDuration};
//!
//! struct TestTask(u32);
//!
//! impl Scheduler for TestTask {
//!     type Message = u32;
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let task = TestTask(0);
//!     let time = Time::new()
//!         .every(1.d())
//!         .plus(2.h())
//!         .plus(3.m())
//!         .plus(4.s());
//!     // run task with a 1 day, 2 hours, 3 minutes and 4 seconds interval.
//!     let addr = task.start(time, |task, ctx| {
//!         if let Some(msg) = ctx.get_msg_front() {
//!             /* do something with message */
//!         }
//!
//!         // do something with task.
//!         task.0 += 1;
//!
//!         // run a future.
//!         async {
//!             Ok::<(),()>(())
//!         }
//!     });
//!
//!     // use address to push message to task's context;
//!     addr.send(1).await;
//!     Ok(())
//! }
//! ```

use std::any::TypeId;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures_util::{SinkExt, StreamExt};

// re export futures channel's SendError
pub use futures_channel::mpsc::SendError;
use tokio_timer::{Interval, Timeout};

pub use time::{Time, ToDuration};

mod time;

pub struct SchedulerSender<M> {
    tx: Option<UnboundedSender<M>>,
    tx_sig: Option<UnboundedSender<Signal>>,
}

impl<M> Clone for SchedulerSender<M> {
    fn clone(&self) -> Self {
        SchedulerSender {
            tx: self.tx.as_ref().cloned(),
            tx_sig: self.tx_sig.as_ref().cloned(),
        }
    }
}

impl<M: Send + 'static> SchedulerSender<M> {
    /// send message to `Scheduler`'s `Context`.
    ///
    /// `Context` stores the message in a `VecDequeue`.New message is pushed to the back.
    pub async fn send(&self, msg: M) -> Result<(), SendError> {
        self.tx
            .as_ref()
            .expect("SchedulerSender is None")
            .send(msg)
            .await
    }

    /// send message to `Scheduler`'s `Context` and ignore the result.
    pub fn do_send(&self, msg: M) {
        let sender = self.clone();
        tokio_executor::spawn(async move {
            let _ = sender.tx.as_ref().unwrap().send(msg).await;
        });
    }

    pub fn start(&self) -> impl Future<Output = Result<(), SendError>> + '_ {
        self.send_signal(Signal::Start)
    }

    pub fn stop(&self) -> impl Future<Output = Result<(), SendError>> + '_ {
        self.send_signal(Signal::Stop)
    }

    /// change the interval of `Scheduler`.
    pub fn change_time(
        &self,
        time: impl Into<Duration>,
    ) -> impl Future<Output = Result<(), SendError>> + '_ {
        self.send_signal(Signal::ChangeDur(time.into()))
    }

    async fn send_signal(&self, signal: Signal) -> Result<(), SendError> {
        self.tx_sig
            .as_ref()
            .expect("SignalSender is None")
            .send(signal)
            .await
    }
}

pub trait Scheduler: Sized + Send + 'static {
    type Message: Send;

    /// start a new `Scheduler` with the given time and closure.
    ///
    /// You can't get access of `&mut Self` and `&mut Context` in the closure with a future or async block.
    fn start<F, Fut, R>(self, time: impl Into<Duration>, f: F) -> SchedulerSender<Self::Message>
    where
        F: FnMut(&mut Self, &mut Context<Self>) -> Fut + Send + 'static,
        Fut: Future<Output = R> + Send + 'static,
    {
        // setup context.
        let (mut ctx, tx_sig) = Context::new();

        // setup message channel.
        let tx = spawn_message_channel::<Self>(&ctx.msg);

        // set duration.
        ctx.signal(Signal::ChangeDur(time.into()));

        // run the interval.
        self.run(f, ctx);

        // return shared channel sender.
        SchedulerSender { tx, tx_sig }
    }

    fn run<F, Fut, R>(mut self, mut f: F, mut ctx: Context<Self>)
    where
        F: FnMut(&mut Self, &mut Context<Self>) -> Fut + Send + 'static,
        Fut: Future<Output = R> + Send + 'static,
    {
        tokio_executor::spawn(async move {
            let dur = ctx.dur;
            let mut interval = Interval::new(Instant::now(), dur);
            while let Some(_instant) = interval.next().await {
                // if we are not running we just ignore F.
                if ctx.running {
                    f(&mut self, &mut ctx).await;
                }

                if ctx.should_restart(dur).await {
                    drop(interval);
                    return self.run(f, ctx);
                }
            }
        });
    }

    fn handler<'a>(
        &'a mut self,
        _ctx: &'a mut Context<Self>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }

    /// You can access `&mut Self` and `&mut Context<Self>` in a future if you manually impl `handler` method for your Type
    /// and start the task with `start_with_handler`
    ///```rust
    /// use std::pin::Pin;
    /// use std::future::Future;
    /// use std::time::{Duration, Instant};
    ///
    /// use heng_rs::{Scheduler, Context};
    ///
    /// struct TestTask {
    ///     field: u32
    /// }
    ///
    /// impl Scheduler for TestTask {
    ///     type Message = u32;
    ///
    ///     fn handler<'a>(&'a mut self, ctx: &'a mut Context<Self>) -> Pin<Box<dyn Future<Output=()> + Send + 'a>> {
    ///         Box::pin(async move {
    ///             assert_eq!(self.field, 0);
    ///             // we can modify the context and self in the async block.
    ///             if let Some(msg) = ctx.get_msg_front() {
    ///                 self.field = msg;
    ///             };
    ///             assert_eq!(self.field, 3);
    ///         })
    ///     }
    /// }
    /// #[tokio::main]
    /// async fn main() -> std::io::Result<()> {
    ///     let task = TestTask { field: 0 };
    ///     let addr = task.start_with_handler(Duration::from_secs(1));
    ///
    ///     // use address to send message to task;
    ///     addr.send(3).await;
    ///     tokio::timer::delay(Instant::now() + Duration::from_secs(2)).await;
    ///     Ok(())
    /// }
    ///```
    fn start_with_handler(self, time: impl Into<Duration>) -> SchedulerSender<Self::Message> {
        let (mut ctx, tx_sig) = Context::new();

        let tx = spawn_message_channel::<Self>(&ctx.msg);

        ctx.signal(Signal::ChangeDur(time.into()));

        self.run_with_handler(ctx);

        SchedulerSender { tx, tx_sig }
    }

    fn run_with_handler(mut self, mut ctx: Context<Self>) {
        tokio_executor::spawn(async move {
            let dur = ctx.dur;
            let mut interval = Interval::new(Instant::now(), dur);
            while let Some(_instant) = interval.next().await {
                // if we are not running we just ignore handler.
                if ctx.running {
                    self.handler(&mut ctx).await;
                }

                // we listen to signal for a period of self duration after handler
                if ctx.should_restart(dur).await {
                    drop(interval);
                    return self.run_with_handler(ctx);
                }
            }
        });
    }
}

fn spawn_message_channel<S: Scheduler>(
    msg: &Arc<Mutex<VecDeque<S::Message>>>,
) -> Option<UnboundedSender<S::Message>> {
    // If we have a message type other than ().
    // Then we pass message queue and message channel to spawn future and push new message to it.
    if TypeId::of::<S::Message>() == TypeId::of::<()>() {
        None
    } else {
        // setup message channel.
        let (tx, mut rx) = unbounded::<S::Message>();

        // spawn a future to inject message to context.msg with channel sender.
        let msg = Arc::downgrade(&msg);
        tokio_executor::spawn(async move {
            while let Some(m) = rx.next().await {
                match msg.upgrade() {
                    Some(msg) => msg
                        .lock()
                        .expect("Failed to acquire Message Mutex lock")
                        .push_back(m),
                    None => panic!("Fail to upgrade Arc<Mutex<VecDequeue<Scheduler::Message>"),
                };
            }
        });

        Some(tx)
    }
}

enum Signal {
    Stop,
    Start,
    ChangeDur(Duration),
}

pub struct Context<S: Scheduler> {
    msg: Arc<Mutex<VecDeque<S::Message>>>,
    dur: Duration,
    running: bool,
    rx_sig: UnboundedReceiver<Signal>,
}

impl<S: Scheduler> Context<S> {
    fn new() -> (Self, Option<UnboundedSender<Signal>>) {
        let (tx, rx_sig) = unbounded::<Signal>();
        let ctx = Context {
            msg: Arc::new(Mutex::new(VecDeque::new())),
            dur: Duration::default(),
            running: true,
            rx_sig,
        };

        (ctx, Some(tx))
    }

    fn signal(&mut self, signal: Signal) -> &mut Self {
        match signal {
            Signal::Stop => self.running = false,
            Signal::Start => self.running = true,
            Signal::ChangeDur(dur) => self.dur = dur,
        };
        self
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<S::Message>> {
        self.msg
            .lock()
            .expect("Failed to acquire Message Mutex lock")
    }

    /// Access the message queue with a closure.
    pub fn get_queue_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut VecDeque<S::Message>) -> R,
    {
        let mut guard = self.lock();
        f(&mut guard)
    }

    pub fn get_msg_front(&self) -> Option<S::Message> {
        self.lock().pop_front()
    }

    pub fn get_msg_back(&self) -> Option<S::Message> {
        self.lock().pop_back()
    }

    pub fn push_msg_back(&self, msg: S::Message) {
        self.lock().push_back(msg)
    }

    pub fn push_msg_front(&self, msg: S::Message) {
        self.lock().push_front(msg)
    }

    async fn should_restart(&mut self, dur: Duration) -> bool {
        // we listen to signal for a period of self duration after handle
        if let Ok(signal) = Timeout::new(self.rx_sig.next(), dur).await {
            if let Some(signal) = signal {
                // if we have a changed duration we drop the interval and start a new run.
                if self.signal(signal).dur != dur {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod test_lib {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use futures_util::{lock::Mutex, SinkExt, StreamExt};

    use crate::time::{Time, ToDuration};
    use crate::Scheduler;

    struct TestSchedule;

    impl Scheduler for TestSchedule {
        type Message = u32;
    }

    #[tokio::test]
    async fn stop_send_restart() -> std::io::Result<()> {
        let test = TestSchedule;

        let (tx, mut rx) = futures_channel::mpsc::channel::<u32>(1);
        let tx = Arc::new(Mutex::new(tx));

        let time = Time::new().every(400.millis()).plus(100.millis());

        let addr = test.start(time, move |_task, ctx| {
            let sender = tx.clone();
            let msg = ctx.get_msg_front();
            async move {
                if let Some(msg) = msg {
                    let _ = sender.lock().await.send(msg).await;
                };
            }
        });

        let _ = addr.stop().await;
        let _ = addr.send(32).await;

        let now = Instant::now();

        tokio::timer::delay(Instant::now() + Duration::from_secs(2)).await;

        let _ = addr.start().await;

        assert_eq!(rx.next().await.unwrap(), 32);
        assert!(Instant::now().duration_since(now) > Duration::from_secs(2));

        Ok(())
    }

    #[tokio::test]
    async fn queue() -> std::io::Result<()> {
        let test = TestSchedule;

        let time = Time::new().every(1.s());

        let addr = test.start(time, |_task, ctx| {
            assert_eq!(ctx.get_queue_mut(|queue| queue.pop_front()), Some(1));
            assert_eq!(ctx.get_msg_front(), Some(2));
            assert_eq!(ctx.get_msg_back(), Some(3));
            assert_eq!(ctx.get_queue_mut(|queue| queue.get(0) == Some(&4)), true);
            futures_util::future::ready(())
        });

        let _ = addr.stop().await;
        let _ = addr.send(1).await;
        let _ = addr.send(2).await;
        let _ = addr.send(4).await;
        let _ = addr.send(3).await;
        let _ = addr.start().await;

        tokio::timer::delay(Instant::now() + Duration::from_secs(2)).await;

        Ok(())
    }
}
