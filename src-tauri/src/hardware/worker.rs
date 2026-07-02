//! Rate-limited push worker: a dedicated thread that owns one device handle and
//! applies EQ band updates to it without flooding the hardware.
//!
//! Hardware can't take updates as fast as Equalizer APO's file reload, so the
//! worker *coalesces* rapid updates (only the latest desired state matters) and
//! *throttles* writes to at most one push per [`MIN_INTERVAL`]. Live edits are
//! volatile (RAM); an explicit commit also saves to the device's flash.
//!
//! The device is opened on the worker thread (HID handles are thread-affine) and
//! released when the worker stops, so nothing non-`Send` ever crosses threads.

use super::{DetectedDevice, open};
use fastpeq_core::{HardwareProfile, HwBand};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Minimum wall-clock gap between consecutive pushes. A push itself takes tens of
/// ms (many small reports), so together with that this caps the live update rate
/// to something the device's MCU keeps up with.
const MIN_INTERVAL: Duration = Duration::from_millis(60);

enum Command {
    Push {
        bands: Vec<HwBand>,
        pregain: f64,
        commit: bool,
    },
    Stop,
}

/// Live runtime state of the connection, shared with [`crate::state`] for status
/// reporting and disconnect handling.
#[derive(Default, Clone)]
pub struct RuntimeStatus {
    pub version: Option<String>,
    pub error: Option<String>,
    pub connected: bool,
}

/// A running connection to a hardware-EQ device: the worker thread plus the
/// channel that feeds it. [`stop`](Self::stop)ping (or dropping) ends the worker
/// and releases the device.
pub struct HardwareSession {
    pub descriptor: DetectedDevice,
    pub profile: HardwareProfile,
    tx: Sender<Command>,
    status: Arc<Mutex<RuntimeStatus>>,
    join: Option<JoinHandle<()>>,
}

impl HardwareSession {
    /// Open `descriptor` on a new worker thread and start listening for pushes.
    pub fn start(descriptor: DetectedDevice, profile: HardwareProfile) -> Self {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let id = descriptor.id.clone();
        let st = status.clone();
        let join = std::thread::Builder::new()
            .name("fastpeq-hw".into())
            .spawn(move || run(id, rx, st))
            .ok();
        HardwareSession {
            descriptor,
            profile,
            tx,
            status,
            join,
        }
    }

    /// Queue a volatile (RAM) push — coalesced and throttled.
    pub fn push_live(&self, bands: Vec<HwBand>, pregain: f64) -> Result<(), String> {
        self.send(Command::Push {
            bands,
            pregain,
            commit: false,
        })
    }

    /// Queue a push that also saves to the device's flash.
    pub fn push_commit(&self, bands: Vec<HwBand>, pregain: f64) -> Result<(), String> {
        self.send(Command::Push {
            bands,
            pregain,
            commit: true,
        })
    }

    fn send(&self, cmd: Command) -> Result<(), String> {
        self.tx
            .send(cmd)
            .map_err(|_| "Hardware worker has stopped".to_string())
    }

    /// A snapshot of the connection's runtime state.
    pub fn status(&self) -> RuntimeStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Signal the worker to stop and wait for it to release the device.
    pub fn stop(mut self) {
        let _ = self.tx.send(Command::Stop);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn set_status(status: &Arc<Mutex<RuntimeStatus>>, f: impl FnOnce(&mut RuntimeStatus)) {
    if let Ok(mut s) = status.lock() {
        f(&mut s);
    }
}

fn run(id: String, rx: Receiver<Command>, status: Arc<Mutex<RuntimeStatus>>) {
    let mut dev = match open(&id) {
        Ok(d) => d,
        Err(e) => {
            set_status(&status, |s| {
                s.error = Some(e);
                s.connected = false;
            });
            return;
        }
    };
    set_status(&status, |s| {
        s.connected = true;
        s.error = None;
    });
    // Version handshake (non-fatal): also a first proof the protocol is talking.
    if let Ok(v) = dev.version() {
        set_status(&status, |s| s.version = Some(v));
    }

    let mut pending: Option<(Vec<HwBand>, f64, bool)> = None;
    let mut last_write = Instant::now() - MIN_INTERVAL;

    loop {
        // Block for the next command, or wake to flush throttled pending work.
        let next = match &pending {
            Some(_) => {
                let wait = MIN_INTERVAL.saturating_sub(last_write.elapsed());
                match rx.recv_timeout(wait) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            },
        };

        match next {
            Some(Command::Stop) => break,
            Some(Command::Push {
                bands,
                pregain,
                commit,
            }) => {
                // Coalesce onto any pending state; a requested commit sticks until
                // the next flush so a flash save is never dropped.
                let prev_commit = pending.as_ref().is_some_and(|p| p.2);
                pending = Some((bands, pregain, commit || prev_commit));
            }
            None => {}
        }

        if let Some((bands, pregain, commit)) = pending.take() {
            if last_write.elapsed() >= MIN_INTERVAL {
                match dev.push(&bands, pregain, commit) {
                    Ok(()) => last_write = Instant::now(),
                    Err(e) => {
                        set_status(&status, |s| {
                            s.error = Some(e);
                            s.connected = false;
                        });
                        break;
                    }
                }
            } else {
                pending = Some((bands, pregain, commit)); // flush on the next wake
            }
        }
    }

    set_status(&status, |s| s.connected = false);
}
