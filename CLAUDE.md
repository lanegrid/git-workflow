# git-workflow

A type-safe Git workflow CLI with worktree support and GitHub integration.

## Important: Always Create PRs

When making changes to this repository:

1. **Create a feature branch** before making changes: `/git-workflow new <branch-name>`
2. **Run verification** before committing: `mise run verify`
3. **Create a PR** for any changes: `/git-workflow pr "<description>"`
4. **Never push directly to main** - always go through PR workflow

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
| `mise run install` | Build and install gw locally (for dogfooding) |
| `mise run cleanup` | Cleanup merged branch + reinstall (main worktree only) |

### Development Workflow

This project uses `gw` (itself) for git workflow. Use the `/git-workflow` skill:

```bash
/git-workflow new feature/my-feature   # Start new feature branch
/git-workflow pr "Add feature X"       # Create PR
/git-workflow cleanup                  # Clean up after merge
/git-workflow status                   # Check current state
```

Or use `gw` commands directly:

```bash
gw new feature/my-feature    # Create branch from origin/main
gw status                    # Show state and next action
gw pause "WIP message"       # Save work and go home
gw cleanup                   # Delete merged branch
gw home                      # Return to home branch
gw sync                      # Rebase after base PR merged
gw open                      # Open current branch's PR in browser
gw undo                      # Undo last commit
```

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
