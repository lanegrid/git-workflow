# git-workflow

A type-safe Git workflow CLI with worktree support and GitHub integration.

## Development

This project uses [mise](https://mise.jdx.dev/) for task running. Tasks are defined in `mise.toml`.

### Available Tasks

| Command | Description |
|---------|-------------|
| `mise run verify` | Run all checks (fmt, lint, test, build) |
| `mise run fmt` | Check code formatting |
| `mise run fmt:fix` | Fix code formatting |
| `mise run lint` | Run clippy lints |
| `mise run lint:fix` | Fix clippy lints |
| `mise run test` | Run tests |
| `mise run build` | Build debug binary |
| `mise run build:release` | Build release binary |

### Before Committing

Always run `mise run verify` before committing to ensure:
- Code is formatted correctly
- No clippy warnings
- All tests pass
- Project builds successfully

### Project Structure

```
src/
├── main.rs          # Entry point
├── lib.rs           # Library root
├── cli.rs           # CLI argument parsing
├── error.rs         # Error types
├── commands/        # Command implementations
├── git/             # Git operations abstraction
├── github/          # GitHub CLI integration
├── state/           # Repository state detection
└── output/          # Terminal output formatting
```

### Release Process

Use the `/release` skill to create a new release:

```
/release <version>
```

This will update version, run checks, create tag, and publish.
