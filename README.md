# mytuis — Application Manager

A small, attractive terminal UI for managing a personal catalogue of
applications. Built with [gum](https://github.com/charmbracelet/gum) and
plain bash, with persistent storage in a human-readable YAML file.

The script exposes two surfaces:

- An **interactive TUI** (just run `mytuis` with no arguments). The
  first thing you see is the catalogue list with two meta entries
  (`[+] Add new application` and `[x] Exit`); pick an app to open a
  per-app sub-menu with Run / Edit / Delete / Back.
- A small **command-line interface** for scripting and quick operations:
  `mytuis list`, `mytuis add [name desc path]`, `mytuis remove <name>`
  and `mytuis help`.

```
╔═════════════════════════════════════╗
║  mytuis  ::  Application Manager    ║
╚═════════════════════════════════════╝

Pick an app, add a new one, or exit (type to filter):
▶ [+]
  firefox — Web browser
  bash    — Login shell
  git     — Distributed VCS
  [x]
```

## Features

- **List-first TUI** — the catalogue is the entry point. From any app
  row you can Run, Edit or Delete in one extra keystroke; meta entries
  let you add or exit without leaving the screen.
- **Filterable list** — type to narrow the matches across both apps
  and meta entries.
- **CLI** — `list`, `add` and `remove` work non-interactively so you can
  script the catalogue or seed it from a dotfiles repo.
- **Smart path handling** — accepts absolute paths (`/usr/bin/firefox`),
  relative paths (`./scripts/myscript.sh`), tilde paths (`~/bin/foo`)
  and bare command names looked up in `$PATH` (`firefox`).
- **Persistent metadata** — every entry stores its name, description,
  absolute path, creation date and last-used date.
- **YAML storage** — the catalogue lives in `~/.mytuis.yaml` and can be
  inspected, edited or backed up with any text editor.
- **Friendly TUI** — clear menus, color-coded messages and clean
  borders, all powered by `gum`.

## Requirements

- **bash ≥ 4**
- **[gum](https://github.com/charmbracelet/gum)** — install with
  `brew install gum` (macOS), `pacman -S gum` (Arch),
  `apt install gum` (some Debian-based distros) or see the
  [official installation guide](https://github.com/charmbracelet/gum#installation).
- Standard Unix utilities: `awk`, `sed`, `grep`, `date`, `column`.

## Installation

1. Install `gum` using your package manager.
2. Copy `mytuis.sh` somewhere on your `$PATH`, for example:
   ```bash
   install -m 755 mytuis.sh /usr/local/bin/mytuis
   ```
   Or simply run it from the cloned repository:
   ```bash
   ./mytuis.sh
   ```

The script creates `~/.mytuis.yaml` automatically on the first run.

## Usage

```
mytuis                       Open the interactive TUI.
mytuis list                  List all registered applications.
mytuis add [name desc path]  Add an application. Interactive if no
                             arguments are provided.
mytuis remove <name>         Remove an application by name.
mytuis help                  Show this help message.
```

### Interactive TUI

When you run `mytuis` with no arguments the script shows the catalogue
list right away. The list contains:

- `[+] Add new application` — opens the three-step form (name,
  description, path).
- One row per registered app, formatted as `name — description`.
- `[x] Exit` — quits the manager.

Type to filter; use ↑/↓ to move; Enter to select.

When you pick an app, a sub-menu opens with:

| Option                  | What it does                                  |
|-------------------------|-----------------------------------------------|
| Run this application    | Launches the app via `exec`.                  |
| Edit this application   | Pre-fills the form with current values.       |
| Delete this application | Asks for confirmation, then removes the entry.|
| Back to list            | Returns to the catalogue list.                |

### Adding an application

You will be asked for three things:

1. **Name** — a short identifier (must be unique).
2. **Description** — free-form text, shown in the list and in the
   launch card.
3. **Command** — accepts any of the following forms:
   - A bare **executable**: `firefox`, `nvim`
   - An **absolute** path: `/usr/bin/firefox`
   - A **relative** path: `./scripts/myscript.sh`
   - A **tilde** path: `~/bin/myscript.sh`
   - A **command with arguments**: `ls -lad`, `code /datos/pepe`,
     `git -C ~/repo commit -m "msg"`

   The first whitespace-delimited word is resolved to an absolute path
   (or matched against `$PATH`); everything after it is stored in a
   separate `args` field and forwarded verbatim to the executable at
   launch time.

`last_used` is updated automatically every time you launch the app
through the manager. The `created` date is set on add and is preserved
across edits.

### Editing an application

The edit form has four fields:

1. **Name**
2. **Description**
3. **Command** — pre-filled with the current executable. Type a new
   command (with or without arguments) to change both the executable
   and the arguments in one go.
4. **Arguments** — pre-filled with the current arguments. Useful when
   you only want to tweak flags without touching the executable.

Anything you type in the Command field after the executable word is
concatenated with whatever is in the Arguments field, in that order.

### Examples

```bash
# Open the TUI
mytuis

# List apps in a nicely formatted table (TTY) or tab-aligned text (pipe)
mytuis list
mytuis list | grep firefox

# Add an app non-interactively
mytuis add nvim "Modal text editor" nvim
mytuis add yt-dlp "Video downloader" /usr/local/bin/yt-dlp

# Add an app with arguments
mytuis add lsl "Listado largo" "ls -lad"
mytuis add code-pepe "Abrir code en /datos/pepe" "code /datos/pepe"

# Add an app interactively (equivalent to picking '[+]' in the TUI)
mytuis add

# Remove an app
mytuis remove nvim
```

### Running an application

The list is filterable: just type a few characters of the name or
description to narrow down the matches. The selected app is launched
via `exec`, so the manager process is completely replaced by the
application — no extra shell window.

## File format

The catalogue is stored at `~/.mytuis.yaml`:

```yaml
# mytuis — Application Manager
# Auto-generated file. Use mytuis.sh to manage your apps.
apps:
  - name: 'nvim'
    description: 'Hyperextensible Vim-based text editor'
    path: '/usr/bin/nvim'
    created: '2026-06-21 10:42:11'
    last_used: '2026-06-21 12:15:03'
  - name: 'lsl'
    description: 'Listado largo'
    path: '/usr/bin/ls'
    args: '-lad'
    created: '2026-06-21 10:45:00'
  - name: 'yt-dlp'
    description: 'Fork of youtube-dl with additional fixes'
    path: '/home/user/.local/bin/yt-dlp'
    created: '2026-06-21 10:45:00'
```

The `args` field is optional. When present it is a single string that
will be split on whitespace at launch time and forwarded to the
executable after `path`. When absent the entry is launched with no
arguments.

If the catalogue is empty the file contains:

```yaml
# mytuis — Application Manager
# This file stores your registered applications in YAML format.
# It is automatically generated and managed by the mytuis.sh script.
apps: []
```

Strings are wrapped in single quotes. The only escape inside those
strings is `''` (two consecutive single quotes), which represents a
literal single quote — so a description like `it's a test` is stored
as `'it''s a test'`.

## Script structure

| Function                        | Purpose                                                                |
|---------------------------------|------------------------------------------------------------------------|
| `check_dependencies`            | Verifies that `gum` is available.                                      |
| `init_apps_file`                | Creates `~/.mytuis.yaml` with `apps: []` on the first run.             |
| `get_current_date`              | Returns the current date as `YYYY-MM-DD HH:MM:SS`.                     |
| `resolve_path`                  | Resolves tilde / absolute / relative / `$PATH` inputs.                 |
| `truncate`                      | Truncates a string to a maximum length with an ellipsis.               |
| `read_apps`                     | Parses the YAML file with `awk` and emits one record per app.          |
| `write_apps`                    | Rewrites the YAML file from a stream of records piped via stdin.       |
| `show_header`                   | Draws the styled header banner.                                        |
| `format_apps_listing`           | Builds `name — description` rows for the filter / list.                |
| `format_main_listing`           | Wraps the apps listing with the `[+]` and `[x]` meta entries.          |
| `extract_name_from_selection`   | Extracts the app name from a `gum filter` selection.                   |
| `has_apps` / `app_exists`       | Convenience predicates over the catalogue.                             |
| `action_run_app`                | Updates `last_used`, shows a launch card and `exec`s the app.           |
| `action_add_new`                | Drives the three-step form to add a new application.                   |
| `action_edit_app`               | Edits a single application by name.                                    |
| `action_delete_app`             | Confirms and deletes a single application by name.                     |
| `action_submenu`                | Shows the per-app Run / Edit / Delete / Back menu.                     |
| `cmd_usage`                     | Prints the CLI usage information.                                      |
| `cmd_list`                      | Prints the catalogue as a table (TTY) or column-aligned text (pipe).   |
| `cmd_add`                       | Adds an app, non-interactively if three arguments are provided.        |
| `cmd_remove`                    | Removes an app by name, with confirmation when interactive.            |
| `main_tui`                      | The TUI loop: shows the main list, dispatches selections.              |
| `main`                          | Top-level entry point that dispatches to TUI or CLI sub-commands.      |

## Tips

- The list of apps is filterable: type to narrow down the matches.
- The selected app is launched via `exec`, so the manager process is
  completely replaced by the application.
- Pipe `mytuis list` into `grep`, `fzf`, `less`, etc. — when stdout is
  not a TTY the command falls back to a column-aligned text format.
- Use `mytuis add name desc path` from a shell script or a dotfiles
  installer to seed the catalogue automatically.
- The YAML file is plain text and safe to back up, sync with a
  dotfiles repository, or version-control.
- All file operations are performed atomically by rewriting the YAML
  file from scratch on every change, so there is no risk of leaving
  the file in a half-written state.

## License

MIT.
