# ScreenGauge

## Agent skills

### Issue tracker

Issues live in Linear, project **ScreenGauge** (team WPC Solutions), via the Linear MCP tools. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), team-scoped in WPC; categories map to Linear's `Bug` / `Feature` / `Improvement`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Task list

`tasks.md` at the repo root stays local: the owner settled it, and `.gitignore` already lists it. Create it straight away when the work needs one.

## Ticket worktrees

Every Linear ticket is built in its own git worktree at `.claude/worktrees/<issue-id-lowercase>` (for example `wpc-297`), on the branch Linear names for the issue (its `gitBranchName`), cut from the current `main`. Tickets run in parallel sessions, and one folder per ticket keeps their changes apart. Tickets reach `main` one at a time, each after the owner approves its merge, and a ticket counts as done only once merged, so every ticket it unblocks starts from a `main` that already holds its code. Remove the worktree after the merge.

## App language

Everything ScreenGauge shows is in English: toolbar, Settings, notices, tray menu, Windows notifications, installer and Microsoft Store page, with no language option. This overrides the default of Brazilian Portuguese for user-facing text. The README is the one bilingual piece: English, linking to a Portuguese version. Screen text quoted in Portuguese in Linear tickets is the conversation's language; write it in English. Decided in WPC-293.
