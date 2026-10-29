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

## App language

Everything ScreenGauge shows is in English: toolbar, Settings, notices, tray menu, Windows notifications, installer and Microsoft Store page, with no language option. This overrides the default of Brazilian Portuguese for user-facing text. The README is the one bilingual piece: English, linking to a Portuguese version. Screen text quoted in Portuguese in Linear tickets is the conversation's language; write it in English. Decided in WPC-293.
