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

3. Register the capability with one or more method names:

```json
{
  "jsonrpc": "2.0",
  "method": "runtime.register",
  "params": {
    "capability": "browser-attach",
    "version": "1.0.0",
    "methods": ["browser.navigate", "browser.screenshot"]
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

Registers the caller's capability and contributed methods.

Params:

```json
{
  "capability": "browser-attach",
  "version": "1.0.0",
  "methods": ["browser.navigate", "browser.screenshot"]
}
```

Rules:

- Method names must be unique across all connected capabilities.
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
      "methods": ["browser.navigate", "browser.screenshot"]
    }
  ]
}
```

## Method Contracts

Method names are enough for the runtime to route calls, but agents also need method
contracts to understand how to call those methods. CAPS should treat method contracts as
content-addressed, immutable documents.

The intended discovery flow is:

```text
CAPS capability
  -> full canonical contract
Agent runtime/client library
  -> hydrated and cached contract by SHA
LLM
  -> compact interface summary
```

The runtime should avoid returning full hydrated schemas to the LLM by default. Instead,
capabilities advertise compact method references during registration, and the agent
runtime or client library hydrates the full contract only when it needs an uncached SHA.
The model should usually see a minimal useful representation: method name, short
description, required arguments, and other concise calling hints.

Planned method references look like this:

```json
{
  "name": "browser.navigate",
  "contract_sha": "sha256:1e8f6c4c...",
  "summary": "Navigate the active browser tab to a URL."
}
```

The full contract can then be requested by SHA:

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

The hash must represent the canonical contract content, not incidental formatting. Two
contracts with equivalent canonical content should produce the same SHA even if their
source files differ in whitespace, object key order, or other tiny presentation details.
Two semantically different contracts must produce different SHAs.

Contracts are immutable by definition. Once a SHA is published, the content behind that
SHA cannot change. Any contract change, including a schema change or behavioral contract
change, creates a new canonical document and therefore a new SHA. This prevents agents
from learning against one contract and later calling a subtly different one under the
same identifier.

Current implementation note: `caps dev` currently routes registered method names. Contract
references, canonical contract hashing, and `runtime.contract` hydration are protocol
design targets and are not implemented yet.

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
      "methods": ["browser.navigate"]
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
      "browser.navigate",
      "browser.reload",
      "browser.screenshot",
      "browser.evaluate"
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
