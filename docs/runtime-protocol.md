# Runtime Protocol

`caps dev` exposes a local WebSocket JSON-RPC endpoint for external capabilities.
Capabilities connect to the runtime, register the methods they can handle, and then serve
forwarded JSON-RPC requests over the same connection.

## Connection Flow

1. Start the runtime:

```bash
caps dev
```

2. Connect to the transport:

```text
ws://127.0.0.1:8787
```

3. Register the capability:

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

4. Handle forwarded requests:

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

The runtime rewrites request IDs when forwarding and rewrites the response back to the
original caller's ID.

## Built-In Methods

### `runtime.ping`

Returns `"pong"`.

```json
{ "jsonrpc": "2.0", "method": "runtime.ping", "id": 1 }
```

### `runtime.version`

Returns the runtime package name and version.

### `runtime.health`

Returns runtime health, uptime, connected session count, and registered capability count.

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

### `runtime.unregister`

Unregisters the caller's capability methods.

With no params, all capabilities owned by the caller session are removed. With
`{ "capability": "name" }`, only that capability is removed.

### `runtime.methods`

Returns registered capabilities and their methods.

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

Capability-originated notifications with unregistered method names are currently ignored.
This allows capabilities to start emitting future events, such as `browser.page_loaded`,
without breaking the runtime. Event routing and subscriptions are deferred.

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

Browser Attach is the first concrete external capability. The extension acts as a JSON-RPC
server connected to the SkillSupport runtime.

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

For development, image-like responses such as screenshots should be returned as base64 text.
Binary WebSocket frames are intentionally deferred.
