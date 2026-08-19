//! Notifications a page raises, and the toasts they become.
//!
//! There is no desktop notification service behind this: a notification is
//! shown in the browser's own window, stacked in the corner and expiring on its
//! own. That is a real notification as far as the reader is concerned, and it
//! keeps a page's `new Notification(...)` from being a call that does nothing.
//!
//! A script raises one from inside the JavaScript engine, which has no way to
//! reach the window; the notification goes into a queue here, and the frame
//! loop takes what is in it.

use std::cell::RefCell;
use std::time::{Duration, Instant};

/// How long a toast stays on screen.
pub const TOAST_LIFETIME: Duration = Duration::from_secs(6);

/// The most that are shown at once. Older ones are pushed off the bottom
/// rather than filling the window.
pub const MAX_VISIBLE: usize = 3;

/// What a page asked to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    /// The origin that raised it, shown so the reader knows who is talking.
    pub origin: String,
}

/// A notification on screen, and when it went up.
#[derive(Debug, Clone)]
pub struct Toast {
    pub notification: Notification,
    pub raised_at: Instant,
}

impl Toast {
    /// How far through its life this toast is, from 0 to 1.
    pub fn age(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.raised_at);
        (elapsed.as_secs_f32() / TOAST_LIFETIME.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.raised_at) >= TOAST_LIFETIME
    }
}

thread_local! {
    /// Notifications raised since the frame loop last looked.
    static PENDING: RefCell<Vec<Notification>> = const { RefCell::new(Vec::new()) };
}

/// Raise a notification. Called from the JavaScript engine.
pub fn raise(notification: Notification) {
    log::info!(
        "notification from {}: {}",
        notification.origin,
        notification.title
    );
    PENDING.with(|pending| pending.borrow_mut().push(notification));
}

/// Take everything raised since the last call.
pub fn take_pending() -> Vec<Notification> {
    PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()))
}

/// The toasts currently on screen.
#[derive(Debug, Default)]
pub struct ToastStack {
    toasts: Vec<Toast>,
}

impl ToastStack {
    /// Move anything newly raised onto the stack and drop what has expired.
    ///
    /// Returns whether the stack changed, which is the frame loop's cue to
    /// repaint.
    pub fn update(&mut self, now: Instant) -> bool {
        let raised = take_pending();
        let arrived = !raised.is_empty();
        for notification in raised {
            self.toasts.push(Toast {
                notification,
                raised_at: now,
            });
        }

        let before = self.toasts.len();
        self.toasts.retain(|toast| !toast.expired(now));
        let expired = self.toasts.len() != before;

        // The newest are the ones worth showing, so it is the oldest that go.
        if self.toasts.len() > MAX_VISIBLE {
            let excess = self.toasts.len() - MAX_VISIBLE;
            self.toasts.drain(..excess);
        }

        arrived || expired
    }

    /// The toasts to draw, newest last.
    pub fn visible(&self) -> &[Toast] {
        &self.toasts
    }

    /// Whether anything is showing, and so whether the window has to keep
    /// repainting to age it.
    pub fn is_showing(&self) -> bool {
        !self.toasts.is_empty()
    }

    /// Take one down, as clicking it does.
    pub fn dismiss(&mut self, index: usize) {
        if index < self.toasts.len() {
            self.toasts.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(title: &str) -> Notification {
        Notification {
            title: title.to_string(),
            body: "body".to_string(),
            origin: "https://example.com".to_string(),
        }
    }

    fn drained() -> ToastStack {
        let _ = take_pending();
        ToastStack::default()
    }

    #[test]
    fn a_raised_notification_reaches_the_stack() {
        let mut stack = drained();
        raise(notification("hello"));
        assert!(stack.update(Instant::now()));
        assert_eq!(stack.visible().len(), 1);
        assert_eq!(stack.visible()[0].notification.title, "hello");
    }

    #[test]
    fn taking_the_pending_queue_empties_it() {
        let _ = take_pending();
        raise(notification("once"));
        assert_eq!(take_pending().len(), 1);
        assert!(take_pending().is_empty());
    }

    #[test]
    fn a_quiet_frame_reports_no_change() {
        let mut stack = drained();
        assert!(!stack.update(Instant::now()));
    }

    #[test]
    fn a_toast_comes_down_when_its_time_is_up() {
        let mut stack = drained();
        raise(notification("brief"));
        let now = Instant::now();
        stack.update(now);
        assert!(stack.is_showing());

        let later = now + TOAST_LIFETIME + Duration::from_millis(1);
        assert!(stack.update(later), "the stack changed by losing one");
        assert!(!stack.is_showing());
    }

    #[test]
    fn only_the_newest_few_are_shown() {
        let mut stack = drained();
        for index in 0..MAX_VISIBLE + 2 {
            raise(notification(&format!("n{index}")));
        }
        stack.update(Instant::now());
        assert_eq!(stack.visible().len(), MAX_VISIBLE);
        assert_eq!(
            stack.visible().last().unwrap().notification.title,
            format!("n{}", MAX_VISIBLE + 1),
            "the newest survives"
        );
    }

    #[test]
    fn a_toast_ages_from_nothing_to_everything() {
        let now = Instant::now();
        let toast = Toast {
            notification: notification("aging"),
            raised_at: now,
        };
        assert_eq!(toast.age(now), 0.0);
        assert!((toast.age(now + TOAST_LIFETIME / 2) - 0.5).abs() < 0.05);
        assert_eq!(toast.age(now + TOAST_LIFETIME * 2), 1.0);
    }

    #[test]
    fn a_toast_can_be_taken_down_early() {
        let mut stack = drained();
        raise(notification("a"));
        raise(notification("b"));
        stack.update(Instant::now());
        stack.dismiss(0);
        assert_eq!(stack.visible().len(), 1);
        assert_eq!(stack.visible()[0].notification.title, "b");
        // Dismissing something that is not there must not panic.
        stack.dismiss(9);
    }
}
