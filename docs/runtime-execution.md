# Runtime Execution

This document describes how `caps dev` handles concurrent runtime traffic. The
wire protocol lives in [runtime-protocol.md](runtime-protocol.md).

## Problem

The original dispatcher represented each in-flight forward as a parked thread:

```text
incoming text frame
  -> spawn thread
  -> forward request
  -> block on recv_timeout
```

That shape does not scale with fanout. If a caller sends 64 concurrent requests, the
runtime creates 64 parked OS threads even though the runtime is only waiting for
capability responses.

It also weakens correlation. A sequential forward id is fine as an internal JSON-RPC
correlation id, but the response must be accepted only from the session that owns the
forwarded method.

## Model

Forwarding is now record-and-return:

```text
caller request
  -> allocate forward id
  -> store PendingForward
  -> send request to owner session
  -> return
```

The pending map owns the wait state:

```text
forward_id -> caller session, owner session, original id, method, deadline
```

When the owner responds, the dispatcher takes the pending entry only if the responder
matches the recorded owner session. It rewrites the response id back to the caller's
original id and sends the response to the caller's outbound queue.

## Threads

The sync runtime intentionally keeps one thread per WebSocket connection. It no longer
spawns one thread per inbound text frame.

```text
before: 1 accept thread + N connection threads + M in-flight request threads
after:  1 accept thread + N connection threads + 1 pending reaper thread
```

Inbound frames are processed inline on their connection thread. That restores per-session
ordering and removes the fanout failure mode.

Provider runtimes launched during `caps learn` are child processes. They are reaped both
on explicit `RuntimeSession::shutdown` and from `RuntimeSession::drop`, so early returns
from readiness or verification failures do not leave live children or zombies behind.

## Pending Reaper

A single reaper thread handles timeout and cleanup work:

- expired pending forwards receive `-32003`
- disconnected sessions are reaped after `session_ttl_ms`

The reaper sleeps until the next known pending deadline or `reaper_tick_ms`, whichever is
sooner. The deadline heap uses lazy deletion, so canceled or completed entries may remain
in the heap until they reach the top.

## Disconnect Semantics

When a session disconnects:

1. Pending forwards owned by that session fail immediately with `-32002`.
2. Pending forwards called by that session are discarded.
3. Registered capability methods owned by that session are unregistered.
4. Lifecycle disconnect notifications are emitted.
5. The disconnected session record is retained until `session_ttl_ms` expires.

Failed WebSocket handshakes do not create sessions.

## Config

`RuntimeConfig` controls the runtime timing knobs:

```toml
[runtime]
forward_timeout_ms = 30000
reaper_tick_ms = 50
session_ttl_ms = 300000
```

- `forward_timeout_ms`: max time an immediate forwarded request can remain pending
- `reaper_tick_ms`: max sleep between reaper sweeps
- `session_ttl_ms`: retention period for disconnected session metadata

## Deferred Jobs

The same map-based design is the foundation for jobs, progress, and cancellation. Jobs
will need a longer-lived entry keyed by an opaque job id and owned by capability name, not
by session id, so work can survive reconnects.
