# Forge Core

Backend service for Automagik Forge. This is a Rust-based API server that manages task orchestration, execution, and management for AI coding agents.

> **Note**: This is the backend repository. The frontend is maintained in the [automagik-forge](https://github.com/namastexlabs/automagik-forge) parent repository.

> **Fork Notice**: This repository is forked from [BloopAI/vibe-kanban](https://github.com/BloopAI/vibe-kanban). After cloning, run `./setup.sh` to configure the correct default repository.

## Quick Start

After cloning, run the setup script:
```bash
./setup.sh
```

This will:
- Set the correct default repository for `gh` CLI
- Install dependencies with `pnpm`
- Install required Rust tools (`cargo-watch`, `sqlx-cli`)

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Node.js](https://nodejs.org/) (>=18)
- [pnpm](https://pnpm.io/) (>=8)
- [GitHub CLI](https://cli.github.com/) (optional, recommended)

Or install manually:
```bash
# Rust development tools
cargo install cargo-watch
cargo install sqlx-cli

# Node dependencies
pnpm i
```

### Running the dev server

```bash
pnpm run dev
```

This will start the backend. A blank DB will be copied from the `dev_assets_seed` folder.

### Build from source

1. Run `build-npm-package.sh`
2. In the `npx-cli` folder run `npm pack`
3. You can run your build with `npx [GENERATED FILE].tgz`


### Environment Variables

- `GITHUB_CLIENT_ID`: GitHub OAuth app client ID (optional, uses default)
- `BACKEND_PORT`: Server port (default: auto-assign)
- `HOST`: Server host (default: 127.0.0.1)

See `CLAUDE.md` for more details.

