# SkillSupport

SkillSupport is an open capability registry for autonomous agents.

It helps agents discover, acquire, initialize, and verify capabilities through interchangeable providers.

## Quick Start

```bash
curl -fsSL https://raw.githubusercontent.com/jamesdigid/SkillSupport/main/install.sh | sh
```

```bash
caps init
caps learn browser-attach
```

`caps init` creates a workspace manifest (`caps.yaml`) plus a `caps/` directory where learned capabilities are installed.
`caps learn` looks up a capability in the registry, resolves a provider, acquires it, initializes it, and runs verification tests before marking it installed.

## Provider Layout

Existing projects become SkillSupport-compatible by adding a lightweight `caps.yaml` manifest.

```text
my-project/
├── caps.yaml
├── prompt.md
├── tests/
└── existing project files...
```



## Current Phase

Phase 1 focuses on bootstrap infrastructure:

- capability discovery
- provider lookup
- installation and acquisition
- runtime initialization
- verification tests
- workspace recording
- release and install smoke checks



## Development

```bash
cargo test
cargo fmt
cargo clippy
```

Requires Rust 1.85+.

### Local Release Build

Build the optimized CLI binary and test it before publishing or installing:

```bash
cargo build --release
./target/release/caps --help
./target/release/caps init --help
```

To test the binary from the local SkillSupport bin directory:

```bash
mkdir -p "$HOME/.caps/bin"
cp ./target/release/caps "$HOME/.caps/bin/caps"
export PATH="$HOME/.caps/bin:$PATH"
caps --help
```

Persist the `PATH` update for new shells:

```bash
echo 'export PATH="$HOME/.caps/bin:$PATH"' >> "$HOME/.zshrc"
```

To try the release binary from anywhere on your machine:

```bash
cargo install --path . --force
caps --help
```

## Runtime Protocol

External capabilities can connect to `caps dev` over WebSocket JSON-RPC, register their methods, and receive lifecycle notifications. See [docs/runtime-protocol.md](docs/runtime-protocol.md) for the protocol reference and [docs/runtime-dev-recipe.md](docs/runtime-dev-recipe.md) for the practical development loop.

## Roadmap

- GitHub-backed provider discovery
- richer provider selection
- semantic search
- remote registries
- broader capability sets such as `browser-generic`



## License

Apache License 2.0