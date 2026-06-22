#!/usr/bin/env bash
#
# ============================================================================
# mytuis.sh — Application Manager with TUI
# ----------------------------------------------------------------------------
# A bash script that uses 'gum' (https://github.com/charmbracelet/gum) to
# provide a friendly terminal interface for managing a personal catalogue
# of applications installed on the system.
#
# Features
#   * CRUD operations for applications: Create, Read, Update and Delete.
#   * Persistent storage in YAML format at ~/.mytuis.yaml.
#   * For every app the following metadata is kept:
#       - name            (human readable identifier, must be unique)
#       - description     (short free-form text)
#       - path            (absolute path to the executable)
#       - created         (date the entry was added)
#       - last_used       (date the app was last launched from the script)
#   * Quick launch: picking an app from the menu launches it via 'exec',
#     so the manager process is replaced by the application itself.
#
# Dependencies
#   * gum         — https://github.com/charmbracelet/gum
#   * awk, grep, sed, date (standard on virtually any Unix system)
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

# DISPLAY_SEP: separator used when building the human-readable listing that
# is fed to 'gum filter'. The em-dash with surrounding spaces is used so
# the visual list looks like "name — description".
readonly DISPLAY_SEP=' — '

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

# yaml_escape_sq
# ----------------------------------------------------------------------------
# Escapes a string for use inside a single-quoted YAML scalar. In YAML
# single-quoted strings, the only escape sequence is '' (two consecutive
# single quotes), which represents a literal single quote character.
#
# Arguments:
#   $1  -  the string to escape
# Output:
#   The escaped string on stdout.
yaml_escape_sq() {
    local str="$1"
    # Replace each single quote with two single quotes ('' -> ').
    str="${str//\'/\'\'}"
    echo "$str"
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

# format_listing
# ----------------------------------------------------------------------------
# Builds the list of strings used by 'gum filter' to present the apps.
# Each entry is formatted as: "name — description" so the user can
# identify every app at a glance. The description is truncated to keep
# the listing readable.
#
# Output: one line per registered app on stdout
format_listing() {
    while IFS="$DELIM" read -r name desc _path _created _last_used; do
        [[ -z "$name" ]] && continue
        local short_desc
        short_desc="$(truncate "$desc" 60)"
        echo "${name}${DISPLAY_SEP}${short_desc}"
    done < <(read_apps)
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
    printf '%s' "$1" | awk -v sep="$DISPLAY_SEP" -F"$DISPLAY_SEP" '{print $1}'
}

# ============================================================================
# CRUD OPERATIONS
# ============================================================================

# action_run
# ----------------------------------------------------------------------------
# Displays a filterable, scrollable list of all registered apps. When the
# user picks one, the script updates the 'last_used' date, shows the app
# details for a brief moment, and finally launches the application with
# 'exec', which replaces the current shell process.
action_run() {
    # Defensive: ensure the storage file exists before reading it.
    init_apps_file

    # Build the list of selectable options. We capture the listing in a
    # variable so we can check whether there is at least one entry.
    local listing
    listing="$(format_listing)"

    if [[ -z "$listing" ]]; then
        # No apps registered: show a friendly message and return.
        gum style --foreground 214 --margin "1 0" \
        gum style --foreground 240 "Use 'Add a new application' to get started."
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi

    # Show the filter prompt. The user can type to narrow down the list.
    local selection
    selection="$(printf '%s\n' "$listing" | gum filter \
        --header "Select an application to run (type to filter):" \
        --height 18 \
        --prompt "▶ " \
        --placeholder "Search by name or description..." \
        --indicator "▶" \
        --match.foreground 212 \
        --text.foreground 255 \
        --cursor-text.foreground 212)"

    # An empty selection means the user cancelled (Esc or Ctrl+C).
    [[ -z "$selection" ]] && return

    # Extract the application name from the formatted line.
    local selected_name
    selected_name="$(extract_name_from_selection "$selection")"
    [[ -z "$selected_name" ]] && return

    # Look up the full record for the selected app and update last_used.
    local now path_to_run description
    local -a records=()
    now="$(get_current_date)"

    while IFS="$DELIM" read -r name desc path created last_used; do
        if [[ "$name" == "$selected_name" ]]; then
            description="$desc"
            path_to_run="$path"
            # Refresh the last_used timestamp for this entry.
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
        -- "$(gum style --foreground 212 --bold "$selected_name")" \
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

# action_add
# ----------------------------------------------------------------------------
# Prompts the user for the name, description and path of a new application
# and appends it to the YAML storage file. Duplicate names are rejected.
action_add() {
    local name desc path resolved now

    # Defensive: ensure the storage file exists before reading it.
    init_apps_file

    # --- 1. Name ----------------------------------------------------------
    # The name cannot be empty; it is the unique identifier of the entry.
    name=$(gum input \
        --header "Add new application — Name" \
        --placeholder "Application name (e.g. nvim)" \
        --prompt "Name: ")
    [[ -z "$name" ]] && return

    # --- 2. Description ---------------------------------------------------
    # The description is optional; the user may leave it empty.
    desc=$(gum input \
        --header "Add new application — Description" \
        --placeholder "What does it do?" \
        --prompt "Description: ")

    # --- 3. Path ----------------------------------------------------------
    # The path may be absolute, relative or a bare command name.
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
    if read_apps | awk -v FS="$DELIM" -v n="$name" \
            '$1 == n {found=1} END {exit !found}'; then
        gum style --foreground 196 --margin "1 0" \
            "✖ Error: an application named '$name' already exists."
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi

    # --- 6. Persist the new entry ----------------------------------------
    # We read all existing apps, append the new record and rewrite the
    # YAML file in one shot. last_used is left empty until the user
    # actually launches the application for the first time.
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

# action_edit
# ----------------------------------------------------------------------------
# Lets the user pick an existing application and modify any of its
# fields. The name, description and path can all be changed; the creation
# date is preserved and the last_used timestamp is kept untouched.
action_edit() {
    # Defensive: ensure the storage file exists before reading it.
    init_apps_file

    local listing
    listing="$(format_listing)"

    if [[ -z "$listing" ]]; then
        gum style --foreground 214 --margin "1 0" \
            "No applications to edit."
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi
    local selection
    selection="$(printf '%s\n' "$listing" | gum filter \
        --header "Select an application to edit:" \
        --height 15 \
        --placeholder "Search..." \
        --match.foreground 212 \
        --text.foreground 255 \
        --cursor-text.foreground 212)"
    [[ -z "$selection" ]] && return

    local selected_name
    selected_name="$(extract_name_from_selection "$selection")"
    [[ -z "$selected_name" ]] && return

    # --- 2. Read the current values for that entry -----------------------
    local old_name="" old_desc="" old_path="" old_created="" old_last_used=""
    while IFS="$DELIM" read -r name desc path created last_used; do
        if [[ "$name" == "$selected_name" ]]; then
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
            "✖ Error: could not find data for '$selected_name'."
        return
    fi

    # --- 3. Prompt for new values, pre-filled with the current ones ------
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

    # --- 4. Resolve the new path -----------------------------------------
    resolved="$(resolve_path "$new_path")"
    if [[ -z "$resolved" ]]; then
        gum style --foreground 196 --margin "1 0" \
            "✖ Error: could not resolve path: $new_path"
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi

    # --- 5. If the name changed, ensure the new name is not taken --------
    if [[ "$new_name" != "$old_name" ]]; then
        if read_apps | awk -v FS="$DELIM" -v n="$new_name" \
                '$1 == n {found=1} END {exit !found}'; then
            gum style --foreground 196 --margin "1 0" \
                "✖ Error: an application named '$new_name' already exists."
            gum input --placeholder "Press Enter to continue..." >/dev/null
            return
        fi
    fi

    # --- 6. Persist the changes ------------------------------------------
    # We rebuild the file from scratch, replacing the edited entry and
    # leaving the rest untouched. created is preserved and last_used is
    # not changed by an edit operation.
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

# action_delete
# ----------------------------------------------------------------------------
# Asks the user to pick an application, requests confirmation, and then
# removes the entry from the YAML storage file.
action_delete() {
    # Defensive: ensure the storage file exists before reading it.
    init_apps_file

    local listing
    listing="$(format_listing)"

    if [[ -z "$listing" ]]; then
        gum style --foreground 214 --margin "1 0" \
            "No applications to delete."
        gum input --placeholder "Press Enter to continue..." >/dev/null
        return
    fi

    # --- 1. Pick the app --------------------------------------------------
    local selection
    selection="$(printf '%s\n' "$listing" | gum filter \
        --header "Select an application to delete:" \
        --height 15 \
        --placeholder "Search..." \
        --match.foreground 212 \
        --text.foreground 255 \
        --cursor-text.foreground 212)"
    [[ -z "$selection" ]] && return

    local selected_name
    selected_name="$(extract_name_from_selection "$selection")"
    [[ -z "$selected_name" ]] && return

    # --- 2. Confirm -------------------------------------------------------
    # gum confirm returns 0 on Yes, 1 on No and 130 on Ctrl+C.
    if ! gum confirm \
        --prompt.bold \
        --prompt.foreground 196 \
        "Delete '$selected_name'? This cannot be undone."; then
        return
    fi

    # --- 3. Persist -------------------------------------------------------
    # Re-emit every record except the one being deleted.
    while IFS="$DELIM" read -r name desc path created last_used; do
        [[ "$name" == "$selected_name" ]] && continue
        printf '%s\n' "${name}${DELIM}${desc}${DELIM}${path}${DELIM}${created}${DELIM}${last_used}"
    done < <(read_apps) | write_apps

    gum style --foreground 82 --margin "1 0" \
        "✔ Application '$selected_name' deleted."
    sleep 1
}

# ============================================================================
# MAIN LOOP
# ============================================================================

# main
# ----------------------------------------------------------------------------
# Entry point of the script. Performs the initial setup (dependency
# check, file initialization) and then enters a loop that shows the
# main menu and dispatches the chosen CRUD action. The loop terminates
# when the user picks "Exit" or aborts with Ctrl+C.
main() {
    check_dependencies
    init_apps_file

    while true; do
        # Clear the screen and show a styled header on every iteration
        # so the user always sees a fresh view of the menu.
        clear
        show_header

        # Show the top-level menu and capture the chosen action.
        local choice
        choice=$(gum choose \
            --header "What do you want to do?" \
            --height 9 \
            --cursor "▶ " \
            --item.foreground 255 \
            --selected.foreground 212 \
            "Run an application" \
            "Add a new application" \
            "Edit an application" \
            "Delete an application" \
            "Exit")

        case "$choice" in
            "Run an application")     action_run   ;;
            "Add a new application")  action_add   ;;
            "Edit an application")    action_edit  ;;
            "Delete an application")  action_delete ;;
            "Exit")                   exit 0       ;;
            *)                        ;;           # ignore empty selection
        esac
    done
}

# Only run main if the script is executed directly, not when sourced.
# This makes the script easier to test in isolation and to embed in
# larger wrappers if needed in the future.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
