# Issue tracker: Linear

Issues and specs for this repo live in Linear: team **WPC Solutions** (key `WPC`), project **Pixmeter**. Use the Linear MCP tools for every operation. Their prefix depends on how Linear is connected (currently `mcp__plugin_linear_linear__*`), so look for the operation names used below (`save_issue`, `list_issues`, `get_issue`, `save_comment`, `list_comments`) rather than a fixed prefix.

## Conventions

- **Create an issue**: `save_issue` with `team: "WPC Solutions"`, `project: "Pixmeter"`, `state: "Backlog"`, a `title`, a Markdown `description`, and an `estimate` on the T-shirt scale sent as a number (XS = 1, S = 2, M = 3, L = 5, XL = 8). Search first with `list_issues` (`project: "Pixmeter"`, `query: "..."`) so you don't file a duplicate.
- **Read an issue**: `get_issue` with the identifier (e.g. `WPC-123`); pass `includeRelations: true` when blocking matters. Comments: `list_comments`.
- **List issues**: `list_issues` with `project: "Pixmeter"` plus `state`, `label` or `parentId` filters.
- **Make an issue a sub-issue of a parent**: `save_issue` with `parentId: "WPC-<n>"`, on create or afterwards.
- **Start work on an issue**: `save_issue` with `assignee: "me"` and `state: "In Progress"`, before anything else, so the owner sees in Linear which issue is being worked on. This holds for every issue, wayfinder tickets included.
- **Comment on an issue**: `save_comment` on the issue.
- **Apply / remove labels**: `save_issue` with `addLabels` / `removeLabels`. Create a missing label with `create_issue_label` first.
- **Close**: comment with the outcome, then `save_issue` with `state: "Done"` (or `"Canceled"`).

The `WPC` team is shared with other projects, so always scope lists and searches to `project: "Pixmeter"`.

## When a skill says "publish to the issue tracker"

Create a Linear issue in project Pixmeter, as in **Create an issue** above.

## When a skill says "fetch the relevant ticket"

Read it as in **Read an issue** above.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single issue with **child** issues as tickets.

- **Map**: a single issue in project Pixmeter labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body.
- **Child ticket**: a sub-issue of the map (see **Make an issue a sub-issue of a parent**), labelled `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, the ticket is assigned to the driving dev and In Progress.
- **Blocking**: Linear's native issue relations, the canonical, UI-visible representation. Add an edge with `save_issue` on the child, passing `blockedBy: ["WPC-<n>"]`. A ticket is unblocked when every blocker is in a completed or canceled state.
- **Frontier query**: `list_issues` with `parentId: "<map>"`, open states only; drop any with an open blocker (`get_issue` with `includeRelations: true`) or an assignee; first in map order wins.
- **Claim**: `save_issue` with `assignee: "me"` and `state: "In Progress"`, the session's first write (see **Start work on an issue**). A research ticket handed to a subagent is claimed this way when the subagent is fired.
- **Resolve**: `save_comment` with the answer, then `save_issue` with `state: "Done"`, then append a context pointer (gist + link) to the map's Decisions-so-far.
