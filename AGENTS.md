# AGENTS.md

## What this repo is

A single-file bash application manager with a TUI (powered by
[`gum`](https://github.com/charmbracelet/gum)) and a small CLI. The
catalogue lives in `~/.mytuis.yaml`.

## Files

| File             | Status     | Purpose                                               |
|------------------|------------|-------------------------------------------------------|
| `mytuis.sh`      | tracked    | The whole application. ~920 lines, runnable.          |
| `README.md`      | tracked    | User-facing docs and usage examples.                  |
| `mytuisv1.sh`    | untracked  | Older snapshot kept by the author. **Do not edit.**   |
| `AGENTS.md`      | this file  | Notes for future agents.                              |

There is no test suite, no CI, no linter, and no formatter configured
in this repo. The test scripts that exist live in `/tmp/` and are not
checked in.

## Entry points

`mytuis.sh` has a single entry guard at the very bottom:

```bash
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
```

`main` is a CLI dispatcher: with no args it calls `main_tui`, otherwise
it routes to `cmd_list | cmd_add | cmd_remove | cmd_usage` based on
`$1`. Add new sub-commands there and in the `case` block — don't
invent a second entry point.

## How to validate changes

The repo has no automated test runner. After modifying `mytuis.sh`:

```bash
# 1. Syntax check.
bash -n /datos/tui/mytuis.sh

# 2. Smoke test the CLI surface.
export HOME=/tmp/mytuis_test
rm -rf "$HOME" && mkdir -p "$HOME"
/datos/tui/mytuis.sh help
/datos/tui/mytuis.sh list
/datos/tui/mytuis.sh add firefox "Web browser" firefox
/datos/tui/mytuis.sh add bash "Login shell" bash
/datos/tui/mytuis.sh list | cat
/datos/tui/mytuis.sh remove firefox </dev/null
```

A 32-assertion smoke test for the CLI lives at
`/tmp/awapps_test_cli.sh` (sourced helpers + assertions, not a
checked-in test). It sources the script via an `eval`/`awk` filter
that strips the entry guard; if you refactor the bottom of
`mytuis.sh`, keep the `if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then … fi`
shape or the tests will silently fail to load.

The TUI is harder to test non-interactively. `script -qc … </dev/null`
renders a frame but the gum components time out without input. Trust
the CLI tests for the data layer and verify the TUI visually.

## Internal conventions that matter

- **Internal delimiter is `\x1f` (Unit Separator), stored in
  `DELIM`.** Every helper that talks to `read_apps` / `write_apps`
  must use it. Do not switch to tab or newline — descriptions can
  contain those. `read_apps` emits six fields per record:
  `name<delim>description<delim>path<delim>args<delim>created<delim>last_used`.
- **`resolve_path` communicates via globals, not stdout.** It sets
  `RESOLVED_PATH` and `RESOLVED_ARGS`. Callers MUST invoke it directly
  (`resolve_path "$x"`) — never inside `$()` — because the subshell
  would discard the global assignments. Read the globals afterwards.
- **YAML uses single-quoted strings; internal `'` is escaped as
  `''`.** If you add a new field, mirror the escape rules in
  `write_apps` (use `${var//\'/\'\'}`) and the strip rules in the
  `awk` `extract_value` function in `read_apps`.
- **Atomic writes.** `write_apps` rewrites the whole file on every
  call. Don't add incremental edits — keep the rewrite-the-whole-file
  discipline and the YAML stays consistent.
- **Optional fields are omitted from the YAML when empty.** The
  `args` field is only written when non-empty, so the file stays
  compact. The reader still emits six fields per record (with an
  empty `args` slot), which means all consumers can `read` six
  variables uniformly regardless of file age.
- **`set -euo pipefail` is on.** Declare arrays with `=()` so they
  exist when input is empty (the `cmd_*` and `write_apps` functions
  rely on this).
- **Functions return; CLI sub-commands `exit 1` on error.** This
  asymmetry is intentional — `action_*` are TUI helpers that recover
  and stay in the loop, `cmd_*` are CLI helpers that must propagate
  the failure to the shell.

## Arguments support

The `args` field lets a stored entry launch a command with flags or
operands. The user types the full command line in the Command field
(`ls -lad`, `code /datos/pepe`); `resolve_path` splits the first
whitespace-delimited word off as the executable and stores the rest
in `args`. At launch time `action_run_app` does
`read -ra args_array <<< "$run_args"; exec "$path" "${args_array[@]}"`
which is safe (no glob expansion) and respects quoting when the user
edits the YAML by hand.

The edit form has a separate **Arguments** field. Whatever the user
typed after the executable in the **Command** field is concatenated
with whatever is in the **Arguments** field, in that order.

## Gum gotchas (verified on gum 0.17.0)

- `gum filter` does **not** accept `--selected.foreground`; use
  `--cursor-text.foreground` for the highlighted line.
- `gum table` does **not** accept `--border-foreground`; the border
  colour is fixed.
- `gum table` needs `--print` to emit static output (no TTY mode).
- `gum confirm` exits 0/1 for yes/no; do not pipe it to `head` when
  capturing the exit code, as `head` will return 0 and mask the
  result.

When bumping gum, re-test these four call sites:
`main_tui` (`gum filter`), `action_submenu` (`gum choose`), `cmd_list`
(`gum table`), and `action_delete_app` / `cmd_remove` (`gum confirm`).

## History note

The project was renamed from `awapps` to `mytuis` in commit `b5cd0ce`.
If you bisect or grep the history you'll see both names. The remote is
`https://github.com/horaciod/mytuis.git`.
