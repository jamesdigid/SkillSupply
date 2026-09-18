use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket, connect};

type Socket = WebSocket<MaybeTlsStream<std::net::TcpStream>>;

fn main() -> Result<(), Box<dyn Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ws://127.0.0.1:8787".to_string());

    println!("connecting to {url}");
    let mut socket = connect_runtime(&url)?;

    println!("registering browser.navigate with a full contract body");
    let registered = request(&mut socket, register_full_contract(1), 1)?;
    println!("{}", serde_json::to_string_pretty(&registered)?);

    let contract_sha = registered["result"]["methods"][0]["contract_sha"]
        .as_str()
        .ok_or("runtime.register response did not include a contract_sha")?
        .to_string();
    println!("contract sha: {contract_sha}");

    println!("hydrating contract");
    let hydrated = request(
        &mut socket,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "runtime.contract",
            "params": { "sha": contract_sha.clone() },
            "id": 2,
        }),
        2,
    )?;
    println!("{}", serde_json::to_string_pretty(&hydrated)?);

    println!("reconnecting and registering with SHA only");
    drop(socket);
    let mut socket = connect_runtime(&url)?;
    let compact = retry_sha_registration(&mut socket, &contract_sha)?;
    println!("{}", serde_json::to_string_pretty(&compact)?);

    println!("listing compact runtime methods");
    let methods = request(
        &mut socket,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "runtime.methods",
            "id": 20,
        }),
        20,
    )?;
    println!("{}", serde_json::to_string_pretty(&methods)?);

    Ok(())
}

fn connect_runtime(url: &str) -> Result<Socket, Box<dyn Error>> {
    let (mut socket, _) = connect(url)?;
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    }
    Ok(socket)
}

fn retry_sha_registration(
    socket: &mut Socket,
    contract_sha: &str,
) -> Result<Value, Box<dyn Error>> {
    for attempt in 0..10 {
        let id = 10 + attempt;
        let response = request(socket, register_sha_only(contract_sha, id), id)?;
        if response
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64)
            == Some(-32001)
        {
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        return Ok(response);
    }

    Err("SHA-only registration kept colliding with the previous session".into())
}

fn request(socket: &mut Socket, payload: Value, id: u64) -> Result<Value, Box<dyn Error>> {
    socket.send(Message::Text(payload.to_string().into()))?;
    read_until_id(socket, id)
}

fn read_until_id(socket: &mut Socket, id: u64) -> Result<Value, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let message = socket.read()?;
        let Message::Text(source) = message else {
            continue;
        };
        let value = serde_json::from_str::<Value>(source.as_str())?;
        if value.get("id").and_then(Value::as_u64) == Some(id) {
            return Ok(value);
        }
    }

    Err(format!("response id {id} was not received before deadline").into())
}

fn register_full_contract(id: u64) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "runtime.register",
        "params": {
            "capability": "browser",
            "version": "1.0.0",
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
            ]
        },
        "id": id,
    })
}

fn register_sha_only(contract_sha: &str, id: u64) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "runtime.register",
        "params": {
            "capability": "browser",
            "version": "1.0.0",
            "methods": [
                {
                    "name": "browser.navigate",
                    "contract_sha": contract_sha
                }
            ]
        },
        "id": id,
    })
}
