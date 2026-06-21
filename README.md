# mytuis — Application Manager

A small, attractive terminal UI for managing a personal catalogue of
applications. Built with [gum](https://github.com/charmbracelet/gum) and
plain bash, with persistent storage in a human-readable YAML file.

```
╔═════════════════════════════════════╗
║  mytuis  ::  Application Manager    ║
╚═════════════════════════════════════╝

What do you want to do?
▶ Run an application
  Add a new application
  Edit an application
  Delete an application
  Exit
```

## Features

- **CRUD operations** — create, read, update and delete application
  entries from a single menu.
- **Quick launch** — pick an app from the filterable list and it is
  launched immediately, replacing the manager process via `exec`.
- **Smart path handling** — accepts absolute paths (`/usr/bin/firefox`),
  relative paths (`./scripts/myscript.sh`), tilde paths (`~/bin/foo`) or
  plain command names looked up in `$PATH` (`firefox`).
- **Persistent metadata** — every entry stores its name, description,
  absolute path, creation date and last-used date.
- **YAML storage** — the catalogue lives in `~/.mytuis.yaml` and can be
  inspected, edited or backed up with any text editor.
- **Filterable list** — quickly find an app by typing into the filter
  prompt; the description is visible in every row of the list.
- **Friendly TUI** — clear menus, color-coded messages and clean
  borders, all powered by `gum`.

## Requirements

- **bash ≥ 4**
- **[gum](https://github.com/charmbracelet/gum)** — install with
  `brew install gum` (macOS), `pacman -S gum` (Arch),
  `apt install gum` (some Debian-based distros) or see the
  [official installation guide](https://github.com/charmbracelet/gum#installation).
- Standard Unix utilities: `awk`, `sed`, `grep`, `date`, `tput`.

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

```bash
mytuis
```

From the main menu you can:

| Option                  | What it does                                                |
|-------------------------|-------------------------------------------------------------|
| Run an application      | Filterable, scrollable list — picking one launches it.      |
| Add a new application   | Prompts for name, description and path.                     |
| Edit an application     | Lets you change any field of an existing entry.             |
| Delete an application   | Removes an entry after confirmation.                        |
| Exit                    | Quit the manager.                                           |

### Adding an application

You will be asked for three things:

1. **Name** — a short identifier (must be unique).
2. **Description** — free-form text, shown in the list and in the
   launch card.
3. **Path** — any of the following forms is accepted:
   - An **absolute** path: `/usr/bin/firefox`
   - A **relative** path: `./scripts/myscript.sh`
   - A **tilde** path: `~/bin/myscript.sh`
   - A bare **command name** available in `$PATH`: `firefox`

`last_used` is updated automatically every time you launch the app
through the manager. The `created` date is set on add and is preserved
across edits.

### Running an application

The list is filterable: just type a few characters of the name or
description to narrow down the matches. Use ↑/↓ to move, Enter to
select. The selected app is launched via `exec`, so the manager
process is completely replaced by the application — no extra shell
window.

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
  - name: 'yt-dlp'
    description: 'Fork of youtube-dl with additional fixes'
    path: '/home/user/.local/bin/yt-dlp'
    created: '2026-06-21 10:45:00'
```

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

| Function               | Purpose                                                              |
|------------------------|----------------------------------------------------------------------|
| `check_dependencies`   | Verifies that `gum` is available.                                    |
| `init_apps_file`       | Creates `~/.mytuis.yaml` with `apps: []` on the first run.           |
| `get_current_date`     | Returns the current date as `YYYY-MM-DD HH:MM:SS`.                   |
| `resolve_path`         | Resolves tilde / absolute / relative / `$PATH` inputs.               |
| `yaml_escape_sq`       | Escapes single quotes for use in single-quoted YAML scalars.         |
| `truncate`             | Truncates a string to a maximum length with an ellipsis.             |
| `read_apps`            | Parses the YAML file with `awk` and emits one record per app.        |
| `write_apps`           | Rewrites the YAML file from a stream of records piped via stdin.     |
| `show_header`          | Draws the styled header banner.                                      |
| `format_listing`       | Builds the human-readable `name — description` list for the filter.  |
| `extract_name_from_selection` | Extracts the app name from a `gum filter` selection.          |
| `action_run`           | Run: pick an app, update `last_used` and `exec` it.                   |
| `action_add`           | Create: prompt for name / description / path and persist.            |
| `action_edit`          | Update: change the fields of an existing entry.                      |
| `action_delete`        | Delete: confirm and remove an entry.                                 |
| `main`                 | Top-level menu loop that dispatches to the action functions.         |

## Tips

- The list of apps is filterable: type to narrow down the matches.
- The selected app is launched via `exec`, so the manager process is
  completely replaced by the application.
- The YAML file is plain text and safe to back up, sync with a
  dotfiles repository, or version-control.
- All file operations are performed atomically by rewriting the YAML
  file from scratch on every change, so there is no risk of leaving
  the file in a half-written state.

## License

MIT.
