use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tungstenite::{accept, Error as WebSocketError, Message};

use crate::error::{Result, SkillSupportError};

use super::dispatch::Dispatcher;
use super::session::SessionManager;

pub struct WebSocketTransport {
    listener: TcpListener,
}

impl WebSocketTransport {
    pub fn bind(addr: impl ToSocketAddrs) -> Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    pub fn serve(
        self,
        dispatcher: Dispatcher,
        sessions: Arc<SessionManager>,
        shutdown: Arc<AtomicBool>,
    ) -> Result<()> {
        while !shutdown.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((stream, peer_addr)) => {
                    let dispatcher = dispatcher.clone();
                    let sessions = Arc::clone(&sessions);
                    let shutdown = Arc::clone(&shutdown);
                    thread::spawn(move || {
                        handle_connection(stream, Some(peer_addr), dispatcher, sessions, shutdown);
                    });
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(SkillSupportError::Transport(error.to_string())),
            }
        }

        Ok(())
    }
}

fn handle_connection(
    stream: std::net::TcpStream,
    peer_addr: Option<SocketAddr>,
    dispatcher: Dispatcher,
    sessions: Arc<SessionManager>,
    shutdown: Arc<AtomicBool>,
) {
    if stream.set_nonblocking(false).is_err() {
        return;
    }

    let Ok(mut socket) = accept(stream) else {
        return;
    };
    if socket.get_mut().set_nonblocking(true).is_err() {
        return;
    }

    let session_id = sessions.register(peer_addr);
    let outbound = sessions.attach_outbound(session_id);
    dispatcher.handle_connect(session_id);

    'connection: while !shutdown.load(Ordering::SeqCst) {
        let mut idle = true;

        loop {
            match outbound.try_recv() {
                Ok(payload) => {
                    idle = false;
                    if socket.send(Message::Text(payload.into())).is_err() {
                        break 'connection;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'connection,
            }
        }

        match socket.read() {
            Ok(message) => {
                idle = false;
                match message {
                    Message::Text(source) => {
                        sessions.touch(session_id);
                        dispatcher.handle_inbound(session_id, source.to_string());
                    }
                    Message::Close(_) => break,
                    Message::Ping(payload) => {
                        if socket.send(Message::Pong(payload)).is_err() {
                            break;
                        }
                    }
                    Message::Pong(_) => sessions.touch(session_id),
                    Message::Binary(_) | Message::Frame(_) => {}
                }
            }
            Err(WebSocketError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
            Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => break,
            Err(_) => break,
        }

        if socket.flush().is_err() {
            break;
        }

        if idle {
            thread::sleep(Duration::from_millis(10));
        }
    }

    sessions.detach_outbound(session_id);
    sessions.mark_disconnected(session_id);
    dispatcher.handle_disconnect(session_id);
}
