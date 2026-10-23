//! Keeping running workers in line with [`Pools`] ([ADR 0014] §2–4).
//!
//! [`Supervisor::reconcile`] is called in a loop. It starts missing workers, restarts exited
//! ones on the same port (backing off when they crash right after starting), and stops workers
//! that are no longer wanted. Process creation sits behind [`Launcher`], so the logic is tested
//! with fake processes and a fake clock.
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

use crate::pools::{PoolSpec, Pools, WorkerSpec};

/// A failed exit sooner than this after starting backs off before the restart; later failures
/// restart at once.
pub const CRASH_WINDOW: Duration = Duration::from_secs(1);
pub const MIN_BACKOFF: Duration = Duration::from_millis(250);
pub const MAX_BACKOFF: Duration = Duration::from_secs(5);

/// Starts worker processes.
pub trait Launcher {
    fn launch(&mut self, pool: &PoolSpec, worker: &WorkerSpec) -> io::Result<Box<dyn Worker>>;
}

/// A running worker process.
pub trait Worker {
    /// `None` while running; `Some(true)` after a clean exit (php-cgi exits 0 when it reaches
    /// `PHP_FCGI_MAX_REQUESTS`), `Some(false)` after a failure or crash. Non-blocking.
    fn exit_status(&mut self) -> Option<bool>;
    /// Ends it and waits for it.
    fn stop(&mut self);
}

/// What happened during one [`Supervisor::reconcile`] call, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Started {
        pool: String,
        port: u16,
    },
    /// Exited cleanly after serving its requests (normal recycling), or `crashed` (a failed
    /// exit); restarted after `retry_in`.
    Exited {
        pool: String,
        port: u16,
        crashed: bool,
        retry_in: Duration,
    },
    StartFailed {
        pool: String,
        port: u16,
        error: String,
        retry_in: Duration,
    },
    Stopped {
        pool: String,
        port: u16,
    },
}

/// Everything that determines how a worker is started; a change means a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    kind: crate::pools::PoolKind,
    program: std::path::PathBuf,
    ini: std::path::PathBuf,
    fpm_config: Option<std::path::PathBuf>,
    max_requests: u32,
}

struct Slot {
    identity: Identity,
    worker: Option<Box<dyn Worker>>,
    started: Instant,
    backoff: Duration,
    next_start: Instant,
}

/// The running workers, keyed by (pool, port).
pub struct Supervisor<L: Launcher> {
    launcher: L,
    slots: BTreeMap<(String, u16), Slot>,
}

impl<L: Launcher> std::fmt::Debug for Supervisor<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor")
            .field("workers", &self.slots.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl<L: Launcher> Supervisor<L> {
    pub fn new(launcher: L) -> Self {
        Self {
            launcher,
            slots: BTreeMap::new(),
        }
    }

    /// Number of workers currently running.
    #[must_use]
    pub fn running(&self) -> usize {
        self.slots.values().filter(|s| s.worker.is_some()).count()
    }

    /// Brings the running workers in line with `desired` at time `now`.
    pub fn reconcile(&mut self, desired: &Pools, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        let mut wanted = BTreeMap::new();
        for pool in &desired.pools {
            for worker in &pool.workers {
                wanted.insert((pool.id.clone(), worker.port), (pool, *worker));
            }
        }

        // Stop workers that are no longer wanted, or whose settings changed.
        let stale: Vec<(String, u16)> = self
            .slots
            .iter()
            .filter(|(key, slot)| {
                wanted
                    .get(*key)
                    .is_none_or(|(pool, worker)| identity(pool, *worker) != slot.identity)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            if let Some(mut slot) = self.slots.remove(&key) {
                if let Some(mut worker) = slot.worker.take() {
                    worker.stop();
                }
                events.push(Event::Stopped {
                    pool: key.0,
                    port: key.1,
                });
            }
        }

        for (key, (pool, worker_spec)) in wanted {
            let slot = self.slots.entry(key.clone()).or_insert_with(|| Slot {
                identity: identity(pool, worker_spec),
                worker: None,
                started: now,
                backoff: Duration::ZERO,
                next_start: now,
            });

            if let Some(worker) = slot.worker.as_mut() {
                let Some(clean) = worker.exit_status() else {
                    continue;
                };
                slot.worker = None;
                // Only a failed exit counts as a crash: a busy worker can legitimately reach its
                // request limit within a second.
                let crashed = !clean;
                let quick = now.duration_since(slot.started) < CRASH_WINDOW;
                slot.backoff = if crashed && quick {
                    (slot.backoff * 2).clamp(MIN_BACKOFF, MAX_BACKOFF)
                } else {
                    Duration::ZERO
                };
                slot.next_start = now + slot.backoff;
                events.push(Event::Exited {
                    pool: key.0.clone(),
                    port: key.1,
                    crashed,
                    retry_in: slot.backoff,
                });
            }

            if now < slot.next_start {
                continue;
            }
            match self.launcher.launch(pool, &worker_spec) {
                Ok(worker) => {
                    slot.worker = Some(worker);
                    slot.started = now;
                    events.push(Event::Started {
                        pool: key.0,
                        port: key.1,
                    });
                }
                Err(e) => {
                    slot.backoff = (slot.backoff * 2).clamp(MIN_BACKOFF, MAX_BACKOFF);
                    slot.next_start = now + slot.backoff;
                    events.push(Event::StartFailed {
                        pool: key.0,
                        port: key.1,
                        error: e.to_string(),
                        retry_in: slot.backoff,
                    });
                }
            }
        }
        events
    }

    /// Stops every worker.
    pub fn stop_all(&mut self) -> Vec<Event> {
        self.reconcile(&Pools::default(), Instant::now())
    }
}

fn identity(pool: &PoolSpec, worker: WorkerSpec) -> Identity {
    Identity {
        kind: pool.kind,
        program: pool.program.clone(),
        ini: pool.ini.clone(),
        fpm_config: pool.fpm_config.clone(),
        max_requests: worker.max_requests,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Shared view of the fake processes, so tests can make them exit.
    #[derive(Default)]
    struct World {
        launched: Vec<(String, u16, u32)>,
        /// `None` while running, else whether it exited cleanly.
        exits: BTreeMap<u16, Option<bool>>,
        stopped: Vec<u16>,
        fail_next: bool,
    }

    struct FakeLauncher(Rc<RefCell<World>>);
    struct FakeWorker(Rc<RefCell<World>>, u16);

    impl Launcher for FakeLauncher {
        fn launch(&mut self, pool: &PoolSpec, w: &WorkerSpec) -> io::Result<Box<dyn Worker>> {
            let mut world = self.0.borrow_mut();
            if world.fail_next {
                world.fail_next = false;
                return Err(io::Error::other("php-cgi.exe not found"));
            }
            world
                .launched
                .push((pool.id.clone(), w.port, w.max_requests));
            world.exits.insert(w.port, None);
            Ok(Box::new(FakeWorker(Rc::clone(&self.0), w.port)))
        }
    }

    impl Worker for FakeWorker {
        fn exit_status(&mut self) -> Option<bool> {
            self.0.borrow().exits.get(&self.1).copied().flatten()
        }
        fn stop(&mut self) {
            let mut world = self.0.borrow_mut();
            world.exits.insert(self.1, Some(false));
            world.stopped.push(self.1);
        }
    }

    fn pools(id: &str, ports: &[u16]) -> Pools {
        Pools {
            pools: vec![PoolSpec::new(
                id.into(),
                "php-cgi.exe".into(),
                "php.ini".into(),
                ports,
                500,
            )],
        }
    }

    fn setup() -> (Rc<RefCell<World>>, Supervisor<FakeLauncher>) {
        let world = Rc::new(RefCell::new(World::default()));
        let supervisor = Supervisor::new(FakeLauncher(Rc::clone(&world)));
        (world, supervisor)
    }

    #[test]
    fn starts_every_worker_with_its_staggered_limit() {
        let (world, mut sup) = setup();
        let events = sup.reconcile(&pools("8.4", &[9101, 9102, 9103]), Instant::now());
        assert_eq!(events.len(), 3);
        assert_eq!(
            world.borrow().launched,
            [
                ("8.4".into(), 9101, 500),
                ("8.4".into(), 9102, 666),
                ("8.4".into(), 9103, 833)
            ]
        );
        assert_eq!(sup.running(), 3);
    }

    #[test]
    fn a_recycled_worker_restarts_at_once_on_the_same_port() {
        let (world, mut sup) = setup();
        let t0 = Instant::now();
        let desired = pools("8.4", &[9101, 9102]);
        sup.reconcile(&desired, t0);
        world.borrow_mut().exits.insert(9101, Some(true)); // served its 500 requests
        let events = sup.reconcile(&desired, t0 + Duration::from_secs(30));
        assert_eq!(
            events,
            [
                Event::Exited {
                    pool: "8.4".into(),
                    port: 9101,
                    crashed: false,
                    retry_in: Duration::ZERO
                },
                Event::Started {
                    pool: "8.4".into(),
                    port: 9101
                }
            ]
        );
        assert_eq!(world.borrow().launched.len(), 3);
    }

    #[test]
    fn a_crashing_worker_backs_off_up_to_the_cap() {
        let (world, mut sup) = setup();
        let desired = pools("8.4", &[9101]);
        let mut now = Instant::now();
        sup.reconcile(&desired, now);
        let mut waits = Vec::new();
        for _ in 0..8 {
            world.borrow_mut().exits.insert(9101, Some(false)); // fails right after starting
            now += Duration::from_millis(10);
            for event in sup.reconcile(&desired, now) {
                if let Event::Exited {
                    retry_in, crashed, ..
                } = event
                {
                    assert!(crashed);
                    waits.push(retry_in);
                }
            }
            // Nothing restarts before the back-off has passed.
            assert_eq!(sup.running(), 0);
            now += waits.last().copied().unwrap_or_default();
            sup.reconcile(&desired, now);
            assert_eq!(sup.running(), 1);
        }
        assert_eq!(waits[0], MIN_BACKOFF);
        assert_eq!(waits[1], MIN_BACKOFF * 2);
        assert_eq!(*waits.last().unwrap(), MAX_BACKOFF);
    }

    #[test]
    fn a_busy_worker_recycling_within_a_second_is_not_a_crash() {
        let (world, mut sup) = setup();
        let desired = pools("8.4", &[9101]);
        let now = Instant::now();
        sup.reconcile(&desired, now);
        // Reached its request limit 200 ms after starting: clean exit, restart at once.
        world.borrow_mut().exits.insert(9101, Some(true));
        let events = sup.reconcile(&desired, now + Duration::from_millis(200));
        assert!(matches!(
            &events[0],
            Event::Exited { crashed: false, retry_in, .. } if retry_in.is_zero()
        ));
        assert!(matches!(&events[1], Event::Started { .. }));
    }

    #[test]
    fn a_late_failure_restarts_at_once_but_is_reported() {
        let (world, mut sup) = setup();
        let desired = pools("8.4", &[9101]);
        let now = Instant::now();
        sup.reconcile(&desired, now);
        world.borrow_mut().exits.insert(9101, Some(false));
        let events = sup.reconcile(&desired, now + Duration::from_secs(60));
        assert!(matches!(
            &events[0],
            Event::Exited { crashed: true, retry_in, .. } if retry_in.is_zero()
        ));
        assert!(matches!(&events[1], Event::Started { .. }));
    }

    #[test]
    fn launch_failures_are_retried_with_back_off() {
        let (world, mut sup) = setup();
        let desired = pools("8.4", &[9101]);
        let now = Instant::now();
        world.borrow_mut().fail_next = true;
        let events = sup.reconcile(&desired, now);
        assert!(
            matches!(&events[0], Event::StartFailed { retry_in, .. } if *retry_in == MIN_BACKOFF)
        );
        assert_eq!(
            sup.reconcile(&desired, now + Duration::from_millis(100)),
            Vec::new()
        );
        let events = sup.reconcile(&desired, now + MIN_BACKOFF);
        assert!(matches!(&events[0], Event::Started { .. }));
    }

    #[test]
    fn removed_or_changed_workers_are_stopped_and_new_ones_started() {
        let (world, mut sup) = setup();
        let now = Instant::now();
        sup.reconcile(&pools("8.4", &[9101, 9102]), now);
        // Switch to another version on other ports: old workers go, new ones start.
        let events = sup.reconcile(&pools("8.5", &[9201, 9202]), now);
        let stopped: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Stopped { .. }))
            .collect();
        assert_eq!(stopped.len(), 2);
        assert_eq!(world.borrow().stopped, [9101, 9102]);
        assert_eq!(sup.running(), 2);

        // Same port, different limit: restarted with the new setting.
        let mut changed = pools("8.5", &[9201, 9202]);
        changed.pools[0].workers[0].max_requests = 50;
        sup.reconcile(&changed, now);
        assert_eq!(
            world.borrow().launched.last(),
            Some(&("8.5".into(), 9201, 50))
        );

        sup.stop_all();
        assert_eq!(sup.running(), 0);
    }
}
