---
name: team-spawn
description: Delegate work to subagents — how long an agent lives, handing work over through brief files, owner vs handed roles, and isolating parallel edits in gw worktree pools. Use when spawning one or more agents or sending follow-up work to them.
---

# Agent Spawn & Coordination

Delegating comes down to three choices: what an agent lives for, how work is
handed over, and how parallel edits are isolated.

## An agent lives as long as its work item

A **work item** is one outcome with its own brief, e.g. "implement X and get it
through review". It ends when its result is accepted, dropped or handed off —
not when the agent's first report lands.

- **Follow-ups go to the agent that holds the context.** Review findings, fixes
  and scope changes: `SendMessage` to it by name. A finished agent resumes from
  its transcript with its history intact.
- **Spawn fresh only when independence is the point**: a fresh-eyes review,
  parallel variants, or a brief that changed so much the old context misleads.
- **Idle agents cost nothing; stopping may be final.** In some setups a stopped
  agent can't be messaged again. Use `TaskStop` to abort running work that is
  no longer wanted, not to tidy up.
- **Name agents after their work item** (`review-auth`, not `reviewer`). A newer
  agent with the same name takes over the address.

Agents spawned in a session are addressable by name; there is no separate team
to create or delete.

## Hand work over through a brief file

Messages to and from agents get truncated, so the substance lives in files.
Each work item has a brief:

```markdown
# <work item>
owner: <who started it>      # publishes (see below)
publish: owner | you         # "you" hands publishing to the agent
rev: 3
goal: …
scope: in … / out …
acceptance: …
decisions: [rev2] … / [rev3] …
open: …
changed in this rev: …
```

- The message is the brief's path plus one or two lines.
- To change direction: edit the brief, bump `rev`, message only
  "brief rev N: <what changed>". Sending it is not the same as the agent having
  switched — check the next report.
- Agents write results to a file next to the brief, state the rev they worked
  from, and reply with the path plus a one- or two-line summary. Check results
  against the current rev, not the one they were built from.

## Owner and handed

Roles follow the work item, not who spawned whom (agents can spawn agents, and
other sessions can hand you work):

- **Owner** — wrote the brief, ran `gw new`, or acquired the pool worktree:
  publishes (push, PR, merge, `gw await`, cleanup, pool release).
- **Handed** — received the item: stages and commits on its branch, and stops
  there unless the brief says `publish: you`. Questions that change goal or
  scope go back to whoever handed it over.

The git side of this is in the `git-workflow` skill ("Who does what").

## Isolating parallel edits — worktree pool

When several agents edit files at once, a single working directory causes
overwrites. Give each agent its own pre-warmed `gw` worktree.

**The pool mechanics live in the `git-workflow` skill** (its "parallel agents —
worktree pool" section): `gw worktree pool warm / status / acquire / release /
drain`. Three rules matter most when spawning agents into the pool:

- **Set up the worktree after `acquire`, before launching the agent.** A fresh
  pool worktree has the code but not its dependencies / build artifacts.
- **Always release, even on error.** A forgotten release drains the pool.
- **Never use the agent's own `isolation: worktree`.** `gw` is the single
  worktree authority; a stray worktree breaks `gw cleanup`.

If several agents must edit the same files, partition the file set per agent
instead — or run them one after another.

## Choosing model and effort

Pick both per job instead of letting the agent inherit the session's setting.
Agent definitions (`.claude/agents/*.md`) can pin `model` and `effort` per role;
override them on the call when the situation differs.
