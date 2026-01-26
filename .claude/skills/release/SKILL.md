---
name: release
description: Release a new version of git-workflow
argument-hint: <version>
allowed-tools: Bash(mise*), Bash(cargo*), Bash(git*), Bash(gh*), Read, Edit, Grep
---

# git-workflow Release

Release version `$ARGUMENTS`.

## Steps

First, gather current state:
- Run `grep '^version' Cargo.toml` to check current version
- Run `git describe --tags --abbrev=0` to get latest tag
- Run `git log <latest-tag>..HEAD --oneline` to see commits since last release

1. **Validate Version**
   - Verify version format (e.g., 0.1.1)
   - Ensure tag doesn't already exist

2. **Update Cargo.toml**
   - Update `version = "X.X.X"` to new version

3. **Quality Checks**
   ```bash
   mise run verify
   ```
   This runs fmt, lint, test, and build.

4. **Commit**
   - Message: `chore: release vX.X.X`

5. **Generate Release Notes**
   Analyze commits since last release and create notes in this format:

   ```markdown
   ## What's Changed

   ### Features
   - Commits starting with feat:

   ### Bug Fixes
   - Commits starting with fix:

   ### Other Changes
   - Other commits (chore, docs, refactor, etc.)

   **Full Changelog**: https://github.com/lanegrid/git-workflow/compare/v{prev}...v{new}
   ```

6. **Create Tag and Push**
   ```bash
   git tag vX.X.X
   git push origin main
   git push origin vX.X.X
   ```

7. **Create GitHub Release**
   ```bash
   gh release create vX.X.X --title "vX.X.X" --notes "release notes here"
   ```

## Notes

- crates.io publish happens automatically via CI after tag push
- Highlight breaking changes if any
- Keep release notes concise but informative
