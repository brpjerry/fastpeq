//! Rate-limited push worker: a dedicated thread that owns one device handle and
//! applies EQ band updates to it without flooding the hardware.
//!
//! Hardware can't take updates as fast as Equalizer APO's file reload, so the
//! worker *coalesces* rapid updates (only the latest desired state matters) and
//! *throttles* writes to at most one push per [`MIN_INTERVAL`]. Live edits are
//! volatile (RAM); an explicit commit also saves to the device's flash — inline
//! on commit-to-apply devices, otherwise *debounced* so a burst of deliberate
//! actions wears the flash once, not once per action (see [`CommitPolicy`]).
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

/// How long the device must stay quiet before a debounced flash commit is
/// written (see [`CommitPolicy::Debounced`]). Long enough that cycling presets
/// from the tray or toggling bypass coalesces into one flash at the end;
/// short enough that unplugging the device seconds later still finds the EQ
/// persisted.
const FLASH_DEBOUNCE: Duration = Duration::from_secs(2);

/// How a device's flash commits are scheduled.
#[derive(Clone, Copy, PartialEq)]
enum CommitPolicy {
    /// Write the commit inline with the push. For commit-to-apply devices
    /// (the DHA15): RAM writes never reach the audio, so deferring the flash
    /// would defer the change itself.
    Immediate,
    /// Apply every push volatile immediately, and write one flash commit once
    /// the device has been quiet for the given interval. For devices that
    /// apply RAM writes live (KA17, Space Pro): the sound is right instantly,
    /// and the flash — only needed so the EQ survives a power cycle — is
    /// written once per burst of activity instead of once per preset click
    /// (flash wear, and the KA17's save has faulted the device before).
    Debounced(Duration),
}

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
        let policy = if profile.commit_to_apply {
            CommitPolicy::Immediate
        } else {
            CommitPolicy::Debounced(FLASH_DEBOUNCE)
        };
        let join = match std::thread::Builder::new()
            .name("fastpeq-hw".into())
            .spawn(move || run(id, rx, st, policy))
        {
            Ok(join) => Some(join),
            // Without this, a failed spawn left the default status (not
            // connected, no error) — a session that looks like it's
            // connecting forever.
            Err(e) => {
                set_status(&status, |s| {
                    s.error = Some(format!("Could not start the hardware worker: {e}"));
                });
                None
            }
        };
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

    /// Block (bounded) until the worker has settled its open — `connected`, or
    /// an error — and return that status. A status read straight after
    /// [`start`](Self::start) would otherwise race the device open and briefly
    /// report a healthy session as disconnected.
    pub fn wait_ready(&self, timeout: Duration) -> RuntimeStatus {
        let deadline = Instant::now() + timeout;
        loop {
            let s = self.status();
            if s.connected || s.error.is_some() || Instant::now() >= deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
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

fn run(id: String, rx: Receiver<Command>, status: Arc<Mutex<RuntimeStatus>>, policy: CommitPolicy) {
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
    run_loop(dev.as_mut(), &rx, &status, policy);
}

/// The coalesce/throttle loop, on a device that's already open. Split from
/// [`run`] so it can be driven by a mock [`HardwareEq`] in tests.
fn run_loop(
    dev: &mut dyn HardwareEq,
    rx: &Receiver<Command>,
    status: &Arc<Mutex<RuntimeStatus>>,
    policy: CommitPolicy,
) {
    // Desired state not yet written (coalesced; a requested commit sticks).
    let mut pending: Option<(Vec<HwBand>, f64, bool)> = None;
    // Backdated so the first push isn't throttled. `checked_sub` because an
    // `Instant` can't be rewound past its (opaque) epoch — the panic is
    // theoretical, but the fallback (a one-time 60 ms delay) costs nothing.
    let mut last_write = Instant::now()
        .checked_sub(MIN_INTERVAL)
        .unwrap_or_else(Instant::now);
    // What the device's RAM holds (the last state actually pushed), so an
    // unchanged push can be skipped.
    let mut ram: Option<(Vec<HwBand>, f64)> = None;
    // What the device's flash holds, as far as this session knows, so an
    // identical re-commit (re-clicking the active preset) never re-flashes.
    let mut flashed: Option<(Vec<HwBand>, f64)> = None;
    // When a debounced flash falls due; `None` = no flash owed.
    let mut flash_due: Option<Instant> = None;

    loop {
        // Block for the next command, or wake for the earlier of: flushing
        // throttled pending work, or a debounced flash falling due.
        let deadline = match (&pending, flash_due) {
            (Some(_), _) => Some(last_write + MIN_INTERVAL),
            (None, Some(due)) => Some(due.max(last_write + MIN_INTERVAL)),
            (None, None) => None,
        };
        let next = match deadline {
            Some(d) => {
                let wait = d.saturating_duration_since(Instant::now());
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

        // Write pending state once the throttle window is clear.
        if let Some((bands, pregain, commit)) = pending.take() {
            if last_write.elapsed() < MIN_INTERVAL {
                pending = Some((bands, pregain, commit)); // flush on the next wake
            } else {
                let state = (bands, pregain);
                let commit_inline = commit && policy == CommitPolicy::Immediate;
                // Skip the write when the device already holds this state (and,
                // for an inline commit, already has it flashed).
                if ram.as_ref() == Some(&state)
                    && (!commit_inline || flashed.as_ref() == Some(&state))
                {
                    last_write = Instant::now();
                } else {
                    match dev.push(&state.0, state.1, commit_inline) {
                        Ok(()) => {
                            last_write = Instant::now();
                            ram = Some(state.clone());
                            if commit_inline {
                                flashed = Some(state.clone());
                            }
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
                // Debounced-flash bookkeeping. A commit for already-flashed
                // state owes nothing; a fresh commit (re)starts the quiet
                // timer — and so does any further write while a flash is
                // owed, so the flash lands after the burst, not inside it.
                if let CommitPolicy::Debounced(delay) = policy {
                    if flashed.as_ref() == Some(&state) {
                        flash_due = None;
                    } else if commit || flash_due.is_some() {
                        flash_due = Some(Instant::now() + delay);
                    }
                }
            }
        }

        // Write the debounced flash once it falls due (and the throttle allows).
        if pending.is_none()
            && flash_due.is_some_and(|due| Instant::now() >= due)
            && last_write.elapsed() >= MIN_INTERVAL
        {
            match ram.clone() {
                Some((bands, pregain)) if flashed.as_ref() != Some(&(bands.clone(), pregain)) => {
                    match dev.push(&bands, pregain, true) {
                        Ok(()) => {
                            last_write = Instant::now();
                            flashed = Some((bands, pregain));
                            flash_due = None;
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
                _ => flash_due = None, // nothing to persist / already flashed
            }
        }
    }

    // A promised flash must not be lost on shutdown — it may still be
    // coalesced in `pending` (Stop landed inside the throttle window) or
    // deferred by the debounce. Volatile pending state is fine to drop.
    let owed = match pending {
        Some((bands, pregain, true)) => Some((bands, pregain)),
        _ => flash_due.and(ram),
    };
    if let Some((bands, pregain)) = owed
        && flashed.as_ref() != Some(&(bands.clone(), pregain))
    {
        std::thread::sleep(MIN_INTERVAL.saturating_sub(last_write.elapsed()));
        let _ = dev.push(&bands, pregain, true);
    }
    // `connected` is reset by the caller's DisconnectOnExit guard (which also
    // covers a panic anywhere above), not by a trailing update here.
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastpeq_core::HwFilterType;
    use std::sync::mpsc::channel;

    type PushLog = Arc<Mutex<Vec<(Vec<HwBand>, f64, bool)>>>;

    /// Records every push into a shared log (shared so the debounce test can
    /// run the loop on its own thread and inspect from the test thread).
    struct MockDev(PushLog);

    impl MockDev {
        fn new() -> (Self, PushLog) {
            let log: PushLog = Arc::new(Mutex::new(Vec::new()));
            (MockDev(log.clone()), log)
        }
    }

    impl HardwareEq for MockDev {
        fn push(&mut self, bands: &[HwBand], pregain: f64, commit: bool) -> Result<(), String> {
            self.0.lock().unwrap().push((bands.to_vec(), pregain, commit));
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
        let (mut dev, log) = MockDev::new();
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

        run_loop(&mut dev, &rx, &status, CommitPolicy::Immediate);

        assert_eq!(
            log.lock().unwrap().last(),
            Some(&(vec![band()], -3.0, true)),
            "the pending flash commit must reach the device"
        );
    }

    /// Same when the sender is dropped (app quit) instead of an explicit Stop —
    /// and under the debounced policy, where the flash was deferred.
    #[test]
    fn pending_commit_is_flushed_on_disconnect() {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let (mut dev, log) = MockDev::new();
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

        run_loop(
            &mut dev,
            &rx,
            &status,
            CommitPolicy::Debounced(FLASH_DEBOUNCE),
        );

        assert_eq!(log.lock().unwrap().last(), Some(&(vec![band()], -3.0, true)));
    }

    /// Shutdown never invents a flash write: volatile-only traffic stays volatile.
    #[test]
    fn volatile_pushes_are_never_flash_committed_on_stop() {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let (mut dev, log) = MockDev::new();
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

        run_loop(
            &mut dev,
            &rx,
            &status,
            CommitPolicy::Debounced(FLASH_DEBOUNCE),
        );

        assert!(log.lock().unwrap().iter().all(|(.., commit)| !commit));
    }

    /// Debounced policy: a burst of commit pushes (tray preset cycling) applies
    /// each state volatile but writes exactly ONE flash — for the final state —
    /// once the device goes quiet.
    #[test]
    fn debounced_commits_coalesce_into_one_flash() {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let (mut dev, log) = MockDev::new();
        let st = status.clone();
        let worker = std::thread::spawn(move || {
            run_loop(
                &mut dev,
                &rx,
                &st,
                CommitPolicy::Debounced(Duration::from_millis(50)),
            );
        });

        // Three "preset clicks" in quick succession, then quiet.
        for gain in [1.0, 2.0, 3.0] {
            tx.send(Command::Push {
                bands: vec![HwBand { gain, ..band() }],
                pregain: -gain,
                commit: true,
            })
            .unwrap();
        }
        std::thread::sleep(Duration::from_millis(600)); // > throttle + debounce
        drop(tx);
        worker.join().unwrap();

        let pushes = log.lock().unwrap();
        let flashes: Vec<_> = pushes.iter().filter(|(.., commit)| *commit).collect();
        assert_eq!(flashes.len(), 1, "one flash per burst, got {pushes:?}");
        assert_eq!(
            flashes[0],
            &(vec![HwBand { gain: 3.0, ..band() }], -3.0, true),
            "the flash must persist the final state"
        );
        // Every state change before the flash was applied volatile.
        assert!(pushes.iter().any(|(b, _, commit)| !commit && b[0].gain == 3.0));
    }

    /// Re-committing state the flash already holds (re-clicking the active
    /// preset) writes nothing — inline (DHA15) or debounced.
    #[test]
    fn identical_recommit_never_reflashes() {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(RuntimeStatus::default()));
        let (mut dev, log) = MockDev::new();
        for _ in 0..2 {
            tx.send(Command::Push {
                bands: vec![band()],
                pregain: -3.0,
                commit: true,
            })
            .unwrap();
        }
        tx.send(Command::Stop).unwrap();

        run_loop(&mut dev, &rx, &status, CommitPolicy::Immediate);

        let pushes = log.lock().unwrap();
        assert_eq!(
            pushes.len(),
            1,
            "the identical re-commit must be skipped, got {pushes:?}"
        );
        assert!(pushes[0].2, "the one write is the inline flash");
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
