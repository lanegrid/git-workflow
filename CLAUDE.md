# git-workflow

A type-safe Git workflow CLI (`gw`) with worktree support and GitHub integration.

## Git workflow: read the skill first

**At the start of every session, load the `/git-workflow` skill before doing any
git work.** It is the single source of truth for how this repo uses `gw`
(feature branches, PRs, `gw await`, worktree pools, cleanup). Do not duplicate
that workflow here — follow the skill.

Hard rules (details in the skill):

- Always work on a feature branch; **never push directly to `main`**.
- Run `mise run verify` before committing.
- Every change goes through a PR.

## Development

This project uses [mise](https://mise.jdx.dev/) for task running (see `mise.toml`).

| Command | Description |
|---------|-------------|
| `mise run verify` | Run all checks (fmt, lint, test, build) — run before committing |
| `mise run fmt` / `fmt:fix` | Check / fix formatting |
| `mise run lint` / `lint:fix` | Run / fix clippy lints |
| `mise run test` | Run tests |
| `mise run build` | Build debug binary |
| `mise run install` | Build and install `gw` locally (dogfooding) |

`mise run verify` must pass (formatting, clippy, tests, build) before any commit.

### Dogfooding

We use our own `gw` CLI for this repo's git workflow, so keep the installed
binary current. **After a PR that changes `gw` is merged, run `mise run install`
from the main worktree** to rebuild and reinstall, picking up the latest `gw`.
This is decoupled from `gw cleanup` — cleanup no longer reinstalls.

## Project Structure

Rust binary crate. The layout is roughly:

```
src/
├── main.rs / lib.rs   # Entry point and library root
├── cli.rs             # CLI argument parsing (clap)
├── error.rs           # Error types
├── commands/          # One module per gw subcommand
├── git/               # Git operations (query / mutation)
├── github/            # gh CLI integration (serde-parsed JSON)
├── state/             # Repository / working-dir / next-action detection
├── pool/              # Worktree pool management
└── output/            # Terminal output formatting
```

Tests live in `tests/` (integration) and inline `#[cfg(test)]` modules. The
codebase changes often — treat this as a map, not a spec.

## Release Process

Use the `/release` skill to create a new release:

```
/release <version>
```

This will update version, run checks, create tag, and publish.
