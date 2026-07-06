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

use super::{DetectedDevice, HardwareEq, open};
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

/// Live runtime state of the connection, shared with the consumer (the app's
/// state layer, or the CLI's `session` command) for status reporting and
/// disconnect handling.
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

/// Clears `connected` when the worker exits — *however* it exits. A panic
/// (say, a malformed device reply tripping a decoder) unwinds past any
/// trailing status update, and without this guard the session would keep
/// reporting a connected device forever while the worker is dead.
struct DisconnectOnExit<'a>(&'a Arc<Mutex<RuntimeStatus>>);

impl Drop for DisconnectOnExit<'_> {
    fn drop(&mut self) {
        let panicked = std::thread::panicking();
        set_status(self.0, |s| {
            s.connected = false;
            if panicked && s.error.is_none() {
                s.error = Some("Hardware worker crashed".to_string());
            }
        });
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
    let _disconnect = DisconnectOnExit(&status);
    set_status(&status, |s| {
        s.connected = true;
        s.error = None;
    });
    // Version handshake (non-fatal): also a first proof the protocol is talking.
    if let Ok(v) = dev.version() {
        set_status(&status, |s| s.version = Some(v));
    }
    run_loop(dev.as_mut(), &rx, &status);
}

/// The coalesce/throttle loop, on a device that's already open. Split from
/// [`run`] so it can be driven by a mock [`HardwareEq`] in tests.
fn run_loop(dev: &mut dyn HardwareEq, rx: &Receiver<Command>, status: &Arc<Mutex<RuntimeStatus>>) {
    let mut pending: Option<(Vec<HwBand>, f64, bool)> = None;
    let mut last_write = Instant::now() - MIN_INTERVAL;
    // The last state actually written, so an unchanged push can be skipped — chiefly
    // to avoid re-flashing an identical config (the editor debounces a commit that may
    // repeat the current bands/pregain).
    let mut last_written: Option<(Vec<HwBand>, f64, bool)> = None;

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
                let unchanged = last_written
                    .as_ref()
                    .is_some_and(|(b, p, c)| *b == bands && *p == pregain && *c == commit);
                if unchanged {
                    last_write = Instant::now();
                } else {
                    match dev.push(&bands, pregain, commit) {
                        Ok(()) => {
                            last_write = Instant::now();
                            last_written = Some((bands, pregain, commit));
                        }
                        Err(e) => {
                            set_status(status, |s| {
                                s.error = Some(e);
                                s.connected = false;
                            });
                            break;
                        }
                    }
                }
            } else {
                pending = Some((bands, pregain, commit)); // flush on the next wake
            }
        }
    }

    // A pending push carrying a flash commit must not be lost on shutdown —
    // the editor commits on mouse release, and a Stop (or the app quitting and
    // dropping the sender) can land inside the throttle window. Volatile
    // pending state is fine to drop; a promised flash save is not.
    if let Some((bands, pregain, true)) = pending {
        let unchanged = last_written
            .as_ref()
            .is_some_and(|(b, p, c)| *b == bands && *p == pregain && *c);
        if !unchanged {
            std::thread::sleep(MIN_INTERVAL.saturating_sub(last_write.elapsed()));
            let _ = dev.push(&bands, pregain, true);
        }
    }
    // `connected` is reset by the caller's DisconnectOnExit guard (which also
    // covers a panic anywhere above), not by a trailing update here.
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastpeq_core::HwFilterType;
    use std::sync::mpsc::channel;

    /// Records every push it receives.
    struct MockDev(Vec<(Vec<HwBand>, f64, bool)>);

    impl HardwareEq for MockDev {
        fn push(&mut self, bands: &[HwBand], pregain: f64, commit: bool) -> Result<(), String> {
            self.0.push((bands.to_vec(), pregain, commit));
            Ok(())
        }
        fn pull(&mut self) -> Result<Vec<HwBand>, String> {
            Ok(Vec::new())
        }
        fn version(&mut self) -> Result<String, String> {
            Ok("mock".to_string())
        }
    }

    fn band() -> HwBand {
        HwBand {
            kind: HwFilterType::Peak,
            freq: 1000.0,
            gain: 3.0,
            q: 1.0,
        }
    }

    /// A commit still sitting throttled in `pending` when Stop arrives must be
    /// flushed to the device, not dropped — a flash save is a promise. (If the
    /// test machine stalls past the throttle window the commit lands inline
    /// instead; the assertion holds either way.)
    #[test]
    fn pending_commit_is_flushed_on_stop() {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let mut dev = MockDev(Vec::new());
        // The first push lands immediately and opens the throttle window; the
        // commit then coalesces into `pending`; Stop arrives before the flush.
        tx.send(Command::Push {
            bands: Vec::new(),
            pregain: 0.0,
            commit: false,
        })
        .unwrap();
        tx.send(Command::Push {
            bands: vec![band()],
            pregain: -3.0,
            commit: true,
        })
        .unwrap();
        tx.send(Command::Stop).unwrap();

        run_loop(&mut dev, &rx, &status);

        assert_eq!(
            dev.0.last(),
            Some(&(vec![band()], -3.0, true)),
            "the pending flash commit must reach the device"
        );
    }

    /// Same when the sender is dropped (app quit) instead of an explicit Stop.
    #[test]
    fn pending_commit_is_flushed_on_disconnect() {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let mut dev = MockDev(Vec::new());
        tx.send(Command::Push {
            bands: Vec::new(),
            pregain: 0.0,
            commit: false,
        })
        .unwrap();
        tx.send(Command::Push {
            bands: vec![band()],
            pregain: -3.0,
            commit: true,
        })
        .unwrap();
        drop(tx);

        run_loop(&mut dev, &rx, &status);

        assert_eq!(dev.0.last(), Some(&(vec![band()], -3.0, true)));
    }

    /// Shutdown never invents a flash write: volatile-only traffic stays volatile.
    #[test]
    fn volatile_pushes_are_never_flash_committed_on_stop() {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let mut dev = MockDev(Vec::new());
        for gain in [1.0, 2.0] {
            tx.send(Command::Push {
                bands: vec![HwBand {
                    gain,
                    ..band()
                }],
                pregain: 0.0,
                commit: false,
            })
            .unwrap();
        }
        tx.send(Command::Stop).unwrap();

        run_loop(&mut dev, &rx, &status);

        assert!(dev.0.iter().all(|(.., commit)| !commit));
    }

    /// The exit guard clears `connected` even when the worker panics (e.g. a
    /// malformed device reply), and records an error so the UI shows why.
    #[test]
    fn a_panicking_worker_still_reads_as_disconnected() {
        let status = Arc::new(Mutex::new(RuntimeStatus {
            connected: true,
            ..Default::default()
        }));
        let st = status.clone();
        let worker = std::thread::spawn(move || {
            let _guard = DisconnectOnExit(&st);
            panic!("simulated decoder panic");
        });
        assert!(worker.join().is_err());

        let s = status.lock().unwrap();
        assert!(!s.connected, "a dead worker must not read as connected");
        assert_eq!(s.error.as_deref(), Some("Hardware worker crashed"));
    }
}
