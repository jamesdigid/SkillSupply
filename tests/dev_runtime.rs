use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use skillsupport::runtime;
use skillsupport::runtime::config::{RuntimeConfig, TransportKind};
use skillsupport::runtime::lifecycle::{LifecycleBus, NotificationBroadcaster};
use skillsupport::runtime::services::registry::RuntimeCapabilityRegistry;
use skillsupport::runtime::transport::{
    Dispatcher, PendingRequests, Router, SessionManager, WebSocketTransport,
    register_runtime_methods,
};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, connect};

#[test]
fn runtime_serve_wires_builtin_methods() {
    let port = open_local_port();
    let config = RuntimeConfig {
        transport: TransportKind::Websocket,
        host: "127.0.0.1".to_string(),
        port,
        forward_timeout_ms: 2_000,
    };
    let shutdown = Arc::new(AtomicBool::new(false));
    let serve_shutdown = Arc::clone(&shutdown);
    let server = thread::spawn(move || {
        runtime::serve(&config, serve_shutdown).expect("serve runtime");
    });

    let url = format!("ws://127.0.0.1:{port}");
    let mut socket = connect_with_retry(&url);
    let ping = request(&mut socket, "runtime.ping", None, 1);
    assert_eq!(ping["result"], Value::from("pong"));

    socket.close(None).expect("close socket");
    shutdown.store(true, Ordering::SeqCst);
    server.join().expect("server thread");
}

#[test]
fn dev_runtime_answers_builtin_methods_and_reconnects() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut socket = runtime.connect();

    let ping = request(&mut socket, "runtime.ping", None, 1);
    assert_eq!(ping["result"], Value::from("pong"));

    let version = request(&mut socket, "runtime.version", None, 2);
    assert_eq!(version["result"]["name"], Value::from("skillsupport"));
    assert_eq!(
        version["result"]["version"],
        Value::from(skillsupport::VERSION)
    );

    let health = request(&mut socket, "runtime.health", None, 3);
    assert_eq!(health["result"]["status"], Value::from("ok"));
    assert_eq!(health["result"]["sessions"], Value::from(1));
    assert_eq!(runtime.sessions.connected_count(), 1);

    socket.close(None).expect("close socket");
    wait_for(|| runtime.sessions.connected_count() == 0);
    assert_eq!(runtime.sessions.count(), 1);

    let mut socket = runtime.connect();
    let ping = request(&mut socket, "runtime.ping", None, 4);
    assert_eq!(ping["result"], Value::from("pong"));
    assert_eq!(runtime.sessions.connected_count(), 1);
    assert_eq!(runtime.sessions.count(), 2);
    socket.close(None).expect("close reconnected socket");

    runtime.shutdown();
}

#[test]
fn forwards_registered_methods_between_sessions() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut caller = runtime.connect();
    let mut capability = runtime.connect();

    register_capability(&mut capability, "math", &["math.add"], 10);
    let registered = read_until_method(&mut caller, "lifecycle.capability_registered");
    assert_eq!(
        registered["params"]["capability"]["capability"],
        Value::from("math")
    );

    send_request(
        &mut caller,
        "math.add",
        Some(serde_json::json!({ "left": 2, "right": 3 })),
        11,
    );
    let forwarded = read_request(&mut capability, "math.add");
    assert_eq!(forwarded["params"]["left"], Value::from(2));
    send_response(
        &mut capability,
        forwarded["id"].clone(),
        serde_json::json!(5),
    );

    let response = read_until_id(&mut caller, 11);
    assert_eq!(response["result"], Value::from(5));

    runtime.shutdown();
}

#[test]
fn duplicate_registration_is_rejected() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut first = runtime.connect();
    let mut second = runtime.connect();

    register_capability(&mut first, "math", &["math.add"], 20);
    let response = request(
        &mut second,
        "runtime.register",
        Some(serde_json::json!({
            "capability": "other",
            "methods": ["math.add"],
        })),
        21,
    );

    assert_eq!(response["error"]["code"], Value::from(-32001));
    runtime.shutdown();
}

#[test]
fn disconnect_unregisters_and_reconnect_can_register_same_methods() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut caller = runtime.connect();
    let mut capability = runtime.connect();

    register_capability(&mut capability, "browser", &["browser.navigate"], 30);
    let _ = read_until_method(&mut caller, "lifecycle.capability_registered");
    capability.close(None).expect("close capability");
    let unregistered = read_until_method(&mut caller, "lifecycle.capability_unregistered");
    assert_eq!(
        unregistered["params"]["capability"]["capability"],
        Value::from("browser")
    );

    let missing = request(&mut caller, "browser.navigate", None, 31);
    assert_eq!(missing["error"]["code"], Value::from(-32601));

    let mut capability = runtime.connect();
    register_capability(&mut capability, "browser", &["browser.navigate"], 32);
    send_request(&mut caller, "browser.navigate", None, 33);
    let forwarded = read_request(&mut capability, "browser.navigate");
    send_response(
        &mut capability,
        forwarded["id"].clone(),
        serde_json::json!({"ok": true}),
    );
    let response = read_until_id(&mut caller, 33);
    assert_eq!(response["result"]["ok"], Value::from(true));

    runtime.shutdown();
}

#[test]
fn forward_timeout_returns_json_rpc_error() {
    let runtime = RuntimeHarness::spawn(Duration::from_millis(50));
    let mut caller = runtime.connect();
    let mut capability = runtime.connect();

    register_capability(&mut capability, "slow", &["slow.wait"], 40);
    send_request(&mut caller, "slow.wait", None, 41);

    let response = read_until_id(&mut caller, 41);
    assert_eq!(response["error"]["code"], Value::from(-32003));

    runtime.shutdown();
}

#[test]
fn unknown_capability_notification_is_ignored() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut capability = runtime.connect();

    capability
        .send(Message::Text(
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "browser.page_loaded",
                "params": { "url": "https://example.com" }
            })
            .to_string()
            .into(),
        ))
        .expect("send notification");

    let ping = request(&mut capability, "runtime.ping", None, 50);
    assert_eq!(ping["result"], Value::from("pong"));

    runtime.shutdown();
}

fn request(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    method: &str,
    params: Option<Value>,
    id: u64,
) -> Value {
    send_request(socket, method, params, id);
    read_until_id(socket, id)
}

fn send_request(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    method: &str,
    params: Option<Value>,
    id: u64,
) {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id,
    });
    socket
        .send(Message::Text(payload.to_string().into()))
        .expect("send request");
}

fn send_response(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    id: Value,
    result: Value,
) {
    socket
        .send(Message::Text(
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": result,
                "id": id,
            })
            .to_string()
            .into(),
        ))
        .expect("send response");
}

fn register_capability(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    capability: &str,
    methods: &[&str],
    id: u64,
) -> Value {
    request(
        socket,
        "runtime.register",
        Some(serde_json::json!({
            "capability": capability,
            "methods": methods,
            "version": "1.0.0",
        })),
        id,
    )
}

fn read_until_id(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    id: u64,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let value = read_json(socket);
        if value.get("id") == Some(&Value::from(id)) {
            return value;
        }
    }
    panic!("response id {id} was not received before deadline");
}

fn read_until_method(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    method: &str,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let value = read_json(socket);
        if value.get("method").and_then(Value::as_str) == Some(method) {
            return value;
        }
    }
    panic!("notification {method} was not received before deadline");
}

fn read_request(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    method: &str,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let value = read_json(socket);
        if value.get("method").and_then(Value::as_str) == Some(method) && value.get("id").is_some()
        {
            return value;
        }
    }
    panic!("request {method} was not received before deadline");
}

fn read_json(socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>) -> Value {
    let message = socket.read().expect("read message");
    let Message::Text(source) = message else {
        panic!("expected text message");
    };
    serde_json::from_str(source.as_str()).expect("json message")
}

fn wait_for(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("condition was not met before deadline");
}

fn open_local_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn connect_with_retry(url: &str) -> tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match connect(url) {
            Ok((mut socket, _)) => {
                if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .expect("set read timeout");
                }
                return socket;
            }
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("connect websocket before deadline: {error}"),
        }
    }
}

struct RuntimeHarness {
    url: String,
    shutdown: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
    sessions: Arc<SessionManager>,
}

impl RuntimeHarness {
    fn spawn(forward_timeout: Duration) -> Self {
        let sessions = Arc::new(SessionManager::default());
        let registry = Arc::new(RuntimeCapabilityRegistry::default());
        let pending = Arc::new(PendingRequests::default());
        let lifecycle = Arc::new(LifecycleBus::default());
        lifecycle.register(Arc::new(NotificationBroadcaster::new(Arc::clone(
            &sessions,
        ))));

        let mut router = Router::default();
        register_runtime_methods(
            &mut router,
            Arc::clone(&sessions),
            Arc::clone(&registry),
            Arc::clone(&lifecycle),
            Instant::now(),
        );
        let router = Arc::new(router);
        let dispatcher = Dispatcher::new(
            router,
            registry,
            Arc::clone(&sessions),
            pending,
            lifecycle,
            forward_timeout,
        );

        let transport = WebSocketTransport::bind("127.0.0.1:0").expect("bind transport");
        let addr = transport.local_addr().expect("local addr");
        let shutdown = Arc::new(AtomicBool::new(false));
        let serve_shutdown = Arc::clone(&shutdown);
        let serve_sessions = Arc::clone(&sessions);
        let server = thread::spawn(move || {
            transport
                .serve(dispatcher, serve_sessions, serve_shutdown)
                .expect("serve transport");
        });

        Self {
            url: format!("ws://{addr}"),
            shutdown,
            server: Some(server),
            sessions,
        }
    }

    fn connect(&self) -> tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>> {
        let mut socket = connect(self.url.as_str()).expect("connect websocket").0;
        if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
        }
        socket
    }

    fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            server.join().expect("server thread");
        }
    }
}

impl Drop for RuntimeHarness {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}
