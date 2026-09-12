# Runtime Protocol

`caps dev` starts the local SkillSupport developer runtime. The runtime exposes a
WebSocket JSON-RPC endpoint where external capabilities can connect, register the
methods they handle, and receive forwarded JSON-RPC requests over the same socket.

The implemented transport is WebSocket text frames carrying JSON-RPC 2.0 messages.
Binary WebSocket frames are ignored.

## Implementation Status

Implemented today: session management, capability registration by method name or
contract reference, contract SHA storage and hydration, the built-in `runtime.*`
methods, request forwarding with id rewriting, forward timeouts, and lifecycle
notifications.

Designed but not implemented: execution modes, jobs, progress, cancellation, and
opaque grant handles for scoped invocation.

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
reaper_tick_ms = 50
session_ttl_ms = 300000
```

3. Register the capability with full contracts the first time they are published:

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
      },
      {
        "name": "browser.screenshot",
        "contract_sha": "sha256:7aa2f093..."
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

Registers the caller's capability and contributed methods.

Params:

```json
{
  "capability": "browser-attach",
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
}
```

Rules:

- Method names must be unique across all connected capabilities.
- A method can be a legacy string name, a full contract body, or a `{ "name", "contract_sha" }`
  reference to a contract already stored by the runtime.
- Contract refs point at immutable content by `contract_sha`.
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

### `runtime.contracts`

Debug-only. Registered only in debug builds with `#[cfg(debug_assertions)]`; absent
from release builds.

Returns the contract SHAs currently stored by the local runtime:

```json
{
  "count": 1,
  "contracts": [
    "sha256:f62c3e0502dba35ac5b09003a419b8df6f58f967f393a83b3edb879a6946ed0f"
  ]
}
```

This method is intentionally light: it returns SHAs only, not hydrated contract bodies.
Use `runtime.contract` to hydrate a specific contract.

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

First registration publishes the full contract body:

```json
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
```

The runtime canonicalizes the contract, stores it in `caps/.contracts/`, and returns:

```json
{
  "name": "browser.navigate",
  "contract_sha": "sha256:1e8f6c4c...",
  "summary": "Navigate the active browser tab to a URL."
}
```

Later registrations can use only the SHA:

```json
{
  "name": "browser.navigate",
  "contract_sha": "sha256:1e8f6c4c..."
}
```

If the SHA is unknown, the runtime rejects registration and the capability should
re-register with the full body.

The SHA is computed over canonical contract content, not formatting noise. Objects are
serialized with sorted keys and compact JSON, so whitespace and object key order do not
matter. Equivalent canonical contracts share a SHA; semantic contract changes produce a
new SHA.

Once published, a contract SHA never points at different content. This keeps agents from
learning against one contract and later calling a subtly different one under the same id.

Contracts may reference previously stored contracts through `refs`. A registration with
a dangling `ref` is rejected so contract hydration never returns a pointer the runtime
cannot resolve.

The LLM should not receive hydrated schemas by default. The agent runtime or client
library should cache full contracts and expose only a compact calling interface to the
model.

## Obfuscation and Grants (Design)

Raw contract SHAs are not a security mechanism for hiding skill names. A method name such
as `browser.navigate` has low entropy, so an attacker can hash likely names and compare
the output. `contract_sha` is therefore an integrity and cache key only.

The security-oriented version of this idea is an opaque, random grant handle:

```text
grant: mh_7f3a9c21b8e04d56
  -> method: browser.navigate
  -> contract_sha: sha256:1e8f6c4c...
  -> scope: session or caller
```

Grant handles would be random, scoped, expiring, and revocable. The runtime would keep
the mapping internally and callers would invoke the handle rather than the global method
name. This layer is not implemented yet.

## Execution Modes

A contract declares how its method executes, so callers know the calling convention
before they invoke it:

```json
{
  "name": "migration.run",
  "contract_sha": "sha256:9c02be71...",
  "summary": "Migrate a source site into the target dataset.",
  "execution": "job",
  "cancelable": true
}
```

- `immediate` (default): one forwarded request, one response, bounded by
  `forward_timeout_ms`.
- `job`: the runtime mints a job id and returns it immediately. The capability reports
  progress and completion against that id.

### Immediate Methods

The current forward-and-respond behavior, plus optional progress and cancellation.

### Job Methods

The caller invokes the method normally:

```json
{"jsonrpc":"2.0","method":"migration.run","params":{"source":"wordpress"},"id":7}
```

The runtime responds with a handle instead of a result:

```json
{"jsonrpc":"2.0","result":{"job_id":"job-4f3c1a","status":"running"},"id":7}
```

The runtime forwards the work to the capability as a notification carrying the job id:

```json
{"jsonrpc":"2.0","method":"migration.run","params":{"source":"wordpress"},"job_id":"job-4f3c1a"}
```

The capability emits progress, which the runtime routes to the calling session only:

```json
{"jsonrpc":"2.0","method":"runtime.progress","params":{"job_id":"job-4f3c1a","completed":312,"total":4000,"message":"Uploading assets"}}
```

```json
{"jsonrpc":"2.0","method":"job.progress","params":{"job_id":"job-4f3c1a","completed":312,"total":4000,"message":"Uploading assets"}}
```

The capability finishes with `runtime.job.complete` or `runtime.job.fail`:

```json
{"jsonrpc":"2.0","method":"runtime.job.complete","params":{"job_id":"job-4f3c1a","result":{"documents":4000}}}
```

The caller receives a terminal status notification:

```json
{"jsonrpc":"2.0","method":"job.status","params":{"job_id":"job-4f3c1a","status":"completed","result":{"documents":4000}}}
```

Progress on an immediate method uses the forwarded request `id` in place of `job_id`.

### Cancellation

The caller cancels by request id or job id:

```json
{"jsonrpc":"2.0","method":"runtime.cancel","params":{"job_id":"job-4f3c1a"},"id":8}
```

The runtime marks the job `canceling` and notifies the owner with `job.cancel`.
Cancellation is cooperative: the job is not terminal until the capability confirms with
`runtime.job.complete` or `runtime.job.fail`.

### Job Inspection

- `runtime.job` returns the status of one job.
- `runtime.jobs` lists jobs visible to the caller.

### Correlation

Forwarded request ids are correlated with the owning session, so only that session can
satisfy a pending request. Job ids are opaque tokens rather than sequential counters,
because they are held by callers and survive reconnects.

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
- `-32005`: unknown job
- `-32006`: job canceled
- `-32007`: job orphaned
- `-32008`: contract mismatch or malformed contract
- `-32009`: unknown contract SHA
- `-32010`: dangling contract ref

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
be reused after the old session is disconnected. If the runtime still has the contract in
`caps/.contracts/`, the capability can re-register with `{ "name", "contract_sha" }`
instead of sending the full contract body again.

Jobs outlive sessions, so job ownership is tracked by capability name rather than session
id. When a capability disconnects:

1. In-flight immediate requests fail with `-32002`.
2. Its jobs move to `orphaned`, and callers receive a `job.status` notification.
3. Orphaned jobs expire after `job_ttl_ms`.

After re-registering, a capability calls `runtime.jobs` to find its orphaned jobs, then
either resumes them with `runtime.job.resume` or ends them with `runtime.job.fail`.

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
      },
      {
        "name": "browser.screenshot",
        "contract_sha": "sha256:7aa2f093..."
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
