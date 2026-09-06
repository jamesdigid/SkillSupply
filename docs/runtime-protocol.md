# Runtime Protocol

`caps dev` starts the local SkillSupport developer runtime. The runtime exposes a
WebSocket JSON-RPC endpoint where external capabilities can connect, register the
methods they handle, and receive forwarded JSON-RPC requests over the same socket.

The implemented transport is WebSocket text frames carrying JSON-RPC 2.0 messages.
Binary WebSocket frames are ignored.

## Connection Flow

1. Start the runtime:

```bash
caps dev
```

2. Connect to the transport:

```text
ws://127.0.0.1:8787
```

The host and port can be overridden on the command line:

```bash
caps dev --host 127.0.0.1 --port 9000
```

Runtime defaults can also be loaded from a TOML file:

```bash
caps dev --config runtime.toml
```

```toml
[runtime]
transport = "websocket"
host = "127.0.0.1"
port = 8787
forward_timeout_ms = 30000
```

3. Register the capability with compact method references:

```json
{
  "jsonrpc": "2.0",
  "method": "runtime.register",
  "params": {
    "capability": "browser-attach",
    "version": "1.0.0",
    "methods": [
      {
        "name": "browser.navigate",
        "contract_sha": "sha256:1e8f6c4c...",
        "summary": "Navigate the active browser tab to a URL."
      },
      {
        "name": "browser.screenshot",
        "contract_sha": "sha256:7aa2f093...",
        "summary": "Capture a screenshot of the active browser tab."
      }
    ]
  },
  "id": 1
}
```

4. Handle forwarded requests sent by the runtime:

```json
{
  "jsonrpc": "2.0",
  "method": "browser.navigate",
  "params": {
    "url": "https://example.com"
  },
  "id": 42
}
```

5. Return a JSON-RPC response with the same forwarded `id`:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "ok": true
  },
  "id": 42
}
```

For request/response calls, the runtime rewrites request IDs when forwarding and
rewrites the response back to the original caller's ID. Notifications are forwarded
without an `id` and do not produce responses.

## Built-In Methods

### `runtime.ping`

Returns `"pong"`.

```json
{ "jsonrpc": "2.0", "method": "runtime.ping", "id": 1 }
```

### `runtime.version`

Returns the runtime package name and version:

```json
{
  "name": "skillsupport",
  "version": "0.2.0"
}
```

### `runtime.health`

Returns runtime health, uptime, connected session count, and registered capability count:

```json
{
  "status": "ok",
  "uptime_secs": 12,
  "sessions": 2,
  "capabilities": 1
}
```

### `runtime.register`

Registers the caller's capability and contributed method references.

Params:

```json
{
  "capability": "browser-attach",
  "version": "1.0.0",
  "methods": [
    {
      "name": "browser.navigate",
      "contract_sha": "sha256:1e8f6c4c...",
      "summary": "Navigate the active browser tab to a URL."
    }
  ]
}
```

Rules:

- Method names must be unique across all connected capabilities.
- Each method reference points at an immutable contract by `contract_sha`.
- `runtime.*` and `lifecycle.*` are reserved prefixes.
- Registrations are tied to the current session and removed automatically on disconnect.
- A session cannot invoke methods registered by that same session.

### `runtime.unregister`

Unregisters the caller's capability methods.

With no params, all capabilities owned by the caller session are removed. With
`{ "capability": "name" }`, only that capability is removed.

### `runtime.methods`

Returns registered capabilities and their methods:

```json
{
  "capabilities": [
    {
      "capability": "browser-attach",
      "version": "1.0.0",
      "methods": [
        {
          "name": "browser.navigate",
          "contract_sha": "sha256:1e8f6c4c...",
          "summary": "Navigate the active browser tab to a URL."
        },
        {
          "name": "browser.screenshot",
          "contract_sha": "sha256:7aa2f093...",
          "summary": "Capture a screenshot of the active browser tab."
        }
      ]
    }
  ]
}
```

## Method Contracts

CAPS contracts are content-addressed and immutable. Registration returns compact method
refs; the full contract is hydrated by SHA only when needed.

```text
CAPS capability
  -> full canonical contract
Agent runtime/client library
  -> hydrated and cached contract by SHA
LLM
  -> compact interface summary
```

Contract hydration:

```json
{
  "jsonrpc": "2.0",
  "method": "runtime.contract",
  "params": {
    "sha": "sha256:1e8f6c4c..."
  },
  "id": 2
}
```

The SHA is computed over canonical contract content, not formatting noise such as
whitespace or object key order. Equivalent canonical contracts share a SHA; semantic
contract changes produce a new SHA.

Once published, a contract SHA never points at different content. This keeps agents from
learning against one contract and later calling a subtly different one under the same id.

The LLM should not receive hydrated schemas by default. The agent runtime or client
library should cache full contracts and expose only a compact calling interface to the
model.

## Lifecycle Notifications

The runtime pushes lifecycle events to connected sessions as JSON-RPC notifications
(messages with no `id`):

```json
{
  "jsonrpc": "2.0",
  "method": "lifecycle.capability_registered",
  "params": {
    "session_id": "session-2",
    "capability": {
      "capability": "browser-attach",
      "version": "1.0.0",
      "methods": [
        {
          "name": "browser.navigate",
          "contract_sha": "sha256:1e8f6c4c...",
          "summary": "Navigate the active browser tab to a URL."
        }
      ]
    }
  }
}
```

Current notifications:

- `lifecycle.session_connected`
- `lifecycle.capability_registered`
- `lifecycle.capability_unregistered`
- `lifecycle.session_disconnected`
- `lifecycle.runtime_shutdown`

Capability-originated notifications with unregistered method names are ignored. Event
routing and subscriptions are not implemented yet.

## Error Codes

Runtime-specific JSON-RPC errors use the server-error range:

- `-32001`: registration conflict
- `-32002`: forward target disconnected
- `-32003`: forward timeout
- `-32004`: self-invocation

Standard JSON-RPC errors are also used:

- `-32700`: parse error
- `-32600`: invalid request
- `-32601`: method not found
- `-32602`: invalid params

## Reconnect Semantics

Capabilities should treat reconnect as normal. Browser extensions, especially Manifest V3
extensions, may suspend and restart background service workers frequently.

On disconnect, the runtime:

1. Removes all methods owned by the disconnected session.
2. Broadcasts `lifecycle.capability_unregistered`.
3. Broadcasts `lifecycle.session_disconnected`.

On reconnect, the capability should call `runtime.register` again. The same method names can
be reused after the old session is disconnected.

## Browser Extension Example

Browser Attach can act as a JSON-RPC server connected to the SkillSupport runtime.

Registration:

```json
{
  "jsonrpc": "2.0",
  "method": "runtime.register",
  "params": {
    "capability": "browser-attach",
    "version": "1.0.0",
    "methods": [
      {
        "name": "browser.navigate",
        "contract_sha": "sha256:1e8f6c4c...",
        "summary": "Navigate the active browser tab to a URL."
      },
      {
        "name": "browser.screenshot",
        "contract_sha": "sha256:7aa2f093...",
        "summary": "Capture a screenshot of the active browser tab."
      }
    ]
  },
  "id": 1
}
```

When another client calls `browser.navigate`, the runtime forwards the call to the extension:

```json
{
  "jsonrpc": "2.0",
  "method": "browser.navigate",
  "params": {
    "url": "https://example.com"
  },
  "id": 1001
}
```

The extension performs the browser action and responds:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "ok": true,
    "url": "https://example.com"
  },
  "id": 1001
}
```

For development, image-like responses such as screenshots should be returned as base64
text in JSON-RPC results. Binary WebSocket frames are not implemented yet.
