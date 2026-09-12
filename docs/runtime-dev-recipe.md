# Runtime Development Recipe

This is the practical loop for building a capability against `caps dev`.

The runtime is intentionally small: it accepts JSON-RPC over WebSocket, stores
method contracts by SHA, forwards calls to the owning capability, and hydrates
full contracts only when a caller asks for them.

## Target Developer Flow

```text
provider code
  -> discover IO methods
  -> derive method contracts
  -> runtime.register
  -> runtime stores contract by SHA
  -> callers see compact method refs
  -> runtime forwards calls to the provider
```

The ideal developer experience is that IO methods are pulled directly from the
provider's code at the boundary where outside callers are allowed to interact
with the provider. If a provider has no declared IO surface yet, `caps` should
generate a scaffold that the developer fills in.

## 1. Start The Runtime

Run the local runtime:

```bash
caps dev
```

For predictable local testing:

```bash
caps dev --host 127.0.0.1 --port 9000
```

The runtime prints the WebSocket URL:

```text
ws://127.0.0.1:9000
```

## 2. Define The IO Surface

The IO surface is the set of functions that can be called from outside the
capability. Everything else remains normal internal provider code.

For example, a browser provider might expose:

```text
browser.navigate
browser.screenshot
browser.current_url
```

Each exposed method needs:

- `name`: the JSON-RPC method name.
- `summary`: the compact description shown to callers.
- `params`: the input JSON Schema.
- `result`: the output JSON Schema.
- `execution`: `immediate` today, `job` later.
- `cancelable`: recorded today, acted on later.

## 3. Prefer Code-Derived Contracts

The best version of this flow should discover methods from the provider code
itself, close to the IO boundary.

Examples of acceptable sources:

- Explicit runtime exports, such as a provider-side `register({ ... })` call.
- Annotated functions, such as `#[cap(method = "browser.navigate")]`.
- A typed router table that already describes callable methods.
- Generated schemas from strongly typed params/results.

The provider adapter should convert that source into contract JSON and publish
it with `runtime.register`.

Example registration:

```json
{
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
  "id": 1
}
```

The runtime stores the canonical contract under `caps/.contracts/` and returns a
compact method ref:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "capability": "browser",
    "version": "1.0.0",
    "methods": [
      {
        "name": "browser.navigate",
        "contract_sha": "sha256:f62c3e0502dba35ac5b09003a419b8df6f58f967f393a83b3edb879a6946ed0f",
        "summary": "Navigate the active browser tab to a URL."
      }
    ]
  },
  "id": 1
}
```

## 4. Reconnect With SHAs

After the contract has been published once, the provider can reconnect with a
SHA-only registration:

```json
{
  "jsonrpc": "2.0",
  "method": "runtime.register",
  "params": {
    "capability": "browser",
    "version": "1.0.0",
    "methods": [
      {
        "name": "browser.navigate",
        "contract_sha": "sha256:f62c3e0502dba35ac5b09003a419b8df6f58f967f393a83b3edb879a6946ed0f"
      }
    ]
  },
  "id": 2
}
```

This is the payload-saving path. The SHA does not replace the method name; it
replaces the full contract body.

If the runtime does not know the SHA, it rejects registration with `-32009`.
The provider should then retry with the full contract body.

## 5. Hydrate Only When Needed

Callers can list compact refs:

```json
{ "jsonrpc": "2.0", "method": "runtime.methods", "id": 3 }
```

They hydrate a full contract only when needed:

```json
{
  "jsonrpc": "2.0",
  "method": "runtime.contract",
  "params": {
    "sha": "sha256:f62c3e0502dba35ac5b09003a419b8df6f58f967f393a83b3edb879a6946ed0f"
  },
  "id": 4
}
```

The caller or client library should cache hydrated contracts by SHA.

## 6. Handle Calls At The IO Surface

When another session invokes `browser.navigate`, the runtime forwards the
JSON-RPC request to the session that registered it:

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

The provider executes the corresponding IO method and responds with the same
forwarded ID:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "ok": true
  },
  "id": 1001
}
```

The runtime rewrites the response back to the original caller's ID.

## 7. Scaffold When No IO Surface Exists

If `caps` cannot discover an IO surface, it should generate a scaffold instead
of guessing behavior from arbitrary internal code.

The scaffold should include:

- A provider-side IO module or route table.
- One stub method with params/result placeholders.
- A contract body next to the stub.
- A startup registration hook that publishes the full body first and caches the
  returned SHA for reconnects.

Example scaffold shape:

```text
provider/
  caps.yaml
  caps/
    io.json
    register-runtime.{js,ts,py,rs}
```

The generated method should fail clearly until implemented. The developer then
renames the method, fills in the schema, and wires the stub to real provider
code.

## Current Implementation Boundary

Implemented now:

- Full contract publication through `runtime.register`.
- Canonical SHA computation and storage under `caps/.contracts/`.
- SHA-only reconnect registration.
- `runtime.contract` hydration.
- Compact method refs from `runtime.methods` and lifecycle notifications.

Not implemented yet:

- Automatic code introspection for IO methods.
- Scaffold generation when no IO surface exists.
- Execution modes beyond the current immediate request/response path.
- Grant handles for scoped or obfuscated invocation.
