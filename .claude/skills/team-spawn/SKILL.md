---
name: team-spawn
description: Spawn subagents or Agent Teams for parallel work, with isolated execution via gw worktree pools. Use when running several agents concurrently and deciding how to coordinate and isolate them.
---

# Agent Spawn & Parallel Coordination

Running multiple agents at once comes down to three choices: pick the right
coordination model, isolate their file edits, and drive the Agent Teams
lifecycle.

## Subagents vs Agent Teams — pick the coordination model

| Aspect | Subagent (Task / Agent tool) | Agent Teams (TeamCreate) |
|--------|------------------------------|--------------------------|
| Communication | Returns a result to the lead only | Members message each other directly |
| Coordination | Lead manages all the work | Self-coordinate via a shared task list |
| Best for | One-off independent tasks | Complex work needing collaboration |

**Rule of thumb:** a single independent task → subagent. Multiple agents that
must coordinate → Agent Teams.

## Isolating parallel file edits — worktree pool

When several agents edit files at once, a single working directory causes
overwrites. Give each agent its own pre-warmed `gw` worktree.

**The pool mechanics live in the `git-workflow` skill** (its "running multiple
agents in parallel — worktree pool" section): `gw worktree pool warm / status /
acquire / release / drain`. Don't duplicate them — follow that skill. Three
rules matter most when spawning agents into the pool:

- **Set up the worktree after `acquire`, before launching the agent.** A fresh
  pool worktree has the code but not its dependencies / build artifacts — run
  the project's setup step inside `$WORKTREE_PATH` first, or the agent's first
  commands fail.
- **Always release, even on error.** A forgotten release drains the pool.
- **Never use the agent's own `isolation: worktree`.** `gw` is the single
  worktree authority; a stray worktree breaks `gw cleanup`.

## Agent Teams lifecycle

```
1. TeamCreate          — create the team
2. TaskCreate (×N)     — create the tasks
3. Task / spawn (×N)   — spawn members (set team_name, name)
4. TaskUpdate          — assign tasks (set owner)
5. SendMessage         — coordinate (keep broadcasts minimal)
6. SendMessage(shutdown_request) — stop members when done
7. TeamDelete          — delete the team
```

### Spawn notes

- Members don't inherit the lead's conversation history — put everything they
  need in the prompt.
- If multiple members edit the same file, edits get overwritten — partition the
  file set per member.
- Budget roughly 5–6 tasks per member.
