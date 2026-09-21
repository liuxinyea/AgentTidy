# Default Workspace Cleanup Safety Rules

This document defines the future cleanup gate for default agent workspaces.
It does **not** enable cleanup in the current read-only release. The rules
implement the safety principles in `Start.md` §16 and must be enforced by the
Phase 6 planner and executor before a workspace can enter a CleanupPlan.

## Eligibility

A workspace may be proposed only when all of the following facts are proven
at plan time and revalidated immediately before execution:

- It is under a provider's documented default-generated workspace root, not a
  user-selected cwd or an arbitrary project directory.
- Its full canonical path has an exact one-to-one relationship with one known
  session. Slug decoding or path-name guesses are never sufficient proof.
- No other session, project record, automation, or workspace registry entry
  references the same path or a descendant of it.
- The directory tree contains no `.git` directory or file at any depth.
- The provider scan has not reported unreadable, linked, unknown, or changed
  entries within the candidate workspace.

Any missing, stale, shared, unknown, or conflicting evidence blocks cleanup.

## User Confirmation

Workspace cleanup requires two separate affirmative confirmations:

1. The user reviews a non-mutating CleanupPlan that names the workspace,
   owning session, measured size, and every failed-or-passed safety gate.
2. Immediately before execution, after revalidation, the user confirms the
   same item again. A changed candidate invalidates the first confirmation and
   requires a new plan review.

Both confirmations must prominently state that the directory can contain
agent-generated files the user may still need, that recovery relies on the OS
Trash, and that AgentTidy cannot guarantee semantic recovery of external
references.

## Execution Boundary

Eligible workspaces are moved to the OS Trash as one directory; AgentTidy
never partially deletes their contents. The executor records the result
without transcript bodies, credentials, or file contents. User-selected
projects, repositories, shared workspaces, and all non-default locations stay
Blocked.
