#!/usr/bin/env bash
#
# ============================================================================
# mytuis.sh — Application Manager with TUI and CLI
# ----------------------------------------------------------------------------
# A bash script that uses 'gum' (https://github.com/charmbracelet/gum) to
# provide a friendly terminal interface for managing a personal catalogue
# of applications installed on the system.
#
# It exposes two surfaces:
#
#   * An interactive TUI (just run 'mytuis' with no arguments) that
#     starts in the catalogue list and lets the user run, edit, delete
#     or add applications.
#   * A small command-line interface for scripting and quick operations:
#         mytuis list                  list all registered apps
#         mytuis add [n d p]           add an app (interactive if no args)
#         mytuis remove <name>         remove an app by name
#         mytuis help                  show usage
#
# Storage
#   * All data lives in a single YAML file at ~/.mytuis.yaml. Each entry
#     carries: name, description, path, created and last_used.
#
# Dependencies
#   * gum         — https://github.com/charmbracelet/gum
#   * awk, sed, grep, date (standard on virtually any Unix system)
#
# Author      : generated for the user
# License     : MIT
# ============================================================================

# Strict mode: abort on error, abort on undefined variable, fail on pipe error.
set -euo pipefail

# ============================================================================
# CONFIGURATION CONSTANTS
# ============================================================================

# APPS_FILE: absolute path to the YAML file that stores all applications.
# It is placed in the user's home directory and uses the .yaml extension so
# the file is recognized by standard YAML tooling.
readonly APPS_FILE="${HOME}/.mytuis.yaml"

# DELIM: internal field separator used to pass data between bash and awk,
# and between the listing and the parsing helpers. The Unit Separator
# (0x1F) is used because it is a non-printable control character that is
# extremely unlikely to appear in user-entered data.
readonly DELIM=$'\x1f'

# DISPLAY_SEP: separator used when building the human-readable listing
# that is fed to 'gum filter'. The em-dash with surrounding spaces is
# used so the visual list reads as "name — description".
readonly DISPLAY_SEP=' — '

# META_ADD / META_EXIT: literal strings shown in the main listing as
# pseudo-entries. They are detected by exact match before the script
# tries to extract an application name, so the brackets guarantee no
# collision with real app names.
readonly META_ADD='[+] Add new application'
readonly META_EXIT='[x] Exit'

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

# check_dependencies
# ----------------------------------------------------------------------------
# Verifies that every external tool required by this script is available.
# Currently only 'gum' is required; the rest (awk, sed, grep, date) are
# part of any standard Unix system. If a dependency is missing the script
# prints a helpful error and exits with a non-zero status.
check_dependencies() {
    if ! command -v gum >/dev/null 2>&1; then
        # 'gum' is not found in PATH: print an error to stderr and exit.
        echo "Error: 'gum' is not installed or not in PATH." >&2
        echo "Install it from: https://github.com/charmbracelet/gum" >&2
        exit 1
    fi
}

# init_apps_file
# ----------------------------------------------------------------------------
# Creates the YAML storage file with an empty 'apps' list if it does not
# exist yet. This guarantees that subsequent read/write operations always
# find a well-formed file in place, even on the first run.
init_apps_file() {
    if [[ ! -f "$APPS_FILE" ]]; then
        # Write a minimal valid YAML file with an empty apps list.
        cat > "$APPS_FILE" <<'EOF'
# mytuis — Application Manager
# This file stores your registered applications in YAML format.
# It is automatically generated and managed by the mytuis.sh script.
apps: []
EOF
    fi
}

# get_current_date
# ----------------------------------------------------------------------------
# Returns the current date and time formatted as "YYYY-MM-DD HH:MM:SS".
# Output: the current timestamp on stdout.
get_current_date() {
    date "+%Y-%m-%d %H:%M:%S"
}

# resolve_path
# ----------------------------------------------------------------------------
# Resolves a user-supplied path or command name to an absolute, existing
# filesystem path.
#
# Resolution rules (in order):
#   1. Expand a leading "~" or "~/" to the value of $HOME.
#   2. If the input is an absolute path that exists, return it as-is.
#   3. If the input is a relative path (./foo or ../foo), resolve it from
#      the current working directory.
#   4. Otherwise treat it as a command name and look it up in $PATH.
#
# Arguments:
#   $1  -  the path or command name to resolve
# Output:
#   Prints the resolved absolute path on stdout, or nothing if it could
#   not be resolved to an existing file.
resolve_path() {
    local input="$1"

    # Rule 1: expand a leading tilde to the user's home directory.
    input="${input/#\~/$HOME}"

    # Rule 2: absolute path that exists on disk.
    if [[ "$input" = /* ]] && [[ -e "$input" ]]; then
        echo "$input"
        return
    fi

    # Rule 3: relative path that lives in the current directory.
    if [[ "$input" = ./* || "$input" = ../* ]]; then
        local dir resolved
        dir="$(dirname "$input")"
        resolved="$(cd "$dir" 2>/dev/null && pwd)/$(basename "$input")"
        if [[ -e "$resolved" ]]; then
            echo "$resolved"
        fi
        return
    fi

    # Rule 4: look the name up in $PATH.
    local found
    found="$(command -v "$input" 2>/dev/null || true)"
    if [[ -n "$found" && -e "$found" ]]; then
        echo "$found"
    fi
}

# truncate
# ----------------------------------------------------------------------------
# Truncates a string to the given maximum length, appending an ellipsis
# character (…) when the original string was longer than the limit. This
# is used to keep the entries in the filter list readable.
#
# Arguments:
#   $1  -  the string to truncate
#   $2  -  the maximum length (default: 60)
# Output: the (possibly truncated) string on stdout
truncate() {
    local str="$1"
    local max="${2:-60}"
    if [[ "${#str}" -gt "$max" ]]; then
        echo "${str:0:$((max - 1))}…"
    else
        echo "$str"
    fi
}

# ============================================================================
# YAML READ / WRITE
# ============================================================================
# The file uses single-quoted YAML strings. The reader (an awk one-liner)
# strips one leading and one trailing single quote and replaces every
# occurrence of two consecutive single quotes with a single quote. The
# writer (a pure-bash loop) does the symmetric escaping.
# ============================================================================

# read_apps
# ----------------------------------------------------------------------------
# Reads all applications from the YAML file and writes one line per app to
# stdout. The fields of each line are separated by DELIM and appear in the
# following order:
#     name<delim>description<delim>path<delim>created<delim>last_used
# If the file is empty or contains no apps, the function produces no
# output. This is the only place where the YAML file is parsed.
read_apps() {
    # The single-quote character is passed in via -v SQ to keep the awk
    # script free of awkward backslash-escaping.
    local SQ="'"
    awk -v DELIM="$DELIM" -v SQ="$SQ" '
    # extract_value: returns the value part of a YAML "key: value" line.
    # The first colon separates the key from the value; the value may
    # contain additional colons, which must be preserved.
    function extract_value(line,    pos, value) {
        pos = index(line, ":")
        if (pos == 0) return ""
        value = substr(line, pos + 1)
        sub(/^[[:space:]]+/, "", value)            # trim leading spaces
        # Strip one leading and one trailing single quote (if present).
        if (length(value) >= 2 \
            && substr(value, 1, 1) == SQ \
            && substr(value, length(value), 1) == SQ) {
            value = substr(value, 2, length(value) - 2)
        }
        # Unescape doubled single quotes (two SQ chars -> one SQ char).
        gsub(SQ SQ, SQ, value)
        return value
    }

    BEGIN { OFS = DELIM; name = "" }

    # Match the start of a new app entry (line "- name: ...").
    /^[[:space:]]*-[[:space:]]*name:[[:space:]]*/ {
        # Emit the previously buffered entry before starting a new one.
        if (name != "") print name, desc, path, created, last_used
        name      = extract_value($0)
        desc      = ""
        path      = ""
        created   = ""
        last_used = ""
        next
    }

    # Match the remaining fields of the current app entry.
    /^[[:space:]]+description:[[:space:]]*/ { desc      = extract_value($0); next }
    /^[[:space:]]+path:[[:space:]]*/        { path      = extract_value($0); next }
    /^[[:space:]]+created:[[:space:]]*/     { created   = extract_value($0); next }
    /^[[:space:]]+last_used:[[:space:]]*/   { last_used = extract_value($0); next }

    # At end-of-file, emit the last buffered entry (if any).
    END { if (name != "") print name, desc, path, created, last_used }
    ' "$APPS_FILE"
}

# write_apps
# ----------------------------------------------------------------------------
# Rewrites the YAML storage file using the data piped via stdin. Each input
# line must be a record in the format produced by read_apps:
#     name<delim>description<delim>path<delim>created<delim>last_used
# The file is fully replaced on every call, which keeps the implementation
# simple and avoids complex in-place editing.
write_apps() {
    # First, read all incoming records into parallel arrays. The arrays
    # are pre-declared (with `=()`) so they are always defined, even when
    # the caller pipes an empty stream. This matters because the script
    # runs with `set -u`, which would otherwise abort on the later
    # `${#names[@]}` check.
    local -a names=() descs=() paths=() createds=() last_useds=()
    local name desc path created last_used

    while IFS="$DELIM" read -r name desc path created last_used; do
        # Skip empty lines (defensive; should not happen in practice).
        [[ -z "$name" ]] && continue
        names+=("$name")
        descs+=("$desc")
        paths+=("$path")
        createds+=("$created")
        last_useds+=("$last_used")
    done

    # Now produce the YAML output.
    {
        echo "# mytuis — Application Manager"
        echo "# Auto-generated file. Use mytuis.sh to manage your apps."

        if [[ ${#names[@]} -eq 0 ]]; then
            # No applications registered yet: keep the list empty.
            echo "apps: []"
        else
            echo "apps:"
            local i esc_name esc_desc esc_path esc_created esc_last
            for i in "${!names[@]}"; do
                # In single-quoted YAML strings, a literal single quote is
                # represented by doubling it ('' -> ').
                esc_name="${names[$i]//\'/\'\'}"
                esc_desc="${descs[$i]//\'/\'\'}"
                esc_path="${paths[$i]//\'/\'\'}"
                esc_created="${createds[$i]//\'/\'\'}"
                echo "  - name: '${esc_name}'"
                echo "    description: '${esc_desc}'"
                echo "    path: '${esc_path}'"
                echo "    created: '${esc_created}'"
                # Only emit last_used if it has been set at least once.
                if [[ -n "${last_useds[$i]}" ]]; then
                    esc_last="${last_useds[$i]//\'/\'\'}"
                    echo "    last_used: '${esc_last}'"
                fi
            done
        fi
    } > "$APPS_FILE"
}

# ============================================================================
# UI HELPERS
# ============================================================================

# show_header
# ----------------------------------------------------------------------------
# Prints a styled header banner. Uses gum to draw a double-bordered block
# with the application name in pink and the subtitle in cyan.
show_header() {
    gum style \
        --border double \
        --border-foreground 212 \
        --padding "0 2" \
        --margin "0 0 1 0" \
        --foreground 255 \
        --align center \
        "$(gum style --foreground 212 --bold 'mytuis')  ::  $(gum style --foreground 39 'Application Manager')"
}

# format_apps_listing
# ----------------------------------------------------------------------------
# Builds the plain listing of apps (one per line) used by the TUI filter
# and by the CLI list command. Each entry is formatted as:
#     "name — description"
# so the user can identify every app at a glance.
#
# Output: one line per registered app on stdout
format_apps_listing() {
    while IFS="$DELIM" read -r name desc _path _created _last_used; do
        [[ -z "$name" ]] && continue
        local short_desc
        short_desc="$(truncate "$desc" 60)"
        echo "${name}${DISPLAY_SEP}${short_desc}"
    done < <(read_apps)
}

# format_main_listing
# ----------------------------------------------------------------------------
# Builds the full listing shown in the TUI main view. It consists of the
# meta entries ([+] Add new application and [x] Exit) framing the list
# of registered apps. The user filters through them just like normal
# entries, and the dispatcher in main_tui distinguishes meta entries
# from real apps by exact match on the prefix.
#
# Output: one line per entry on stdout (meta entries first and last,
#         apps in the middle).
format_main_listing() {
    echo "$META_ADD"
    format_apps_listing
    echo "$META_EXIT"
}

# extract_name_from_selection
# ----------------------------------------------------------------------------
# Given a line coming out of 'gum filter' (formatted as "name — desc"),
# returns just the name. Uses awk so the extraction is robust against
# descriptions that contain the DISPLAY_SEP string.
#
# Arguments:
#   $1  -  the raw selection coming from gum filter
# Output: the application name on stdout
extract_name_from_selection() {
    printf '%s' "$1" | awk -F"$DISPLAY_SEP" '{print $1}'
}

# has_apps
# ----------------------------------------------------------------------------
# Returns 0 (true) if at least one app is registered, 1 (false) otherwise.
has_apps() {
    [[ -n "$(read_apps)" ]]
}

# app_exists
# ----------------------------------------------------------------------------
# Returns 0 (true) if an app with the given name is registered.
#
# Arguments:
#   $1  -  the application name to look up
app_exists() {
    local name="$1"
    read_apps | awk -v FS="$DELIM" -v n="$name" \
        '$1 == n {found=1} END {exit !found}'
}

# ============================================================================
# ACTIONS  (operate on a single, already-selected application)
# ============================================================================
# These functions take the application name as their first argument and
# perform the corresponding CRUD operation. They are shared between the
# TUI (called after the user picks an app) and any future command that
# might want to manipulate apps programmatically.
# ============================================================================

# action_run_app
# ----------------------------------------------------------------------------
# Updates the 'last_used' date of the given app, shows a brief details
# card, and launches the application with 'exec', which replaces the
# current shell process.
#
# Arguments:
#   $1  -  the application name to run
action_run_app() {
    local app_name="$1"
    local now path_to_run description
    local -a records=()
    now="$(get_current_date)"

    # Walk the catalogue and rebuild it with the updated last_used for
    # the chosen app, while keeping every other entry untouched.
    while IFS="$DELIM" read -r name desc path created last_used; do
        if [[ "$name" == "$app_name" ]]; then
            description="$desc"
            path_to_run="$path"
            records+=("${name}${DELIM}${desc}${DELIM}${path}${DELIM}${created}${DELIM}${now}")
        else
            records+=("${name}${DELIM}${desc}${DELIM}${path}${DELIM}${created}${DELIM}${last_used}")
        fi
    done < <(read_apps)

    # Persist the updated list (with the new last_used).
    printf '%s\n' "${records[@]}" | write_apps

    # Show a brief details card before launching the app.
    gum style \
        --border rounded \
        --border-foreground 39 \
        --padding "0 1" \
        --margin "0 0 1 0" \
        -- "$(gum style --foreground 212 --bold "$app_name")" \
        "" \
        "$(gum style --foreground 240 'Description: ') ${description}" \
        "$(gum style --foreground 240 'Path:       ') ${path_to_run}" \
        "" \
        "$(gum style --foreground 82 --bold 'Launching...')"

    # Tiny delay so the user perceives the transition.
    sleep 0.4

    # Replace the current shell process with the selected application.
    # Quoting "$path_to_run" preserves paths that contain spaces.
    exec "$path_to_run"
}

# action_add_new
# ----------------------------------------------------------------------------
# Drives the three-step form to add a new application: name, description
# and path. Duplicate names are rejected. This function is shared by the
# TUI ("[+] Add new application") and by the interactive fallback of
# the 'mytuis add' command.
action_add_new() {
    local name desc path resolved now

    # --- 1. Name ----------------------------------------------------------
    name=$(gum input \
        --header "Add new application — Name" \
        --placeholder "Application name (e.g. nvim)" \
        --prompt "Name: ")
    [[ -z "$name" ]] && return

    # --- 2. Description ---------------------------------------------------
    desc=$(gum input \
        --header "Add new application — Description" \
        --placeholder "What does it do?" \
        --prompt "Description: ")

    # --- 3. Path ----------------------------------------------------------
    path=$(gum input \
        --header "Add new application — Path" \
        --placeholder "Absolute path, relative path, or command name in \$PATH" \
        --prompt "Path: ")
    [[ -z "$path" ]] && return

    # --- 4. Resolve the path ---------------------------------------------
    resolved="$(resolve_path "$path")"
    if [[ -z "$resolved" ]]; then
        gum style --foreground 196 --margin "1 0" \
            "✖ Error: could not resolve path: $path"
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi

    # --- 5. Reject duplicate names ---------------------------------------
    if app_exists "$name"; then
        gum style --foreground 196 --margin "1 0" \
            "✖ Error: an application named '$name' already exists."
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi

    # --- 6. Persist the new entry ----------------------------------------
    now="$(get_current_date)"
    {
        read_apps
        printf '%s\n' "${name}${DELIM}${desc}${DELIM}${resolved}${DELIM}${now}${DELIM}"
    } | write_apps

    # --- 7. Confirm -------------------------------------------------------
    gum style --foreground 82 --margin "1 0" \
        "✔ Application '$name' added successfully."
    sleep 1
}

# action_edit_app
# ----------------------------------------------------------------------------
# Drives the edit form for a single, already-selected application. The
# name, description and path can all be changed; the creation date is
# preserved and the last_used timestamp is kept untouched.
#
# Arguments:
#   $1  -  the application name to edit
action_edit_app() {
    local app_name="$1"

    # --- 1. Read the current values for that entry -----------------------
    local old_name="" old_desc="" old_path="" old_created="" old_last_used=""
    while IFS="$DELIM" read -r name desc path created last_used; do
        if [[ "$name" == "$app_name" ]]; then
            old_name="$name"
            old_desc="$desc"
            old_path="$path"
            old_created="$created"
            old_last_used="$last_used"
            break
        fi
    done < <(read_apps)

    if [[ -z "$old_name" ]]; then
        gum style --foreground 196 --margin "1 0" \
            "✖ Error: could not find data for '$app_name'."
        return
    fi

    # --- 2. Prompt for new values, pre-filled with the current ones ------
    local new_name new_desc new_path resolved
    new_name=$(gum input \
        --header "Edit '$old_name' — Name" \
        --placeholder "Application name" \
        --prompt "Name: " \
        --value "$old_name")
    [[ -z "$new_name" ]] && return

    new_desc=$(gum input \
        --header "Edit '$old_name' — Description" \
        --placeholder "What does it do?" \
        --prompt "Description: " \
        --value "$old_desc")

    new_path=$(gum input \
        --header "Edit '$old_name' — Path" \
        --placeholder "Absolute path, relative path, or command name in \$PATH" \
        --prompt "Path: " \
        --value "$old_path")
    [[ -z "$new_path" ]] && return

    # --- 3. Resolve the new path -----------------------------------------
    resolved="$(resolve_path "$new_path")"
    if [[ -z "$resolved" ]]; then
        gum style --foreground 196 --margin "1 0" \
            "✖ Error: could not resolve path: $new_path"
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi

    # --- 4. If the name changed, ensure the new name is not taken --------
    if [[ "$new_name" != "$old_name" ]]; then
        if app_exists "$new_name"; then
            gum style --foreground 196 --margin "1 0" \
                "✖ Error: an application named '$new_name' already exists."
            gum input --placeholder "Press Enter to continue..." >/dev/null
            return
        fi
    fi

    # --- 5. Persist the changes ------------------------------------------
    while IFS="$DELIM" read -r name desc path created last_used; do
        if [[ "$name" == "$old_name" ]]; then
            printf '%s\n' "${new_name}${DELIM}${new_desc}${DELIM}${resolved}${DELIM}${created}${DELIM}${last_used}"
        else
            printf '%s\n' "${name}${DELIM}${desc}${DELIM}${path}${DELIM}${created}${DELIM}${last_used}"
        fi
    done < <(read_apps) | write_apps

    gum style --foreground 82 --margin "1 0" \
        "✔ Application updated."
    sleep 1
}

# action_delete_app
# ----------------------------------------------------------------------------
# Asks for confirmation and then removes the given application from the
# YAML storage file.
#
# Arguments:
#   $1  -  the application name to delete
action_delete_app() {
    local app_name="$1"

    # Ask for confirmation. gum confirm returns 0 on Yes, 1 on No,
    # and a non-zero exit on Ctrl+C (caught by `set -e`).
    if ! gum confirm \
        --prompt.bold \
        --prompt.foreground 196 \
        "Delete '$app_name'? This cannot be undone."; then
        return
    fi

    # Re-emit every record except the one being deleted.
    while IFS="$DELIM" read -r name desc path created last_used; do
        [[ "$name" == "$app_name" ]] && continue
        printf '%s\n' "${name}${DELIM}${desc}${DELIM}${path}${DELIM}${created}${DELIM}${last_used}"
    done < <(read_apps) | write_apps

    gum style --foreground 82 --margin "1 0" \
        "✔ Application '$app_name' deleted."
    sleep 1
}

# action_submenu
# ----------------------------------------------------------------------------
# Shows the per-application action menu after the user has picked an app
# from the main list. The four options dispatch to the CRUD actions or
# fall back to the main listing.
#
# Arguments:
#   $1  -  the application name to operate on
action_submenu() {
    local app_name="$1"
    local choice

    choice=$(gum choose \
        --header "What do you want to do with '$app_name'?" \
        --height 8 \
        --cursor "▶ " \
        --item.foreground 255 \
        --selected.foreground 212 \
        "Run this application" \
        "Edit this application" \
        "Delete this application" \
        "Back to list")

    case "$choice" in
        "Run this application")     action_run_app   "$app_name" ;;
        "Edit this application")    action_edit_app  "$app_name" ;;
        "Delete this application")  action_delete_app "$app_name" ;;
        "Back to list"|"")          return           ;;
        *)                          return           ;;
    esac
}

# ============================================================================
# COMMAND-LINE INTERFACE
# ============================================================================
# These functions implement the 'mytuis <subcommand>' surface. They are
# designed to work both interactively (gum styling when available and
# stdout is a TTY) and in non-interactive scripts (plain text output).
# ============================================================================

# cmd_usage
# ----------------------------------------------------------------------------
# Prints the command-line usage information. Always uses plain text so
# the help is readable when piped or redirected.
cmd_usage() {
    cat <<EOF
mytuis — Application Manager

Usage:
  mytuis                       Open the interactive TUI.
  mytuis list                  List all registered applications.
  mytuis add [name desc path]  Add an application. Interactive if no
                               arguments are provided.
  mytuis remove <name>         Remove an application by name.
  mytuis help                  Show this help message.

Data is stored in: ${APPS_FILE}

Examples:
  mytuis
  mytuis list
  mytuis add nvim "Modal text editor" nvim
  mytuis add yt-dlp "Video downloader" /usr/local/bin/yt-dlp
  mytuis remove nvim
EOF
}

# cmd_list
# ----------------------------------------------------------------------------
# Prints the catalogue as a table. When stdout is a TTY and gum is
# available the table is rendered with colours and borders; otherwise a
# plain tab-separated layout is produced so the output is easy to grep
# or pipe into other tools.
cmd_list() {
    init_apps_file
    local listing
    listing="$(read_apps)"

    if [[ -z "$listing" ]]; then
        if [[ -t 1 ]] && command -v gum >/dev/null 2>&1; then
            gum style --foreground 214 --margin "1 0" \
                "No applications registered yet."
        else
            echo "No applications registered yet."
        fi
        return
    fi

    # Convert DELIM-separated records into tab-separated rows for table
    # output. The description is truncated to keep the table compact.
    local rows
    rows="$(printf '%s\n' "$listing" | \
        awk -F"$DELIM" -v OFS='\t' -v TRUNC=60 '
            {
                desc = $2
                if (length(desc) > TRUNC) desc = substr(desc, 1, TRUNC - 1) "…"
                print $1, desc, $3, $4, $5
            }')"

    if [[ -t 1 ]] && command -v gum >/dev/null 2>&1; then
        # Pretty rendering when running in a terminal. 'gum table' does
        # not expose a foreground color for the border, so we just pick
        # a clean rounded style and let the default colour apply.
        printf '%s\n' "$rows" | \
            gum table \
                --separator $'\t' \
                --columns "Name,Description,Path,Created,Last used" \
                --border rounded \
                --print
    else
        # Plain rendering suitable for pipes and scripts.
        printf '%s\n' "$rows" | \
            column -t -s $'\t' 2>/dev/null || printf '%s\n' "$rows"
    fi
}

# cmd_add
# ----------------------------------------------------------------------------
# Handles the 'mytuis add' command.
#
# Behaviour:
#   * 0 arguments: open the interactive add form (action_add_new).
#   * 3 arguments: add the application non-interactively using the
#     provided name, description and path.
#   * Any other count: print usage and exit non-zero.
cmd_add() {
    init_apps_file
    local name desc path resolved now

    if [[ $# -eq 0 ]]; then
        # Interactive mode: fall back to the TUI form.
        action_add_new
        return
    fi

    if [[ $# -ne 3 ]]; then
        echo "Usage: mytuis add <name> <description> <path>" >&2
        exit 1
    fi

    name="$1"
    desc="$2"
    path="$3"

    # --- Validate the path ----------------------------------------------
    resolved="$(resolve_path "$path")"
    if [[ -z "$resolved" ]]; then
        echo "Error: could not resolve path: $path" >&2
        exit 1
    fi

    # --- Reject duplicate names -----------------------------------------
    if app_exists "$name"; then
        echo "Error: an application named '$name' already exists." >&2
        exit 1
    fi

    # --- Persist --------------------------------------------------------
    now="$(get_current_date)"
    {
        read_apps
        printf '%s\n' "${name}${DELIM}${desc}${DELIM}${resolved}${DELIM}${now}${DELIM}"
    } | write_apps

    echo "✔ Added '$name' -> $resolved"
}

# cmd_remove
# ----------------------------------------------------------------------------
# Handles the 'mytuis remove <name>' command.
#
# Behaviour:
#   * With a name argument: look up the application, optionally ask for
#     confirmation when running in a TTY, and remove it.
#   * Without arguments: print usage and exit non-zero.
cmd_remove() {
    init_apps_file
    local name="${1:-}"

    if [[ -z "$name" ]]; then
        echo "Usage: mytuis remove <name>" >&2
        exit 1
    fi

    if ! app_exists "$name"; then
        echo "Error: no application named '$name'." >&2
        exit 1
    fi

    # Ask for confirmation only when stdin is attached to a terminal and
    # gum is available. Scripts that pipe 'yes' or just want to skip
    # confirmation can use 'mytuis remove <name> < /dev/null'.
    if [[ -t 0 ]] && command -v gum >/dev/null 2>&1; then
        if ! gum confirm "Remove '$name'?"; then
            echo "Cancelled."
            return
        fi
    fi

    while IFS="$DELIM" read -r n d p c lu; do
        [[ "$n" == "$name" ]] && continue
        printf '%s\n' "${n}${DELIM}${d}${DELIM}${p}${DELIM}${c}${DELIM}${lu}"
    done < <(read_apps) | write_apps

    echo "✔ Removed '$name'"
}

# ============================================================================
# INTERACTIVE TUI
# ============================================================================

# main_tui
# ----------------------------------------------------------------------------
# Entry point of the interactive interface. The first thing the user sees
# is the catalogue listing framed by two meta entries. Selecting a meta
# entry triggers a global action; selecting an app opens the per-app
# sub-menu. The loop terminates when the user picks the [x] Exit entry
# or sends Ctrl+C.
main_tui() {
    check_dependencies
    init_apps_file

    while true; do
        # Clear the screen and show the styled header on every iteration
        # so the user always sees a fresh view of the catalogue.
        clear
        show_header

        local listing selection
        listing="$(format_main_listing)"

        if [[ -z "$(read_apps)" ]]; then
            # No apps registered: show the empty state below the header.
            gum style --foreground 214 --margin "1 0" \
                "No applications registered yet."
            gum style --foreground 240 \
                "Pick '[+] Add new application' below to get started."
        fi

        # Show the filterable list. The user can type to narrow down the
        # matches; both apps and meta entries are filterable.
        selection="$(printf '%s\n' "$listing" | gum filter \
            --header "Pick an app, add a new one, or exit (type to filter):" \
            --height 18 \
            --prompt "▶ " \
            --placeholder "Search..." \
            --indicator "▶" \
            --match.foreground 212 \
            --text.foreground 255 \
            --cursor-text.foreground 212)"

        # An empty selection means the user cancelled (Esc / Ctrl+C).
        [[ -z "$selection" ]] && continue

        # Dispatch based on whether the selection is a meta entry or a
        # real application.
        case "$selection" in
            "$META_ADD")
                action_add_new
                ;;
            "$META_EXIT")
                exit 0
                ;;
            *)
                local app_name
                app_name="$(extract_name_from_selection "$selection")"
                if [[ -n "$app_name" ]]; then
                    action_submenu "$app_name"
                fi
                ;;
        esac
    done
}

# ============================================================================
# ENTRY POINT
# ============================================================================

# main
# ----------------------------------------------------------------------------
# Top-level dispatcher. Parses the command-line arguments and routes to
# either the interactive TUI or one of the CLI sub-commands. When the
# script is sourced (rather than executed directly) the dispatcher is
# skipped so the helper functions can be tested in isolation.
main() {
    # Sub-command dispatch. Anything that is not a recognised command is
    # treated as an unknown argument and produces a usage message.
    case "${1:-}" in
        "")             main_tui ;;
        list|ls)        cmd_list ;;
        add)            shift; cmd_add "$@" ;;
        remove|rm|del)  shift; cmd_remove "${1:-}" ;;
        help|-h|--help) cmd_usage ;;
        *)
            echo "Unknown command: $1" >&2
            cmd_usage >&2
            exit 1
            ;;
    esac
}

# Only run main if the script is executed directly, not when sourced.
# This makes the script easier to test in isolation and to embed in
# larger wrappers if needed in the future.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
