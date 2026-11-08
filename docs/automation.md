# Queue automation

Tracker: #782.

## Queue brief

Every queue issue carries a brief a thread starts from without exploring:

```
Goal:
Files: exact paths
Needs: issue numbers that must be closed first
Acceptance: observable criteria
Stage: working | right | fast (one per PR; CLAUDE.md order of work)
Model / turn cap:
```

Pre-flight before a thread starts: every `Needs:` issue is closed and every named file exists;
otherwise stop with a one-line reason (#783).

## Row numbering (#784)

Take numbers last (CLAUDE.md practice 8). If main moved, before merging main run
`uv run python tools/rows.py renumber --base origin/main`; `--check` only reports. Overlapping
spec sections: one spec-writing PR at a time.

## conform/* proposals (#785)

Agents never edit `conformance/`. One branch `conform/<issue>` per proposal, a PR touching only
`conformance/`, titled `conform: <area>`, body linking the issue and the failing output. The lead
reviews them as a batch when the queue ends.

## Workflows (lead merges `.github/`)

- `rerun-infra.yml`: reruns a failed `ci` run once when its failed-job log shows HTTP 5xx on a fetch (#786, #761, #780).
- `merged.yml`: on merge, comments on issues the PR references and appends a line to a log comment on #449 (#787).

## Mac merge rule (#788, not applied)

Mac threads are denied `gh pr merge`. A narrow allow rule in `~/.claude/settings.json` on the Mac:

```
{"permissions": {"allow": ["Bash(gh pr merge * --merge)"]}}
```

`--merge` only (merge commit, per the project rule); other merge modes and pushes still prompt.
