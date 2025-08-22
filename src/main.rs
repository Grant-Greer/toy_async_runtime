use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;

struct Task<T>
where
    T: Send,
{
    future: Pin<Box<dyn Future<Output = T> + Send>>,
    sender: SyncSender<T>,
}

pub struct TaskRunTime<T>
where
    T: Send,
{
    sender: Sender<Task<T>>,
}

pub struct TaskHandle<T>
where
    T: Send,
{
    receiver: Receiver<T>,
}

impl<T> TaskHandle<T>
where
    T: Send,
{
    pub fn join(self) -> T {
        self.receiver.recv().expect("failed to receive result")
    }
}

impl<T> TaskRunTime<T>
where
    T: Send + 'static,
{
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        std::thread::spawn(move || Self::run(receiver));
        Self { sender }
    }

    pub fn spawn<F>(&mut self, f: F) -> TaskHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send,
    {
        let (result_sender, result_receiver) = sync_channel(1);
        let task = Task {
            future: Box::pin(f),
            sender: result_sender,
        };
        self.sender.send(task).expect("failed to spawn");
        TaskHandle {
            receiver: result_receiver,
        }
    }

    pub fn block_on<F>(mut f: F) -> F::Output
    where
        F: Future,
    {
        let mut f = unsafe { Pin::new_unchecked(&mut f) };
        let thread = std::thread::current();
        let waker = Arc::new(SimpleWaker { thread }).into();
        let mut ctx = Context::from_waker(&waker);
        loop {
            println!("polling future");
            match f.as_mut().poll(&mut ctx) {
                Poll::Ready(val) => {
                    return val;
                }
                Poll::Pending => {
                    std::thread::park();
                    println!("parked");
                }
            }
        }
    }

    fn run(receiver: Receiver<Task<T>>) {
        while let Ok(task) = receiver.recv() {
            std::thread::spawn(move || Self::run_task(task));
        }
    }

    fn run_task(task: Task<T>) {
        let res = Self::block_on(task.future);
        task.sender.send(res).expect("failed to send result");
    }
}

// Simple waker implementation
struct SimpleWaker {
    thread: thread::Thread,
}

impl Wake for SimpleWaker {
    fn wake(self: Arc<Self>) {
        self.thread.unpark();
    }
}

// Counter future implementation
struct Counter {
    counter: Arc<AtomicU64>,
    max: u64,
}

impl Future for Counter {
    type Output = u64;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current = self.counter.fetch_add(1, Ordering::SeqCst);
        if current >= self.max {
            Poll::Ready(current)
        } else {
            // Wake the task to continue polling
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

fn main() {
    let mut runtime = TaskRunTime::new();
    let handle_1 = runtime.spawn(async_fn());
    let handle2 = runtime.spawn(async_fn2());
    let res = handle_1.join();
    let res2 = handle2.join();
    println!("res: {res}");
    println!("res2: {res2}");
}

async fn async_fn() -> u64 {
    let res = count(10).await;
    println!("async_fn {res}");
    res
}

async fn async_fn2() -> u64 {
    let res = count(23).await;
    println!("async_fn2 {res}");
    res
}

fn count(max: u64) -> impl Future<Output = u64> {
    println!("count");
    Counter {
        counter: Arc::new(AtomicU64::new(0)),
        max,
    }
}
