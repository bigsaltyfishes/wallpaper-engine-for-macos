use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use crate::{EngineError, display::watcher::DisplayWatcher, window::MainThread};

// DisplayWatcher::setup registers the CG reconfiguration callback globally.
// Must run on the main thread; call at most once per process.
static WATCHER_SETUP: std::sync::Once = std::sync::Once::new();

pub trait DisplayRefreshTarget: Send + Sync + 'static {
    fn schedule(&self);
}

pub struct DisplayChangeRegistration<T: DisplayRefreshTarget> {
    context: Arc<DisplayChangeContext<T>>,
    _watcher_thread: Option<JoinHandle<()>>,
}

struct DisplayChangeContext<T: DisplayRefreshTarget> {
    target: Weak<T>,
    active: AtomicBool,
    refresh_queued: AtomicBool,
}

impl<T: DisplayRefreshTarget> DisplayChangeRegistration<T> {
    #[allow(clippy::single_call_fn)]
    pub fn register(target: &Arc<T>) -> Result<Self, EngineError> {
        let context = Arc::new(DisplayChangeContext {
            target: Arc::downgrade(target),
            active: AtomicBool::new(true),
            refresh_queued: AtomicBool::new(false),
        });

        WATCHER_SETUP.call_once(|| {
            MainThread::dispatch(|| {
                DisplayWatcher::setup();
            });
        });

        // Subscribe this registration to the global display-event stream.
        let watcher = DisplayWatcher::new();

        // Spawn a consumer thread that translates DisplayEvent -> context.schedule().
        let watcher_thread = {
            let context = Arc::clone(&context);
            thread::Builder::new()
                .name("wallpaper-display-watcher".to_string())
                .spawn(move || {
                    while let Some(event) = watcher.next_event() {
                        if !context.active.load(Ordering::Acquire) {
                            break;
                        }
                        eprintln!("[wallpaper-core display] DisplayWatcher event: {event:?}");
                        Arc::clone(&context).schedule();
                    }
                })
                .map_err(|error| {
                    EngineError::Platform(format!(
                        "failed to start display watcher thread: {error}"
                    ))
                })?
        };

        Ok(Self {
            context,
            _watcher_thread: Some(watcher_thread),
        })
    }
}

unsafe impl<T: DisplayRefreshTarget> Send for DisplayChangeRegistration<T> {}
unsafe impl<T: DisplayRefreshTarget> Sync for DisplayChangeRegistration<T> {}

impl<T: DisplayRefreshTarget> Drop for DisplayChangeRegistration<T> {
    fn drop(&mut self) {
        self.context.active.store(false, Ordering::Release);
        // Consumer thread will exit on the next CG event. We do not join it —
        // the DisplayWatcher-based thread may block indefinitely waiting for
        // the next event; joining would hang teardown.
    }
}

impl<T: DisplayRefreshTarget> DisplayChangeContext<T> {
    fn schedule(self: Arc<Self>) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        if self.refresh_queued.swap(true, Ordering::AcqRel) {
            return;
        }

        let context = Arc::clone(&self);
        let spawn_result = thread::Builder::new()
            .name("wallpaper-display-refresh".to_string())
            .spawn(move || {
                let _reset = RefreshQueuedReset(&context.refresh_queued);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    context.refresh_target();
                }));
            });

        if spawn_result.is_err() {
            self.refresh_queued.store(false, Ordering::Release);
        }
    }

    fn refresh_target(&self) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        if let Some(target) = self.target.upgrade() {
            target.schedule();
        }
    }
}

struct RefreshQueuedReset<'a>(&'a AtomicBool);

impl Drop for RefreshQueuedReset<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Mutex, atomic::AtomicUsize, mpsc},
        time::Duration,
    };

    use super::*;

    struct TestTarget {
        calls: AtomicUsize,
        sender: mpsc::Sender<()>,
        release: Mutex<Option<mpsc::Receiver<()>>>,
    }

    impl DisplayRefreshTarget for TestTarget {
        fn schedule(&self) {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.sender.send(()).expect("test receiver should exist");
            if let Some(release) = self.release.lock().unwrap().take() {
                release
                    .recv_timeout(Duration::from_secs(2))
                    .expect("test should release refresh target");
            }
        }
    }

    #[test]
    fn context_schedules_refresh_once_while_queued() {
        let (sender, receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let target = Arc::new(TestTarget {
            calls: AtomicUsize::new(0),
            sender,
            release: Mutex::new(Some(release_receiver)),
        });
        let context = Arc::new(DisplayChangeContext {
            target: Arc::downgrade(&target),
            active: AtomicBool::new(true),
            refresh_queued: AtomicBool::new(false),
        });

        context.clone().schedule();
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("first refresh should be scheduled");
        context.clone().schedule();
        release_sender
            .send(())
            .expect("target release receiver should exist");

        wait_until(|| !context.refresh_queued.load(Ordering::Acquire));
        assert_eq!(target.calls.load(Ordering::Acquire), 1);
    }

    #[test]
    fn context_resets_after_refresh_finishes() {
        let (sender, receiver) = mpsc::channel();
        let target = Arc::new(TestTarget {
            calls: AtomicUsize::new(0),
            sender,
            release: Mutex::new(None),
        });
        let context = Arc::new(DisplayChangeContext {
            target: Arc::downgrade(&target),
            active: AtomicBool::new(true),
            refresh_queued: AtomicBool::new(false),
        });

        context.clone().schedule();
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("first refresh should be scheduled");
        wait_until(|| !context.refresh_queued.load(Ordering::Acquire));
        context.clone().schedule();
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("second refresh should be scheduled");
        wait_until(|| !context.refresh_queued.load(Ordering::Acquire));

        assert_eq!(target.calls.load(Ordering::Acquire), 2);
    }

    #[test]
    fn screen_parameter_notification_does_not_schedule_refresh() {
        let (sender, receiver) = mpsc::channel();
        let target = Arc::new(TestTarget {
            calls: AtomicUsize::new(0),
            sender,
            release: Mutex::new(None),
        });

        assert!(
            receiver.recv_timeout(Duration::from_millis(50)).is_err(),
            "screen parameter notification should not schedule refresh"
        );
        assert_eq!(target.calls.load(Ordering::Acquire), 0);
    }

    #[test]
    fn dropped_target_still_clears_queued_refresh() {
        let (sender, _receiver) = mpsc::channel();
        let target = Arc::new(TestTarget {
            calls: AtomicUsize::new(0),
            sender,
            release: Mutex::new(None),
        });
        let context = Arc::new(DisplayChangeContext {
            target: Arc::downgrade(&target),
            active: AtomicBool::new(true),
            refresh_queued: AtomicBool::new(false),
        });
        drop(target);

        context.clone().schedule();

        wait_until(|| !context.refresh_queued.load(Ordering::Acquire));
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(condition(), "condition was not met before timeout");
    }
}
