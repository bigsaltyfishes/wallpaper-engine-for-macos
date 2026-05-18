// Each public method is called from a single site in callback.rs; the FFI
// callback is a function pointer. Suppressing at module level is appropriate.
#![allow(clippy::single_call_fn)]

use std::ffi::c_void;

use async_channel::{Receiver, Sender};
use crossbeam_queue::SegQueue;
use objc2_core_graphics::{CGDisplayChangeSummaryFlags, CGDisplayRegisterReconfigurationCallback};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayEvent {
    Connected(u32),
    Disconnected(u32),
    ModeChanged(u32),
    PrimaryChanged(u32),
    MirrorChanged(u32),
    Other(u32, CGDisplayChangeSummaryFlags),
}

static LISTENER_QUEUE: SegQueue<Sender<DisplayEvent>> = SegQueue::new();

/// FFI function pointer passed to `CGDisplayRegisterReconfigurationCallback`.
unsafe extern "C-unwind" fn display_callback(
    display: u32,
    flags: CGDisplayChangeSummaryFlags,
    _user_info: *mut c_void,
) {
    let event = if flags.contains(CGDisplayChangeSummaryFlags::AddFlag) {
        DisplayEvent::Connected(display)
    } else if flags.contains(CGDisplayChangeSummaryFlags::RemoveFlag) {
        DisplayEvent::Disconnected(display)
    } else if flags.contains(CGDisplayChangeSummaryFlags::MovedFlag)
        || flags.contains(CGDisplayChangeSummaryFlags::SetMainFlag)
    {
        DisplayEvent::PrimaryChanged(display)
    } else if flags.contains(CGDisplayChangeSummaryFlags::DesktopShapeChangedFlag)
        || flags.contains(CGDisplayChangeSummaryFlags::SetModeFlag)
    {
        DisplayEvent::ModeChanged(display)
    } else if flags.contains(CGDisplayChangeSummaryFlags::MirrorFlag)
        || flags.contains(CGDisplayChangeSummaryFlags::UnMirrorFlag)
    {
        DisplayEvent::MirrorChanged(display)
    } else if flags.contains(CGDisplayChangeSummaryFlags::BeginConfigurationFlag) {
        return;
    } else {
        DisplayEvent::Other(display, flags)
    };

    for _ in 0..LISTENER_QUEUE.len() {
        if let Some(sender) = LISTENER_QUEUE.pop() {
            let _ = sender.try_send(event.clone());
            LISTENER_QUEUE.push(sender);
        }
    }
}

pub struct DisplayWatcher {
    rx: Receiver<DisplayEvent>,
}

impl DisplayWatcher {
    pub fn setup() {
        unsafe {
            CGDisplayRegisterReconfigurationCallback(Some(display_callback), std::ptr::null_mut());
        }
    }

    pub fn new() -> Self {
        let (tx, rx) = async_channel::unbounded();
        LISTENER_QUEUE.push(tx);
        Self { rx }
    }

    #[must_use]
    pub fn next_event(&self) -> Option<DisplayEvent> {
        self.rx.recv_blocking().ok()
    }

    /// Asynchronously receives the next display event.
    ///
    /// Returns `None` if all senders have been dropped (which does not happen
    /// during normal program lifetime because the global `LISTENER_QUEUE`
    /// keeps senders alive).
    pub async fn recv(&self) -> Option<DisplayEvent> {
        self.rx.recv().await.ok()
    }
}

impl Default for DisplayWatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_watcher_is_public() {
        fn assert_public<T>() {}
        assert_public::<crate::DisplayWatcher>();
        assert_public::<crate::DisplayEvent>();
    }

    #[tokio::test]
    async fn display_watcher_async_recv_delivers_events() {
        let watcher = DisplayWatcher::new();

        let forged = DisplayEvent::Connected(999_999);
        for _ in 0..LISTENER_QUEUE.len() {
            let Some(tx) = LISTENER_QUEUE.pop() else {
                break;
            };
            tx.try_send(forged.clone()).expect("send should succeed");
            LISTENER_QUEUE.push(tx);
        }

        let received = watcher.recv().await.expect("recv should deliver the event");
        assert_eq!(received, forged);
    }
}
