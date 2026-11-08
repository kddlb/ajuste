//! A dedicated thread that owns the device, polls status and runs queued jobs.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::device::{Scape, Status};
use crate::transport::DeviceKind;
use crate::{Error, POLL_INTERVAL, Result};

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Connected { kind: DeviceKind, path: String },
    Status(Status),
    Disconnected { reason: String },
}

type Job = Box<dyn FnOnce(Option<&Scape>) + Send>;

pub struct Worker;

#[derive(Clone)]
pub struct WorkerHandle {
    jobs: mpsc::Sender<Job>,
}

impl Worker {
    /// Spawn the worker. Events are delivered on the returned receiver.
    pub fn spawn() -> (WorkerHandle, async_channel::Receiver<DeviceEvent>) {
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let (ev_tx, ev_rx) = async_channel::unbounded();
        thread::Builder::new()
            .name("scape-worker".into())
            .spawn(move || run(job_rx, ev_tx))
            .expect("spawn worker thread");
        (WorkerHandle { jobs: job_tx }, ev_rx)
    }
}

impl WorkerHandle {
    /// Run `f` on the worker thread with exclusive device access. A status poll follows
    /// automatically so subscribers see the effect.
    pub async fn call<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Scape) -> Result<T> + Send + 'static,
    {
        let (tx, rx) = async_channel::bounded(1);
        let job: Job = Box::new(move |dev| {
            let r = match dev {
                Some(d) => f(d),
                None => Err(Error::Disconnected),
            };
            let _ = tx.try_send(r);
        });
        self.jobs.send(job).map_err(|_| Error::Disconnected)?;
        rx.recv().await.unwrap_or(Err(Error::Disconnected))
    }

    /// Fire-and-forget variant; errors are logged.
    pub fn spawn_call<F>(&self, f: F)
    where
        F: FnOnce(&Scape) -> Result<()> + Send + 'static,
    {
        let job: Job = Box::new(move |dev| match dev {
            Some(d) => {
                if let Err(e) = f(d) {
                    log::warn!("device job failed: {e}");
                }
            }
            None => log::warn!("device job dropped: not connected"),
        });
        let _ = self.jobs.send(job);
    }
}

fn run(jobs: mpsc::Receiver<Job>, events: async_channel::Sender<DeviceEvent>) {
    let mut dev: Option<Scape> = None;
    let mut last_err = String::new();
    let mut next_poll = Instant::now();

    loop {
        if dev.is_none() && Instant::now() >= next_poll {
            match Scape::open() {
                Ok(d) => {
                    let _ = events.send_blocking(DeviceEvent::Connected { kind: d.kind(), path: d.path().to_string() });
                    dev = Some(d);
                    last_err.clear();
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg != last_err {
                        log::info!("device unavailable: {msg}");
                        let _ = events.send_blocking(DeviceEvent::Disconnected { reason: msg.clone() });
                        last_err = msg;
                    }
                    next_poll = Instant::now() + Duration::from_secs(2);
                }
            }
        }

        let wait = next_poll.saturating_duration_since(Instant::now());
        // Either a job arrived (run it and any queued ones) or the poll interval elapsed;
        // both end with a status refresh.
        match jobs.recv_timeout(wait) {
            Ok(job) => {
                job(dev.as_ref());
                while let Ok(job) = jobs.try_recv() {
                    job(dev.as_ref());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        if let Some(d) = &dev {
            match d.status() {
                Ok(s) => {
                    if events.send_blocking(DeviceEvent::Status(s)).is_err() {
                        return; // nobody listening
                    }
                }
                Err(Error::Timeout(_)) => log::debug!("status poll timed out"),
                Err(e) => {
                    log::warn!("device lost: {e}");
                    let _ = events.send_blocking(DeviceEvent::Disconnected { reason: e.to_string() });
                    last_err = e.to_string();
                    dev = None;
                }
            }
            next_poll = Instant::now() + POLL_INTERVAL;
        }
    }
}
