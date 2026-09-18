# Stack lifecycle safety

## Problems and ownership

The CLI records one parent and a fork-point SHA per local branch (`gwBase`,
`gwBaseSha`). GitHub records a PR base independently. These edges describe a
single-parent forest, not a general multi-parent dependency DAG. Agents choose
dependencies; correctness of destructive operations must not depend on agents
remembering every edge or the order of a partially completed operation.

Previously, cleanup checked only open GitHub PRs immediately before remote
deletion, after local deletion. Unpublished children were invisible. Sync edited
the PR base before pushing, temporarily removing the remote dependency even
when publication subsequently failed. Retrying could infer a new fork point
from already-rewritten history. A merged parent was assumed to have landed in
the default branch even when its PR targeted another feature branch.

## Implemented invariants

- Cleanup refuses before switching or deleting when any live local branch
  records the target as its parent. This includes unpublished children and
  branches checked out in another linked worktree.
- For a confirmed merged PR, cleanup also checks open remote child PRs before
  local deletion. An unsuccessful dependency query defers cleanup. Remote
  dependencies are checked again immediately before remote deletion.
- `new`, `sync`, and `cleanup` share an advisory lock in the common Git
  directory. A competing command fails with retry guidance. `await` holds no
  lock while polling; its cleanup acquires the same lock.
- Stacked rebases require a recorded fork point still in the child's history.
  Missing metadata never falls back to a moving remote tip or a plain rebase.
- Dropping a merged parent requires its PR to target the default branch and
  its reported merge commit to be reachable from the fetched default branch.
  A parent merged into another feature branch is rejected conservatively.
- Sync persists a plan before rewriting history, publishes with an explicit
  expected remote SHA, then edits the PR base, and only then updates local
  dependency metadata. A retry uses that plan rather than redetecting the base.

## Recovery states

The common Git directory contains `gw-sync.json` while a sync is unfinished.
It records the branch, owning worktree, original/rebased heads, frozen target,
expected remote head, publication checkpoint, and metadata update plan. Writes
use a temporary file and atomic rename. The OS releases the advisory lock when
the process exits; the journal remains to protect dependencies across failures.

| State | Next action |
| --- | --- |
| Rebase conflicted | Resolve and `git rebase --continue`, then `gw sync` |
| Push rejected | Resolve the rejection and rerun `gw sync`; PR base is unchanged |
| PR base update failed after push | Rerun `gw sync`; an already-published head is not rewritten again |
| Want to cancel before publication | `gw sync --abort`; abort an active Git rebase first |
| Want to cancel after publication | Rollback is refused; finish the pending sync |
| Remote or local head changed unexpectedly | Stop without overwriting it; reconcile with the recorded checkpoints |

`gw status` points to the owning worktree and recovery command. Other
`new`/`cleanup` operations and syncs in other worktrees are blocked until this
operation completes or is safely aborted. An abort requires a clean checkout,
an unchanged remote, and a known local checkpoint; it restores the original
head without removing the dependency metadata. Never delete the journal merely
to bypass a guard.

Cleanup deferred by dependencies or lock contention makes an await task exit
with an error. Restack the children, then retry cleanup in the parent's worktree.
It does not resume automatically after another branch's sync finishes.

## Boundaries and follow-up work

This is a conservative single-operation guardrail, not an automatic stack
manager. Serializing mutations and allowing one pending sync per clone favors
safety over concurrent restacks. Independent clones, direct Git/gh commands,
GitHub automatic branch deletion, and other clients do not honor this lock.
GitHub queries and branch deletion cannot be made atomic by a local lock;
server-side rules are necessary for enforcement against other clients.

Remaining work, in priority order:

1. Expose a reconciled stack view (local refs, PR identity/base, checkout,
   fork-point, and pending operations), including missing or conflicting edges.
   Branch names alone are not durable PR identities and config is not cloned.
2. Plan/resume restacks in parent-before-child order across a whole forest;
   provide explicit reparenting and cycle validation. Non-default merge targets
   need an integration-path model rather than assuming every MERGED PR is on main.
3. Make every cleanup partial failure a structured, non-success result; verify
   that local and remote tips contain no post-merge work before force deletion.
4. Add optimistic concurrency checks for PR base changes and conditional remote
   deletion. Current checks protect cooperating local commands, not mutations
   from another clone between an API read and a write.
5. If multiple independent dependencies are required, define how they map to
   GitHub's single-base PR model before representing a general DAG.

Integration tests use real temporary Git repositories with deterministic gh
responses, publish rejection hooks, and shared-worktree locks. They cover local
and remote children, siblings, non-default merges, missing boundaries, failed
publication/retargeting, recovery, cancellation, and concurrent remote changes.
