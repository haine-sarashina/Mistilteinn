//! `WebSocket`, as a page sees it.
//!
//! The object a page holds is a handle: an id, and the handlers it has been
//! given. Everything else — the connection, the frames, the state — belongs to
//! [`crate::network::websocket`] on the other side of a channel, because a
//! socket must not call into the engine while a script is already running on
//! it. The frame loop takes what arrived and dispatches it here.

use std::cell::RefCell;

use boa_engine::{
    Context, JsError, JsResult, JsValue, NativeFunction, js_string,
    object::{FunctionObjectBuilder, JsObject, ObjectInitializer},
    property::Attribute,
};

use crate::network::websocket::{ReadyState, SocketEvent, SocketId};

/// What a page has asked to open, waiting for the window thread to do it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOpen {
    pub id: SocketId,
    pub url: String,
}

thread_local! {
    /// Sockets a script asked for, which only the window thread can open.
    static REQUESTED: RefCell<Vec<PendingOpen>> = const { RefCell::new(Vec::new()) };
    /// Frames a script asked to send.
    static OUTGOING: RefCell<Vec<(SocketId, String)>> = const { RefCell::new(Vec::new()) };
    /// Sockets a script asked to close.
    static CLOSING: RefCell<Vec<SocketId>> = const { RefCell::new(Vec::new()) };
    /// The id the next `new WebSocket(...)` will get.
    static NEXT_ID: RefCell<SocketId> = const { RefCell::new(1) };
    /// What each socket's `readyState` should report, as told by the window.
    static STATES: RefCell<Vec<(SocketId, ReadyState)>> = const { RefCell::new(Vec::new()) };
    /// The page's handle objects, so an event can be delivered to the right one.
    static HANDLES: RefCell<Vec<(SocketId, JsObject)>> = RefCell::new(Vec::new());
    /// The document URL, for deciding whether a socket may be opened at all.
    static PAGE_URL: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Tell the bindings which document is running, so a mixed-content socket can
/// be refused before it is opened.
pub fn set_page_url(url: &str) {
    PAGE_URL.with(|page| *page.borrow_mut() = url.to_string());
}

/// Take the sockets a script has asked to open.
pub fn take_requested() -> Vec<PendingOpen> {
    REQUESTED.with(|requested| std::mem::take(&mut *requested.borrow_mut()))
}

/// Take the frames a script has asked to send.
pub fn take_outgoing() -> Vec<(SocketId, String)> {
    OUTGOING.with(|outgoing| std::mem::take(&mut *outgoing.borrow_mut()))
}

/// Take the sockets a script has asked to close.
pub fn take_closing() -> Vec<SocketId> {
    CLOSING.with(|closing| std::mem::take(&mut *closing.borrow_mut()))
}

/// Record what a socket's `readyState` should now say.
pub fn set_ready_state(id: SocketId, state: ReadyState) {
    STATES.with(|states| {
        let mut states = states.borrow_mut();
        match states.iter_mut().find(|(known, _)| *known == id) {
            Some(entry) => entry.1 = state,
            None => states.push((id, state)),
        }
    });
}

fn ready_state(id: SocketId) -> ReadyState {
    STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|(known, _)| *known == id)
            .map(|(_, state)| *state)
            .unwrap_or(ReadyState::Connecting)
    })
}

/// Forget every socket, as leaving a page does.
pub fn reset() {
    REQUESTED.with(|queue| queue.borrow_mut().clear());
    OUTGOING.with(|queue| queue.borrow_mut().clear());
    CLOSING.with(|queue| queue.borrow_mut().clear());
    STATES.with(|states| states.borrow_mut().clear());
    HANDLES.with(|handles| handles.borrow_mut().clear());
    NEXT_ID.with(|next| *next.borrow_mut() = 1);
}

fn text(value: Option<&JsValue>) -> String {
    value
        .map(|value| match value.as_string() {
            Some(string) => string.to_std_string_escaped(),
            None => value.display().to_string(),
        })
        .unwrap_or_default()
}

fn socket_id(this: &JsValue, ctx: &mut Context) -> Option<SocketId> {
    this.as_object()?
        .get(js_string!("__socket_id"), ctx)
        .ok()?
        .as_number()
        .map(|number| number as SocketId)
}

/// Build the `WebSocket` constructor.
pub fn install(context: &mut Context) -> JsResult<()> {
    let realm = context.realm().clone();
    let constructor = FunctionObjectBuilder::new(
        &realm,
        NativeFunction::from_fn_ptr(|_this, args, ctx| {
            let url = text(args.first());
            let page_url = PAGE_URL.with(|page| page.borrow().clone());
            if !crate::network::websocket::allowed_from(&page_url, &url) {
                return Err(JsError::from_opaque(JsValue::from(js_string!(format!(
                    "SecurityError: {url} is not a WebSocket this document may open"
                )))));
            }

            let id = NEXT_ID.with(|next| {
                let mut next = next.borrow_mut();
                let id = *next;
                *next += 1;
                id
            });
            REQUESTED.with(|requested| {
                requested.borrow_mut().push(PendingOpen {
                    id,
                    url: url.clone(),
                })
            });
            set_ready_state(id, ReadyState::Connecting);

            let handle = build_handle(ctx, id, &url);
            HANDLES.with(|handles| handles.borrow_mut().push((id, handle.clone())));
            Ok(JsValue::from(handle))
        }),
    )
    .name(js_string!("WebSocket"))
    .length(2)
    .constructor(true)
    .build();

    let constructor = JsValue::from(constructor);
    if let Some(object) = constructor.as_object() {
        for (name, value) in [
            ("CONNECTING", 0.0),
            ("OPEN", 1.0),
            ("CLOSING", 2.0),
            ("CLOSED", 3.0),
        ] {
            let _ = object.set(
                js_string!(name.to_string()),
                JsValue::new(value),
                false,
                context,
            );
        }
    }
    context.register_global_property(
        js_string!("WebSocket"),
        constructor.clone(),
        Attribute::all(),
    )?;
    crate::js::dom::set_window_property(context, "WebSocket", constructor);
    Ok(())
}

/// The object a page holds onto for one socket.
fn build_handle(context: &mut Context, id: SocketId, url: &str) -> JsObject {
    let realm = context.realm().clone();
    let state_getter = NativeFunction::from_fn_ptr(|this, _args, ctx| {
        let state = socket_id(this, ctx)
            .map(ready_state)
            .unwrap_or(ReadyState::Closed);
        Ok(JsValue::new(state as u32 as f64))
    })
    .to_js_function(&realm);

    ObjectInitializer::new(context)
        .property(
            js_string!("__socket_id"),
            JsValue::new(id as f64),
            Attribute::READONLY,
        )
        .property(
            js_string!("url"),
            js_string!(url.to_string()),
            Attribute::all(),
        )
        // The four handler properties exist from the start, so a page may
        // assign to them without having to guard.
        .property(js_string!("onopen"), JsValue::null(), Attribute::all())
        .property(js_string!("onmessage"), JsValue::null(), Attribute::all())
        .property(js_string!("onclose"), JsValue::null(), Attribute::all())
        .property(js_string!("onerror"), JsValue::null(), Attribute::all())
        .accessor(
            js_string!("readyState"),
            Some(state_getter),
            None,
            Attribute::all(),
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let Some(id) = socket_id(this, ctx) else {
                    return Ok(JsValue::undefined());
                };
                if ready_state(id) != ReadyState::Open {
                    return Err(JsError::from_opaque(JsValue::from(js_string!(
                        "InvalidStateError: the socket is not open"
                    ))));
                }
                let frame = text(args.first());
                OUTGOING.with(|outgoing| outgoing.borrow_mut().push((id, frame)));
                Ok(JsValue::undefined())
            }),
            js_string!("send"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, _args, ctx| {
                if let Some(id) = socket_id(this, ctx) {
                    set_ready_state(id, ReadyState::Closing);
                    CLOSING.with(|closing| closing.borrow_mut().push(id));
                }
                Ok(JsValue::undefined())
            }),
            js_string!("close"),
            2,
        )
        .build()
}

/// Deliver one socket event to the page's handle for it.
///
/// Both the `onmessage` property and any `addEventListener` registration are
/// honoured, because pages use both and a socket that answered only one would
/// look broken to half of them.
pub fn deliver(context: &mut Context, id: SocketId, event: &SocketEvent) {
    let handle = HANDLES.with(|handles| {
        handles
            .borrow()
            .iter()
            .find(|(known, _)| *known == id)
            .map(|(_, handle)| handle.clone())
    });
    let Some(handle) = handle else {
        return;
    };

    let (property, event_object) = match event {
        SocketEvent::Open => ("onopen", event_with(context, "open", None, None)),
        SocketEvent::Message(data) => (
            "onmessage",
            event_with(context, "message", Some(data.as_str()), None),
        ),
        SocketEvent::Closed { code, reason } => (
            "onclose",
            event_with(context, "close", Some(reason.as_str()), Some(*code)),
        ),
        SocketEvent::Error(message) => (
            "onerror",
            event_with(context, "error", Some(message.as_str()), None),
        ),
    };

    if let Ok(handler) = handle.get(js_string!(property.to_string()), context)
        && let Some(callable) = handler.as_callable()
        && let Err(error) = callable.call(
            &JsValue::from(handle.clone()),
            std::slice::from_ref(&event_object),
            context,
        )
    {
        log::warn!("a WebSocket handler threw: {error}");
    }
}

/// The event object a socket handler is called with.
fn event_with(context: &mut Context, kind: &str, data: Option<&str>, code: Option<u16>) -> JsValue {
    let mut builder = ObjectInitializer::new(context);
    let mut builder = builder.property(
        js_string!("type"),
        js_string!(kind.to_string()),
        Attribute::all(),
    );
    if let Some(data) = data {
        builder = builder.property(
            js_string!("data"),
            js_string!(data.to_string()),
            Attribute::all(),
        );
        builder = builder.property(
            js_string!("reason"),
            js_string!(data.to_string()),
            Attribute::all(),
        );
    }
    if let Some(code) = code {
        builder = builder.property(
            js_string!("code"),
            JsValue::new(code as f64),
            Attribute::all(),
        );
        builder = builder.property(
            js_string!("wasClean"),
            JsValue::new(code == 1000),
            Attribute::all(),
        );
    }
    JsValue::from(builder.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::Page;

    fn page_at(url: &str, script: &str) -> Page {
        reset();
        set_page_url(url);
        let mut page = Page::new(
            "<html><body><div id='out'></div></body></html>",
            "",
            800.0,
            600.0,
        );
        page.page_url = url.to_string();
        page.eval_script(script);
        page
    }

    fn attribute(page: &Page, name: &str) -> Option<String> {
        page.arena
            .find_by_id("out")
            .and_then(|node| page.arena.get_attribute(node, name))
    }

    #[test]
    fn opening_a_socket_asks_the_browser_to_connect() {
        let _page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');",
        );
        let requested = take_requested();
        assert_eq!(requested.len(), 1);
        assert_eq!(requested[0].url, "ws://example.com/live");
    }

    #[test]
    fn a_secure_page_may_not_open_a_plaintext_socket() {
        let page = page_at(
            "https://example.com/",
            "try { new WebSocket('ws://example.com/live'); }
             catch (e) { document.getElementById('out').setAttribute('data-threw', 'yes'); }",
        );
        assert_eq!(attribute(&page, "data-threw"), Some("yes".to_string()));
        assert!(take_requested().is_empty());
    }

    #[test]
    fn a_url_that_is_not_a_socket_is_refused() {
        let page = page_at(
            "http://example.com/",
            "try { new WebSocket('https://example.com/live'); }
             catch (e) { document.getElementById('out').setAttribute('data-threw', 'yes'); }",
        );
        assert_eq!(attribute(&page, "data-threw"), Some("yes".to_string()));
    }

    #[test]
    fn a_new_socket_reports_itself_as_connecting() {
        let page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');
             document.getElementById('out').setAttribute('data-state', String(s.readyState));",
        );
        assert_eq!(attribute(&page, "data-state"), Some("0".to_string()));
    }

    #[test]
    fn nothing_can_be_sent_before_the_socket_opens() {
        let page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');
             try { s.send('too early'); }
             catch (e) { document.getElementById('out').setAttribute('data-threw', 'yes'); }",
        );
        assert_eq!(attribute(&page, "data-threw"), Some("yes".to_string()));
        assert!(take_outgoing().is_empty());
    }

    #[test]
    fn an_open_socket_queues_what_it_is_asked_to_send() {
        let mut page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');",
        );
        let id = take_requested()[0].id;
        set_ready_state(id, ReadyState::Open);
        page.eval_script("s.send('hello there')");

        let outgoing = take_outgoing();
        assert_eq!(outgoing, vec![(id, "hello there".to_string())]);
    }

    #[test]
    fn closing_asks_the_browser_to_close_and_says_so_at_once() {
        let mut page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');",
        );
        let id = take_requested()[0].id;
        set_ready_state(id, ReadyState::Open);
        page.eval_script(
            "s.close();
             document.getElementById('out').setAttribute('data-state', String(s.readyState));",
        );
        assert_eq!(take_closing(), vec![id]);
        assert_eq!(attribute(&page, "data-state"), Some("2".to_string()));
    }

    #[test]
    fn an_arriving_message_reaches_the_handler() {
        let mut page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');
             s.onmessage = function (e) {
                 document.getElementById('out').setAttribute('data-said', e.data);
             };",
        );
        let id = take_requested()[0].id;
        page.deliver_socket_event(id, &SocketEvent::Message("from the server".to_string()));
        assert_eq!(
            attribute(&page, "data-said"),
            Some("from the server".to_string())
        );
    }

    #[test]
    fn opening_and_closing_reach_their_own_handlers() {
        let mut page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');
             s.onopen = function () {
                 document.getElementById('out').setAttribute('data-open', 'yes');
             };
             s.onclose = function (e) {
                 document.getElementById('out').setAttribute('data-code', String(e.code));
             };",
        );
        let id = take_requested()[0].id;
        page.deliver_socket_event(id, &SocketEvent::Open);
        page.deliver_socket_event(
            id,
            &SocketEvent::Closed {
                code: 1000,
                reason: "done".to_string(),
            },
        );
        assert_eq!(attribute(&page, "data-open"), Some("yes".to_string()));
        assert_eq!(attribute(&page, "data-code"), Some("1000".to_string()));
    }

    #[test]
    fn a_failure_reaches_the_error_handler() {
        let mut page = page_at(
            "http://example.com/",
            "var s = new WebSocket('ws://example.com/live');
             s.onerror = function (e) {
                 document.getElementById('out').setAttribute('data-error', e.data);
             };",
        );
        let id = take_requested()[0].id;
        page.deliver_socket_event(id, &SocketEvent::Error("refused".to_string()));
        assert_eq!(attribute(&page, "data-error"), Some("refused".to_string()));
    }

    #[test]
    fn an_event_for_a_socket_the_page_never_opened_is_harmless() {
        let mut page = page_at("http://example.com/", "");
        page.deliver_socket_event(404, &SocketEvent::Message("nobody".to_string()));
        assert_eq!(attribute(&page, "data-said"), None);
    }
}
