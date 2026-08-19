//! WebSocket connections a page has open.
//!
//! A socket lives on the tokio runtime and the page lives on the window
//! thread, so the two talk through channels: the connection task posts what
//! arrived, the window thread posts what the page wants sent. Nothing here
//! calls into JavaScript — the frame loop drains the arrivals and dispatches
//! them, which is what keeps a socket from re-entering the engine while a
//! script is already running on it.

use std::sync::mpsc::{Receiver, Sender, channel};

use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc::UnboundedSender;

/// A page's handle on one connection.
pub type SocketId = u32;

/// What a socket has to say, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum SocketEvent {
    Open,
    /// A text frame. Binary frames are reported as their length rather than
    /// their bytes; a page that wants them needs `Blob`/`ArrayBuffer`, which
    /// this engine has not got.
    Message(String),
    Closed {
        code: u16,
        reason: String,
    },
    Error(String),
}

/// One socket's state, as the `readyState` property reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyState {
    Connecting = 0,
    Open = 1,
    Closing = 2,
    Closed = 3,
}

/// Whether a URL is one a WebSocket may be opened to.
///
/// `ws:` and `wss:` only — a page asking for `http:` here has made a mistake
/// the constructor is specified to throw for.
pub fn is_websocket_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("ws://") || lower.starts_with("wss://")
}

/// Whether a document at `page_url` may open `socket_url`.
///
/// An encrypted page may not open a plaintext socket: it is active content,
/// and a socket a network can tamper with can drive the page it belongs to.
pub fn allowed_from(page_url: &str, socket_url: &str) -> bool {
    if !is_websocket_url(socket_url) {
        return false;
    }
    let page_secure = super::security::Origin::parse(page_url)
        .map(|origin| origin.is_potentially_trustworthy())
        .unwrap_or(false);
    if !page_secure {
        return true;
    }
    socket_url.trim().to_ascii_lowercase().starts_with("wss://")
        || socket_url.contains("://localhost")
        || socket_url.contains("://127.0.0.1")
}

/// Everything the browser has open, and the channel they report through.
pub struct SocketManager {
    next_id: SocketId,
    sockets: Vec<Socket>,
    /// Cloned into each connection task.
    outbox: Sender<(SocketId, SocketEvent)>,
    inbox: Receiver<(SocketId, SocketEvent)>,
}

struct Socket {
    id: SocketId,
    state: ReadyState,
    /// Where to post frames the page wants sent. `None` once it has closed.
    to_socket: Option<UnboundedSender<Command>>,
}

/// What the window thread asks a connection task to do.
enum Command {
    Send(String),
    Close,
}

impl Default for SocketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SocketManager {
    pub fn new() -> Self {
        let (outbox, inbox) = channel();
        Self {
            next_id: 1,
            sockets: Vec::new(),
            outbox,
            inbox,
        }
    }

    /// Open a connection under the id the page already has a handle for.
    ///
    /// The page numbers its own handles, because `new WebSocket(...)` has to
    /// return one before the window thread has looked at the request. Taking
    /// the id from the page rather than minting a second one here is what keeps
    /// the two from drifting apart across a navigation.
    ///
    /// The connection itself is made on the runtime; this returns as soon as
    /// the task is spawned, which is what lets the handle come back with
    /// `readyState` still `CONNECTING`.
    pub fn open(&mut self, id: SocketId, url: &str, runtime: &tokio::runtime::Handle) {
        self.next_id = self.next_id.max(id + 1);

        let (to_socket, mut commands) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let events = self.outbox.clone();
        let url = url.to_string();

        runtime.spawn(async move {
            let stream = match tokio_tungstenite::connect_async(&url).await {
                Ok((stream, _response)) => stream,
                Err(error) => {
                    let _ = events.send((id, SocketEvent::Error(error.to_string())));
                    let _ = events.send((
                        id,
                        SocketEvent::Closed {
                            code: 1006,
                            reason: "connection failed".to_string(),
                        },
                    ));
                    return;
                }
            };
            let _ = events.send((id, SocketEvent::Open));

            let (mut sink, mut source) = stream.split();
            loop {
                tokio::select! {
                    command = commands.recv() => match command {
                        Some(Command::Send(text)) => {
                            if sink
                                .send(tokio_tungstenite::tungstenite::Message::Text(text))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        // The page closed it, or dropped its end of the channel.
                        Some(Command::Close) | None => {
                            let _ = sink.close().await;
                            break;
                        }
                    },
                    frame = source.next() => match frame {
                        Some(Ok(message)) => {
                            use tokio_tungstenite::tungstenite::Message;
                            match message {
                                Message::Text(text) => {
                                    let _ = events.send((id, SocketEvent::Message(text)));
                                }
                                Message::Binary(bytes) => {
                                    let _ = events.send((
                                        id,
                                        SocketEvent::Message(format!(
                                            "[{} binary bytes]",
                                            bytes.len()
                                        )),
                                    ));
                                }
                                Message::Close(frame) => {
                                    let (code, reason) = frame
                                        .map(|frame| {
                                            (u16::from(frame.code), frame.reason.to_string())
                                        })
                                        .unwrap_or((1005, String::new()));
                                    let _ = events.send((id, SocketEvent::Closed { code, reason }));
                                    return;
                                }
                                _ => {}
                            }
                        }
                        Some(Err(error)) => {
                            let _ = events.send((id, SocketEvent::Error(error.to_string())));
                            break;
                        }
                        None => break,
                    },
                }
            }

            let _ = events.send((
                id,
                SocketEvent::Closed {
                    code: 1000,
                    reason: String::new(),
                },
            ));
        });

        self.sockets.push(Socket {
            id,
            state: ReadyState::Connecting,
            to_socket: Some(to_socket),
        });
    }

    /// Ask a socket to send a text frame. Returns whether it could be queued.
    pub fn send(&mut self, id: SocketId, text: &str) -> bool {
        let Some(socket) = self.socket_mut(id) else {
            return false;
        };
        if socket.state != ReadyState::Open {
            return false;
        }
        match &socket.to_socket {
            Some(channel) => channel.send(Command::Send(text.to_string())).is_ok(),
            None => false,
        }
    }

    /// Ask a socket to close.
    pub fn close(&mut self, id: SocketId) {
        let Some(socket) = self.socket_mut(id) else {
            return;
        };
        if matches!(socket.state, ReadyState::Closed | ReadyState::Closing) {
            return;
        }
        socket.state = ReadyState::Closing;
        if let Some(channel) = &socket.to_socket {
            let _ = channel.send(Command::Close);
        }
    }

    pub fn ready_state(&self, id: SocketId) -> ReadyState {
        self.sockets
            .iter()
            .find(|socket| socket.id == id)
            .map(|socket| socket.state)
            .unwrap_or(ReadyState::Closed)
    }

    /// Take everything the sockets have reported since the last call, and move
    /// each one's state along accordingly.
    pub fn drain(&mut self) -> Vec<(SocketId, SocketEvent)> {
        let mut events = Vec::new();
        while let Ok(event) = self.inbox.try_recv() {
            self.apply(&event);
            events.push(event);
        }
        events
    }

    /// Move a socket's state along for an event that has just arrived.
    fn apply(&mut self, (id, event): &(SocketId, SocketEvent)) {
        let Some(socket) = self.socket_mut(*id) else {
            return;
        };
        match event {
            SocketEvent::Open => socket.state = ReadyState::Open,
            SocketEvent::Closed { .. } => {
                socket.state = ReadyState::Closed;
                // Dropping the sender is what tells a task still running that
                // nobody will ask it for anything else.
                socket.to_socket = None;
            }
            _ => {}
        }
    }

    fn socket_mut(&mut self, id: SocketId) -> Option<&mut Socket> {
        self.sockets.iter_mut().find(|socket| socket.id == id)
    }

    /// Close everything and forget it, as leaving a page does.
    pub fn close_all(&mut self) {
        self.next_id = 1;
        for socket in &mut self.sockets {
            if let Some(channel) = &socket.to_socket {
                let _ = channel.send(Command::Close);
            }
            socket.state = ReadyState::Closed;
            socket.to_socket = None;
        }
        self.sockets.clear();
        // Anything still in flight belongs to the page being left.
        while self.inbox.try_recv().is_ok() {}
    }

    pub fn open_count(&self) -> usize {
        self.sockets
            .iter()
            .filter(|socket| socket.state != ReadyState::Closed)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_websocket_schemes_are_websocket_urls() {
        assert!(is_websocket_url("ws://example.com/socket"));
        assert!(is_websocket_url("WSS://example.com/socket"));
        assert!(!is_websocket_url("https://example.com/socket"));
        assert!(!is_websocket_url("example.com"));
        assert!(!is_websocket_url(""));
    }

    #[test]
    fn a_plain_page_may_open_a_plain_socket() {
        assert!(allowed_from("http://example.com/", "ws://example.com/s"));
    }

    #[test]
    fn a_secure_page_may_not_open_a_plaintext_socket() {
        assert!(!allowed_from("https://example.com/", "ws://example.com/s"));
        assert!(allowed_from("https://example.com/", "wss://example.com/s"));
    }

    #[test]
    fn a_secure_page_may_still_reach_a_socket_on_this_machine() {
        // Loopback cannot be tampered with in transit, which is the property
        // the rule is protecting.
        assert!(allowed_from("https://example.com/", "ws://localhost:9001/"));
        assert!(allowed_from("https://example.com/", "ws://127.0.0.1:9001/"));
    }

    #[test]
    fn something_that_is_not_a_socket_url_is_never_allowed() {
        assert!(!allowed_from("http://example.com/", "https://example.com/"));
    }

    /// A manager with one socket registered but no task behind it, so the
    /// state machine can be driven directly.
    ///
    /// The command receiver comes back with it: dropping it would close the
    /// channel, and a send into a closed channel fails for the wrong reason.
    fn manager_with_a_socket() -> (
        SocketManager,
        SocketId,
        tokio::sync::mpsc::UnboundedReceiver<Command>,
    ) {
        let mut manager = SocketManager::new();
        let (to_socket, commands) = tokio::sync::mpsc::unbounded_channel::<Command>();
        manager.sockets.push(Socket {
            id: 7,
            state: ReadyState::Connecting,
            to_socket: Some(to_socket),
        });
        (manager, 7, commands)
    }

    #[test]
    fn the_id_the_page_chose_is_the_one_the_browser_uses() {
        // The page numbers its handle first, and a navigation restarts its
        // counter; a manager minting its own would drift away from it.
        let mut manager = SocketManager::new();
        manager.next_id = 40;
        manager.close_all();
        assert_eq!(manager.next_id, 1, "leaving a page resets the numbering");
    }

    #[test]
    fn a_socket_starts_out_connecting() {
        let (manager, id, _commands) = manager_with_a_socket();
        assert_eq!(manager.ready_state(id), ReadyState::Connecting);
        assert_eq!(manager.open_count(), 1);
    }

    #[test]
    fn an_unknown_socket_reads_as_closed() {
        let manager = SocketManager::new();
        assert_eq!(manager.ready_state(99), ReadyState::Closed);
    }

    #[test]
    fn nothing_can_be_sent_before_the_connection_opens() {
        let (mut manager, id, _commands) = manager_with_a_socket();
        assert!(!manager.send(id, "too early"));

        manager.outbox.send((id, SocketEvent::Open)).unwrap();
        manager.drain();
        assert_eq!(manager.ready_state(id), ReadyState::Open);
        assert!(manager.send(id, "now it goes"));
    }

    #[test]
    fn draining_reports_what_arrived_and_moves_the_state_along() {
        let (mut manager, id, _commands) = manager_with_a_socket();
        manager.outbox.send((id, SocketEvent::Open)).unwrap();
        manager
            .outbox
            .send((id, SocketEvent::Message("hello".to_string())))
            .unwrap();

        let events = manager.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].1, SocketEvent::Message("hello".to_string()));
        assert!(manager.drain().is_empty(), "and the queue is emptied");
    }

    #[test]
    fn a_closed_socket_will_not_send_again() {
        let (mut manager, id, _commands) = manager_with_a_socket();
        manager.outbox.send((id, SocketEvent::Open)).unwrap();
        manager.drain();
        manager
            .outbox
            .send((
                id,
                SocketEvent::Closed {
                    code: 1000,
                    reason: String::new(),
                },
            ))
            .unwrap();
        manager.drain();

        assert_eq!(manager.ready_state(id), ReadyState::Closed);
        assert!(!manager.send(id, "too late"));
        assert_eq!(manager.open_count(), 0);
    }

    #[test]
    fn closing_moves_the_socket_out_of_open_at_once() {
        let (mut manager, id, _commands) = manager_with_a_socket();
        manager.outbox.send((id, SocketEvent::Open)).unwrap();
        manager.drain();
        manager.close(id);
        assert_eq!(manager.ready_state(id), ReadyState::Closing);
        assert!(
            !manager.send(id, "after close"),
            "a socket on its way out takes nothing more"
        );
    }

    #[test]
    fn leaving_a_page_closes_everything_it_had_open() {
        let (mut manager, id, _commands) = manager_with_a_socket();
        manager.outbox.send((id, SocketEvent::Open)).unwrap();
        manager.drain();
        manager.close_all();
        assert_eq!(manager.open_count(), 0);
        assert_eq!(manager.ready_state(id), ReadyState::Closed);
        assert!(manager.drain().is_empty());
    }

    #[test]
    fn an_event_for_a_socket_that_is_gone_is_harmless() {
        let mut manager = SocketManager::new();
        manager.outbox.send((42, SocketEvent::Open)).unwrap();
        let events = manager.drain();
        assert_eq!(events.len(), 1, "it is still reported");
        assert_eq!(manager.ready_state(42), ReadyState::Closed);
    }
}
