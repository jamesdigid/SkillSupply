use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use skillsupport::contract::FilesystemContractStore;
use skillsupport::runtime;
use skillsupport::runtime::config::{RuntimeConfig, TransportKind};
use skillsupport::runtime::lifecycle::{LifecycleBus, NotificationBroadcaster};
use skillsupport::runtime::services::registry::RuntimeCapabilityRegistry;
use skillsupport::runtime::transport::{
    Dispatcher, PendingRequests, Router, SessionManager, WebSocketTransport,
    register_runtime_methods,
};
use tempfile::TempDir;
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
        reaper_tick_ms: 10,
        session_ttl_ms: 300_000,
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
fn concurrent_forwards_do_not_serialize() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut caller = runtime.connect();
    let mut capability = runtime.connect();
    let count = 64_u64;

    register_capability(&mut capability, "math", &["math.add"], 20);
    let start = Instant::now();
    for value in 0..count {
        send_request(
            &mut caller,
            "math.add",
            Some(serde_json::json!({ "value": value })),
            100 + value,
        );
    }

    let mut forwarded = Vec::new();
    for _ in 0..count {
        let request = read_request(&mut capability, "math.add");
        forwarded.push((
            request["id"].clone(),
            request["params"]["value"]
                .as_u64()
                .expect("forwarded value should be a u64"),
        ));
    }

    for (forward_id, value) in forwarded.into_iter().rev() {
        send_response(
            &mut capability,
            forward_id,
            serde_json::json!({ "value": value }),
        );
    }

    let responses = read_result_values(&mut caller, 100, count);
    for value in 0..count {
        assert_eq!(responses.get(&(100 + value)), Some(&value));
    }
    assert!(start.elapsed() < Duration::from_secs(2));

    runtime.shutdown();
}

#[test]
fn response_from_non_owner_is_ignored() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut caller = runtime.connect();
    let mut capability = runtime.connect();
    let mut intruder = runtime.connect();

    register_capability(&mut capability, "math", &["math.add"], 60);
    send_request(
        &mut caller,
        "math.add",
        Some(serde_json::json!({ "value": 5 })),
        61,
    );

    let forwarded = read_request(&mut capability, "math.add");
    send_response(
        &mut intruder,
        forwarded["id"].clone(),
        serde_json::json!({ "value": "spoofed" }),
    );
    send_response(
        &mut capability,
        forwarded["id"].clone(),
        serde_json::json!({ "value": 5 }),
    );

    let response = read_until_id(&mut caller, 61);
    assert_eq!(response["result"]["value"], Value::from(5));

    runtime.shutdown();
}

#[test]
fn owner_disconnect_fails_pending_requests() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut caller = runtime.connect();
    let mut capability = runtime.connect();

    register_capability(&mut capability, "math", &["math.add"], 70);
    send_request(&mut caller, "math.add", None, 71);
    let _forwarded = read_request(&mut capability, "math.add");
    capability.close(None).expect("close capability");

    let response = read_until_id(&mut caller, 71);
    assert_eq!(response["error"]["code"], Value::from(-32002));

    runtime.shutdown();
}

#[test]
fn messages_from_one_session_process_in_order() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut client = runtime.connect();

    send_request(
        &mut client,
        "runtime.register",
        Some(serde_json::json!({
            "capability": "math",
            "methods": ["math.add"],
            "version": "1.0.0",
        })),
        80,
    );
    send_request(&mut client, "math.add", None, 81);

    let response = read_until_id(&mut client, 81);
    assert_eq!(response["error"]["code"], Value::from(-32004));

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
fn contract_registration_hydrates_and_reconnects_by_sha() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut caller = runtime.connect();
    let mut capability = runtime.connect();

    let registered = request(
        &mut capability,
        "runtime.register",
        Some(serde_json::json!({
            "capability": "browser",
            "methods": [
                {
                    "name": "browser.navigate",
                    "contract": {
                        "name": "browser.navigate",
                        "version": "1.0.0",
                        "summary": "Navigate the active browser tab to a URL.",
                        "params": {
                            "type": "object",
                            "properties": {
                                "url": { "type": "string" }
                            }
                        },
                        "result": {
                            "type": "object"
                        }
                    }
                }
            ],
            "version": "1.0.0",
        })),
        22,
    );
    let contract_sha = registered["result"]["methods"][0]["contract_sha"]
        .as_str()
        .expect("contract sha")
        .to_string();
    assert!(contract_sha.starts_with("sha256:"));
    assert_eq!(
        registered["result"]["methods"][0]["summary"],
        Value::from("Navigate the active browser tab to a URL.")
    );

    let hydrated = request(
        &mut caller,
        "runtime.contract",
        Some(serde_json::json!({ "sha": contract_sha.clone() })),
        23,
    );
    assert_eq!(hydrated["result"]["name"], Value::from("browser.navigate"));

    #[cfg(debug_assertions)]
    {
        let contracts = request(&mut caller, "runtime.contracts", None, 26);
        assert_eq!(contracts["result"]["count"], Value::from(1));
        assert_eq!(
            contracts["result"]["contracts"][0],
            Value::from(contract_sha.clone())
        );
    }

    capability.close(None).expect("close capability");
    let _ = read_until_method(&mut caller, "lifecycle.capability_unregistered");

    let mut capability = runtime.connect();
    let registered = request(
        &mut capability,
        "runtime.register",
        Some(serde_json::json!({
            "capability": "browser",
            "methods": [
                {
                    "name": "browser.navigate",
                    "contract_sha": contract_sha
                }
            ],
            "version": "1.0.0",
        })),
        24,
    );
    assert_eq!(
        registered["result"]["methods"][0]["summary"],
        Value::from("Navigate the active browser tab to a URL.")
    );

    let methods = request(&mut caller, "runtime.methods", None, 25);
    assert_eq!(
        methods["result"]["capabilities"][0]["methods"][0]["contract_sha"],
        registered["result"]["methods"][0]["contract_sha"]
    );

    runtime.shutdown();
}

#[test]
fn unknown_contract_sha_is_rejected() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut capability = runtime.connect();

    let response = request(
        &mut capability,
        "runtime.register",
        Some(serde_json::json!({
            "capability": "browser",
            "methods": [
                {
                    "name": "browser.navigate",
                    "contract_sha": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }
            ],
            "version": "1.0.0",
        })),
        27,
    );

    assert_eq!(response["error"]["code"], Value::from(-32009));
    runtime.shutdown();
}

#[test]
fn dangling_contract_ref_is_rejected() {
    let runtime = RuntimeHarness::spawn(Duration::from_secs(2));
    let mut capability = runtime.connect();

    let response = request(
        &mut capability,
        "runtime.register",
        Some(serde_json::json!({
            "capability": "browser",
            "methods": [
                {
                    "name": "browser.navigate",
                    "contract": {
                        "name": "browser.navigate",
                        "version": "1.0.0",
                        "summary": "Navigate the active browser tab to a URL.",
                        "params": {
                            "type": "object",
                            "properties": {
                                "url": { "type": "string" }
                            }
                        },
                        "result": {
                            "type": "object"
                        },
                        "refs": [
                            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        ]
                    }
                }
            ],
            "version": "1.0.0",
        })),
        28,
    );

    assert_eq!(response["error"]["code"], Value::from(-32010));
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

fn read_result_values(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    first_id: u64,
    count: u64,
) -> HashMap<u64, u64> {
    let expected = (first_id..first_id + count).collect::<HashSet<_>>();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut responses = HashMap::new();

    while Instant::now() < deadline && responses.len() < expected.len() {
        let value = read_json(socket);
        let Some(id) = value.get("id").and_then(Value::as_u64) else {
            continue;
        };
        if !expected.contains(&id) {
            continue;
        }
        let result = value["result"]["value"]
            .as_u64()
            .expect("response result value should be a u64");
        responses.insert(id, result);
    }

    assert_eq!(responses.len(), expected.len());
    responses
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
    reaper: Option<thread::JoinHandle<()>>,
    sessions: Arc<SessionManager>,
    _workspace: TempDir,
}

impl RuntimeHarness {
    fn spawn(forward_timeout: Duration) -> Self {
        let sessions = Arc::new(SessionManager::default());
        let registry = Arc::new(RuntimeCapabilityRegistry::default());
        let workspace = tempfile::tempdir().expect("temp workspace");
        let contract_store = Arc::new(FilesystemContractStore::new(workspace.path()));
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
            contract_store,
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
        let reaper = spawn_test_reaper(
            dispatcher.clone(),
            Arc::clone(&sessions),
            Arc::clone(&shutdown),
            Duration::from_millis(10),
            Duration::from_secs(300),
        );
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
            reaper: Some(reaper),
            sessions,
            _workspace: workspace,
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
        if let Some(reaper) = self.reaper.take() {
            reaper.join().expect("reaper thread");
        }
    }
}

impl Drop for RuntimeHarness {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        if let Some(reaper) = self.reaper.take() {
            let _ = reaper.join();
        }
    }
}

fn spawn_test_reaper(
    dispatcher: Dispatcher,
    sessions: Arc<SessionManager>,
    shutdown: Arc<AtomicBool>,
    reaper_tick: Duration,
    session_ttl: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            dispatcher.reap_expired_pending();
            sessions.reap_disconnected(session_ttl);
            let sleep_for = dispatcher
                .next_pending_deadline()
                .map(|deadline| {
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(reaper_tick)
                })
                .unwrap_or(reaper_tick);
            thread::sleep(sleep_for);
        }
    })
}
