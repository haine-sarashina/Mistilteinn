//! Which origins may use the capabilities that need asking about.
//!
//! Notifications, geolocation and reading the clipboard are all things a page
//! must not simply help itself to, so each is gated the same way: an origin
//! starts at `Prompt`, the reader is asked once, and the answer is remembered
//! for the rest of the session. Nothing is remembered across runs — a decision
//! that outlives the browser needs a settings screen to take it back, and there
//! is not one yet.
//!
//! The asking is behind a hook so that tests, and any run with no window
//! server, decide without a dialog.

use std::cell::RefCell;

use rustc_hash::FxHashMap;

/// A capability a page has to be granted before it can use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Notifications,
    Geolocation,
    /// Reading the clipboard. Writing is not gated: a page can only put
    /// something on the board the reader can see and replace, while reading it
    /// takes whatever they last copied, which may be anything at all.
    ClipboardRead,
}

impl Capability {
    /// What the reader is asked, in the language the browser's chrome uses.
    fn question(self, origin: &str) -> String {
        let what = match self {
            Capability::Notifications => "通知の表示",
            Capability::Geolocation => "現在地の取得",
            Capability::ClipboardRead => "クリップボードの読み取り",
        };
        format!("{origin} が{what}を求めています。許可しますか？")
    }

    /// The name this capability goes by in the Permissions API.
    pub fn name(self) -> &'static str {
        match self {
            Capability::Notifications => "notifications",
            Capability::Geolocation => "geolocation",
            Capability::ClipboardRead => "clipboard-read",
        }
    }
}

/// Where an origin stands on one capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionState {
    /// Not asked yet.
    #[default]
    Prompt,
    Granted,
    Denied,
}

impl PermissionState {
    /// The string the web platform uses for this state.
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionState::Prompt => "default",
            PermissionState::Granted => "granted",
            PermissionState::Denied => "denied",
        }
    }
}

/// How the reader is asked. Returns whether they said yes.
type Asker = fn(&str, Capability) -> bool;

/// Put a modal question in front of the reader.
///
/// Blocking is the point: a permission prompt that the page could carry on
/// past would not be a permission prompt.
fn ask_with_a_dialog(origin: &str, capability: Capability) -> bool {
    use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

    MessageDialog::new()
        .set_level(MessageLevel::Info)
        .set_title("Mistilteinn")
        .set_description(capability.question(origin))
        .set_buttons(MessageButtons::YesNo)
        .show()
        == MessageDialogResult::Yes
}

thread_local! {
    static STATES: RefCell<FxHashMap<(String, Capability), PermissionState>> =
        RefCell::new(FxHashMap::default());
    static ASKER: RefCell<Asker> = const { RefCell::new(ask_with_a_dialog) };
    /// The origin whose script is running.
    static ACTIVE_ORIGIN: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Note whose script is about to run, so a request is attributed to it.
pub fn set_active_origin(origin: &str) {
    ACTIVE_ORIGIN.with(|active| *active.borrow_mut() = origin.to_string());
}

/// The origin whose script is running.
pub fn active_origin() -> String {
    ACTIVE_ORIGIN.with(|active| active.borrow().clone())
}

/// Answer permission requests without a dialog. For tests, and for a run with
/// nowhere to put one.
pub fn answer_without_asking(answer: bool) {
    ASKER.with(|asker| {
        *asker.borrow_mut() = if answer {
            |_: &str, _: Capability| true
        } else {
            |_: &str, _: Capability| false
        }
    });
}

/// Forget every decision. Used by tests, and when a profile is reset.
pub fn forget_all() {
    STATES.with(|states| states.borrow_mut().clear());
}

/// Where an origin stands, without asking it anything.
pub fn state(origin: &str, capability: Capability) -> PermissionState {
    STATES.with(|states| {
        states
            .borrow()
            .get(&(origin.to_string(), capability))
            .copied()
            .unwrap_or_default()
    })
}

/// Record a decision without asking. Used when the reader answers elsewhere.
pub fn set_state(origin: &str, capability: Capability, decision: PermissionState) {
    STATES.with(|states| {
        states
            .borrow_mut()
            .insert((origin.to_string(), capability), decision)
    });
}

/// The decision for this origin, asking the reader if it has not been made yet.
///
/// An origin is asked once. Whatever it answers stands for the rest of the
/// session, so a page that calls this in a loop gets one dialog rather than a
/// stream of them.
pub fn request(origin: &str, capability: Capability) -> PermissionState {
    let current = state(origin, capability);
    if current != PermissionState::Prompt {
        return current;
    }
    let granted = ASKER.with(|asker| (*asker.borrow())(origin, capability));
    let decision = if granted {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    };
    set_state(origin, capability, decision);
    decision
}

/// Whether the origin running script may use this capability, asking if needed.
pub fn allowed(capability: Capability) -> bool {
    request(&active_origin(), capability) == PermissionState::Granted
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean slate that never opens a dialog.
    fn with_answer(answer: bool) {
        forget_all();
        answer_without_asking(answer);
        set_active_origin("https://example.com");
    }

    #[test]
    fn an_origin_starts_out_unasked() {
        forget_all();
        assert_eq!(
            state("https://example.com", Capability::Notifications),
            PermissionState::Prompt
        );
        assert_eq!(PermissionState::Prompt.as_str(), "default");
    }

    #[test]
    fn a_yes_is_remembered() {
        with_answer(true);
        assert_eq!(
            request("https://example.com", Capability::Notifications),
            PermissionState::Granted
        );
        // Answering differently now must not change what was already decided.
        answer_without_asking(false);
        assert_eq!(
            request("https://example.com", Capability::Notifications),
            PermissionState::Granted
        );
    }

    #[test]
    fn a_no_is_remembered_too() {
        with_answer(false);
        assert_eq!(
            request("https://example.com", Capability::Geolocation),
            PermissionState::Denied
        );
        answer_without_asking(true);
        assert_eq!(
            request("https://example.com", Capability::Geolocation),
            PermissionState::Denied,
            "a refusal is not re-asked on the next call"
        );
    }

    #[test]
    fn each_capability_is_decided_on_its_own() {
        with_answer(true);
        request("https://example.com", Capability::Notifications);
        assert_eq!(
            state("https://example.com", Capability::Geolocation),
            PermissionState::Prompt
        );
    }

    #[test]
    fn each_origin_is_decided_on_its_own() {
        with_answer(true);
        request("https://example.com", Capability::Notifications);
        assert_eq!(
            state("https://other.example", Capability::Notifications),
            PermissionState::Prompt
        );
    }

    #[test]
    fn the_active_origin_is_the_one_that_gets_asked() {
        with_answer(true);
        set_active_origin("https://asking.example");
        assert!(allowed(Capability::Notifications));
        assert_eq!(
            state("https://asking.example", Capability::Notifications),
            PermissionState::Granted
        );
        assert_eq!(
            state("https://example.com", Capability::Notifications),
            PermissionState::Prompt
        );
    }

    #[test]
    fn a_decision_can_be_set_without_asking_at_all() {
        forget_all();
        answer_without_asking(true);
        set_state(
            "https://example.com",
            Capability::ClipboardRead,
            PermissionState::Denied,
        );
        assert_eq!(
            request("https://example.com", Capability::ClipboardRead),
            PermissionState::Denied
        );
    }

    #[test]
    fn capabilities_have_the_names_the_web_platform_uses() {
        assert_eq!(Capability::Notifications.name(), "notifications");
        assert_eq!(Capability::Geolocation.name(), "geolocation");
        assert_eq!(PermissionState::Granted.as_str(), "granted");
        assert_eq!(PermissionState::Denied.as_str(), "denied");
    }
}
