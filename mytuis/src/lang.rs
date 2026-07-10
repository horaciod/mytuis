//! # Internacionalización (i18n)
//!
//! Este módulo define el `Lang` (idioma) activo y todas las funciones
//! que producen **strings user-facing** del programa. Cualquier string
//! que el usuario final vea en pantalla, en la CLI o en la TUI, debe
//! venir de acá.
//!
//! ## Por qué un módulo custom (sin `rust-i18n` o similar)
//!
//! - Son ~60 strings. Un crate de i18n agregaría complejidad (build
//!   script, catálogos externos, macros) por poca ganancia.
//! - Mantener todo en un archivo Rust hace que un `grep` por el texto
//!   encuentre la traducción en el mismo lugar.
//! - Agregar un idioma nuevo es agregar variantes a un `match`.
//!
//! ## Cómo se elige el idioma
//!
//! `Lang::detect()` mira, en orden:
//!
//! 1. `$MYTUIS_LANG` — override explícito del usuario (e.g. `es`, `en`).
//! 2. `$LC_ALL` — variable estándar POSIX de mayor prioridad.
//! 3. `$LC_MESSAGES` — específica para mensajes.
//! 4. `$LANG` — la más común.
//! 5. Default: `English`.
//!
//! Los valores se parsean con `from_env_value`, que acepta formas como
//! `en`, `en_US`, `en_US.UTF-8`, `es_AR`, `es_ES@euro`, etc. — solo
//! nos importa el prefijo de dos letras.
//!
//! ## Convención para agregar un idioma
//!
//! 1. Agregar variante al enum `Lang` (e.g. `Fr`).
//! 2. Agregar brazo en todos los `match self` de cada función de
//!    traducción. El compilador te avisa si te olvidaste de alguna.
//! 3. Agregar reconocimiento en `from_env_value`.
//! 4. Documentar en AGENTS.md.
//!
//! ## Lo que NO se traduce
//!
//! - Comentarios del código fuente (son para devs).
//! - `clap --help` (clap no soporta i18n nativo y mockearlo tiene baja
//!   relación costo/beneficio).
//! - Nombres de campos YAML (`name`, `description`, `path`).
//! - Nombres de subcomandos (`apps`, `paths`, `list`, etc.).

use std::path::Path;

/// Idiomas soportados por `mytuis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

impl Lang {
    /// Nombre legible del idioma, para mostrar en diagnósticos.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }

    /// Detecta el idioma del entorno con la jerarquía descripta en el
    /// header del módulo.
    pub fn detect() -> Lang {
        // 1. Override explícito del usuario.
        if let Ok(v) = std::env::var("MYTUIS_LANG") {
            if let Some(l) = from_env_value(&v) {
                return l;
            }
        }
        // 2. Variables POSIX estándar (LC_ALL tiene prioridad sobre
        // LC_MESSAGES, que a su vez tiene prioridad sobre LANG).
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(var) {
                if let Some(l) = from_env_value(&v) {
                    return l;
                }
            }
        }
        // 3. Default.
        Lang::En
    }
}

/// Parsea un valor de variable de entorno estilo POSIX
/// (`en_US.UTF-8`, `es_AR`, `fr`, etc.) a un `Lang`.
///
/// Devuelve `None` si el prefijo no corresponde a ningún idioma
/// soportado. La función es robusta a variantes y modificadores
/// (`@euro`, `.UTF-8`, etc.).
fn from_env_value(s: &str) -> Option<Lang> {
    // Cortamos por `.` (encoding), `@` (modifier) y `_` (country).
    let mut s = s.split('.').next().unwrap_or(s);
    s = s.split('@').next().unwrap_or(s);
    s = s.split('_').next().unwrap_or(s);
    match s.to_lowercase().as_str() {
        "en" | "eng" | "english" => Some(Lang::En),
        "es" | "spa" | "spanish" | "español" | "castellano" => Some(Lang::Es),
        _ => None,
    }
}

// ============================================================================
//  Funciones de traducción
// ============================================================================
//
// Todas las funciones reciben `self` (idioma) y devuelven `String` o
// `&'static str`. Si un mensaje no tiene interpolación, devuelve
// `&'static str` para evitar allocations innecesarias.
//
// Convención de nombres:
//   - `err_*`  → mensajes de error que se imprimen al usuario.
//   - `msg_*`  → mensajes de éxito / info / notificaciones.
//   - `tab_*`, `header_*`, `footer_*`, `field_*` → textos de UI fijos.
//   - `form_*` → títulos / hints de formularios.
//
// Si en el futuro agregás un idioma, el compilador va a quejarse en
// TODOS los `match self` que te hayas olvidado. Aprovéchalo.

impl Lang {
    // -----------------------------------------------------------------
    //  Errores (los que viven en `error.rs`)
    // -----------------------------------------------------------------

    pub fn err_not_found(self, name: &str) -> String {
        match self {
            Lang::En => format!("no entry named '{name}' found"),
            Lang::Es => format!("no se encontró una entrada llamada '{name}'"),
        }
    }

    pub fn err_duplicate(self, name: &str) -> String {
        match self {
            Lang::En => format!("an entry named '{name}' already exists"),
            Lang::Es => format!("ya existe una entrada llamada '{name}'"),
        }
    }

    pub fn err_invalid_command(self, cmd: &str) -> String {
        match self {
            Lang::En => format!("could not resolve command '{cmd}'"),
            Lang::Es => format!("no se pudo resolver el comando '{cmd}'"),
        }
    }

    pub fn err_invalid_path(self, path: &str) -> String {
        match self {
            Lang::En => format!("could not resolve path '{path}'"),
            Lang::Es => format!("no se pudo resolver el path '{path}'"),
        }
    }

    pub fn err_invalid_url(self, url: &str) -> String {
        match self {
            Lang::En => format!("invalid URL '{url}' (expected http:// or https://)"),
            Lang::Es => format!("URL inválida '{url}' (se esperaba http:// o https://)"),
        }
    }

    pub fn err_no_terminal(self) -> String {
        match self {
            Lang::En => "no terminal emulator found. Try setting $TERMINAL \
                         (e.g. `export TERMINAL=alacritty`)"
                .to_string(),
            Lang::Es => "no se encontró un emulador de terminal. Probá \
                         definir $TERMINAL (ej. `export TERMINAL=alacritty`)"
                .to_string(),
        }
    }

    pub fn err_io(self, path: &Path) -> String {
        match self {
            Lang::En => format!("I/O error at {}", path.display()),
            Lang::Es => format!("error de E/S en {}", path.display()),
        }
    }

    pub fn err_yaml(self, path: &Path) -> String {
        match self {
            Lang::En => format!("YAML parse error at {}", path.display()),
            Lang::Es => format!("error al parsear YAML en {}", path.display()),
        }
    }

    // -----------------------------------------------------------------
    //  CLI: mensajes de éxito / info
    // -----------------------------------------------------------------

    pub fn app_added(self, name: &str, path: &str, args: &str) -> String {
        match self {
            Lang::En => {
                if args.is_empty() {
                    format!("✔ Added '{name}' → {path}")
                } else {
                    format!("✔ Added '{name}' → {path} {args}")
                }
            }
            Lang::Es => {
                if args.is_empty() {
                    format!("✔ Agregada '{name}' → {path}")
                } else {
                    format!("✔ Agregada '{name}' → {path} {args}")
                }
            }
        }
    }

    pub fn app_removed(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Removed '{name}'"),
            Lang::Es => format!("✔ Borrada '{name}'"),
        }
    }

    pub fn fav_added(self, name: &str, path: &str) -> String {
        match self {
            Lang::En => format!("✔ Added favorite '{name}' → {path}"),
            Lang::Es => format!("✔ Agregado favorito '{name}' → {path}"),
        }
    }

    pub fn fav_removed(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Removed '{name}'"),
            Lang::Es => format!("✔ Borrado '{name}'"),
        }
    }

    pub fn tool_added(self, name: &str, url: &str) -> String {
        match self {
            Lang::En => format!("✔ Added tool '{name}' → {url}"),
            Lang::Es => format!("✔ Agregado tool '{name}' → {url}"),
        }
    }

    pub fn tool_removed(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Removed tool '{name}'"),
            Lang::Es => format!("✔ Borrado tool '{name}'"),
        }
    }

    pub fn cancelled(self) -> String {
        match self {
            Lang::En => "Cancelled.".to_string(),
            Lang::Es => "Cancelado.".to_string(),
        }
    }

    // -----------------------------------------------------------------
    //  CLI: listas / tablas
    // -----------------------------------------------------------------

    pub fn no_apps(self) -> &'static str {
        match self {
            Lang::En => "No apps registered yet.",
            Lang::Es => "No hay aplicaciones registradas todavía.",
        }
    }

    pub fn no_favs(self) -> &'static str {
        match self {
            Lang::En => "No favorite paths yet.",
            Lang::Es => "No hay rutas favoritas todavía.",
        }
    }

    pub fn no_tools(self) -> &'static str {
        match self {
            Lang::En => "No remote tools yet.",
            Lang::Es => "No hay tools remotos todavía.",
        }
    }

    /// Header de la tabla de apps (columnas).
    pub fn table_header_apps(self) -> &'static str {
        match self {
            Lang::En => "Name\tDescription\tPath\tArgs\tCreated\tLast used",
            Lang::Es => "Nombre\tDescripción\tPath\tArgs\tCreado\tÚltimo uso",
        }
    }

    /// Header de la tabla de favoritos.
    pub fn table_header_favs(self) -> &'static str {
        match self {
            Lang::En => "Name\tDescription\tPath\tCreated\tLast used",
            Lang::Es => "Nombre\tDescripción\tPath\tCreado\tÚltimo uso",
        }
    }

    /// Header de la tabla de tools.
    pub fn table_header_tools(self) -> &'static str {
        match self {
            Lang::En => "Name\tDescription\tURL\tCreated\tLast used",
            Lang::Es => "Nombre\tDescripción\tURL\tCreado\tÚltimo uso",
        }
    }

    pub fn confirm_remove_app(self, name: &str) -> String {
        match self {
            Lang::En => format!("Remove app '{name}'? [y/N] "),
            Lang::Es => format!("¿Borrar la app '{name}'? [s/N] "),
        }
    }

    pub fn confirm_remove_fav(self, name: &str) -> String {
        match self {
            Lang::En => format!("Remove favorite '{name}'? [y/N] "),
            Lang::Es => format!("¿Borrar el favorito '{name}'? [s/N] "),
        }
    }

    pub fn confirm_remove_tool(self, name: &str) -> String {
        match self {
            Lang::En => format!("Remove tool '{name}'? [y/N] "),
            Lang::Es => format!("¿Borrar el tool '{name}'? [s/N] "),
        }
    }

    /// Mensaje de migración automática desde la versión bash.
    pub fn migration_message(self, count: usize) -> String {
        match self {
            Lang::En => format!(
                "mytuis: migrated {count} app(s) from ~/.mytuis.yaml \
                 (backup at ~/.mytuis.yaml.bak)"
            ),
            Lang::Es => format!(
                "mytuis: migradas {count} app(s) desde ~/.mytuis.yaml \
                 (backup en ~/.mytuis.yaml.bak)"
            ),
        }
    }

    // -----------------------------------------------------------------
    //  TUI: header, tabs, lista
    // -----------------------------------------------------------------

    pub fn header_subtitle(self) -> &'static str {
        match self {
            Lang::En => "Application & Paths Manager",
            Lang::Es => "Gestor de Aplicaciones y Rutas",
        }
    }

    pub fn tab_apps(self) -> &'static str {
        match self {
            Lang::En => "Apps",
            Lang::Es => "Apps",
        }
    }

    pub fn tab_favs(self) -> &'static str {
        match self {
            Lang::En => "Favorites",
            Lang::Es => "Favoritos",
        }
    }

    pub fn tab_tools(self) -> &'static str {
        match self {
            Lang::En => "Tools",
            Lang::Es => "Tools",
        }
    }

    pub fn tab_section_hint(self) -> &'static str {
        match self {
            Lang::En => " Section (Tab / 1 / 2 / 3) ",
            Lang::Es => " Sección (Tab / 1 / 2 / 3) ",
        }
    }

    pub fn list_title_apps(self, count: usize, empty: bool) -> String {
        match self {
            Lang::En => {
                if empty {
                    "Apps (empty — press 'a' to add)".to_string()
                } else {
                    format!("Apps ({count})")
                }
            }
            Lang::Es => {
                if empty {
                    "Apps (vacío — apretá 'a' para agregar)".to_string()
                } else {
                    format!("Apps ({count})")
                }
            }
        }
    }

    pub fn list_title_favs(self, count: usize, empty: bool) -> String {
        match self {
            Lang::En => {
                if empty {
                    "Favorites (empty — press 'a' to add)".to_string()
                } else {
                    format!("Favorites ({count})")
                }
            }
            Lang::Es => {
                if empty {
                    "Favoritos (vacío — apretá 'a' para agregar)".to_string()
                } else {
                    format!("Favoritos ({count})")
                }
            }
        }
    }

    pub fn list_title_tools(self, count: usize, empty: bool) -> String {
        match self {
            Lang::En => {
                if empty {
                    "Tools (empty — press 'a' to add)".to_string()
                } else {
                    format!("Tools ({count})")
                }
            }
            Lang::Es => {
                if empty {
                    "Tools (vacío — apretá 'a' para agregar)".to_string()
                } else {
                    format!("Tools ({count})")
                }
            }
        }
    }

    pub fn filter_prompt_empty(self) -> &'static str {
        match self {
            Lang::En => "Type to filter",
            Lang::Es => "Tipea para filtrar",
        }
    }

    pub fn filter_prompt_with(self, query: &str) -> String {
        match self {
            Lang::En => format!("Filter: {query}"),
            Lang::Es => format!("Filtro: {query}"),
        }
    }

    // -----------------------------------------------------------------
    //  TUI: submenú
    // -----------------------------------------------------------------

    pub fn submenu_run_app(self) -> &'static str {
        match self {
            Lang::En => "Run this app",
            Lang::Es => "Ejecutar esta app",
        }
    }

    pub fn submenu_run_fav(self) -> &'static str {
        match self {
            Lang::En => "Open terminal here",
            Lang::Es => "Abrir terminal aquí",
        }
    }

    pub fn submenu_run_tool(self) -> &'static str {
        match self {
            Lang::En => "Open in browser",
            Lang::Es => "Abrir en el navegador",
        }
    }

    pub fn submenu_edit(self) -> &'static str {
        match self {
            Lang::En => "Edit",
            Lang::Es => "Editar",
        }
    }

    pub fn submenu_delete(self) -> &'static str {
        match self {
            Lang::En => "Delete",
            Lang::Es => "Borrar",
        }
    }

    pub fn submenu_copy_path(self) -> &'static str {
        match self {
            Lang::En => "Copy path to clipboard",
            Lang::Es => "Copiar path al portapapeles",
        }
    }

    pub fn submenu_back(self) -> &'static str {
        match self {
            Lang::En => "Back",
            Lang::Es => "Volver",
        }
    }

    pub fn submenu_title(self, name: &str) -> String {
        match self {
            Lang::En => format!(" Actions for '{name}' "),
            Lang::Es => format!(" Acciones para '{name}' "),
        }
    }

    pub fn submenu_hint(self) -> &'static str {
        match self {
            Lang::En => " ↑↓ navigate · Enter execute · Esc back ",
            Lang::Es => " ↑↓ navegar · Enter ejecutar · Esc volver ",
        }
    }

    // -----------------------------------------------------------------
    //  TUI: forms
    // -----------------------------------------------------------------

    pub fn form_new_app_title(self) -> &'static str {
        match self {
            Lang::En => "New app",
            Lang::Es => "Nueva app",
        }
    }

    pub fn form_new_fav_title(self) -> &'static str {
        match self {
            Lang::En => "New favorite",
            Lang::Es => "Nuevo favorito",
        }
    }

    pub fn form_new_tool_title(self) -> &'static str {
        match self {
            Lang::En => "New tool",
            Lang::Es => "Nuevo tool",
        }
    }

    pub fn form_edit_title(self, name: &str) -> String {
        match self {
            Lang::En => format!("Edit '{name}'"),
            Lang::Es => format!("Editar '{name}'"),
        }
    }

    pub fn form_hint(self) -> &'static str {
        match self {
            Lang::En => "Tab to move · Enter to confirm · Esc to cancel",
            Lang::Es => "Tab para mover · Enter para confirmar · Esc para cancelar",
        }
    }

    pub fn field_name(self) -> &'static str {
        match self {
            Lang::En => "Name",
            Lang::Es => "Nombre",
        }
    }

    pub fn field_description(self) -> &'static str {
        match self {
            Lang::En => "Description",
            Lang::Es => "Descripción",
        }
    }

    pub fn field_command(self) -> &'static str {
        match self {
            Lang::En => "Command",
            Lang::Es => "Comando",
        }
    }

    pub fn field_args(self) -> &'static str {
        match self {
            Lang::En => "Extra args",
            Lang::Es => "Args extra",
        }
    }

    pub fn field_path(self) -> &'static str {
        match self {
            Lang::En => "Path",
            Lang::Es => "Path",
        }
    }

    pub fn field_url(self) -> &'static str {
        match self {
            Lang::En => "URL",
            Lang::Es => "URL",
        }
    }

    pub fn form_error_name_required(self) -> &'static str {
        match self {
            Lang::En => "Name and Command are required",
            Lang::Es => "Nombre y Comando son obligatorios",
        }
    }

    pub fn form_error_path_required(self) -> &'static str {
        match self {
            Lang::En => "Name and Path are required",
            Lang::Es => "Nombre y Path son obligatorios",
        }
    }

    pub fn form_error_url_required(self) -> &'static str {
        match self {
            Lang::En => "Name and URL are required",
            Lang::Es => "Nombre y URL son obligatorios",
        }
    }

    pub fn form_error_no_selection_app(self) -> &'static str {
        match self {
            Lang::En => "no app selected",
            Lang::Es => "no hay app seleccionada",
        }
    }

    pub fn form_error_no_selection_fav(self) -> &'static str {
        match self {
            Lang::En => "no favorite selected",
            Lang::Es => "no hay favorito seleccionado",
        }
    }

    pub fn form_error_no_selection_tool(self) -> &'static str {
        match self {
            Lang::En => "no tool selected",
            Lang::Es => "no hay tool seleccionado",
        }
    }

    // -----------------------------------------------------------------
    //  TUI: footers (hint por modo)
    // -----------------------------------------------------------------

    pub fn footer_list(self, is_favs: bool) -> &'static str {
        // El footer varía según el tab activo: en Favoritos la tecla
        // `c` cobra sentido (cd al directorio + exit), mientras que
        // en Apps no. En Tools no hay acción extra (todo se hace con
        // Enter / submenú). Hacemos el dispatch acá para no
        // contaminar la `Tui` con strings hardcodeados.
        //
        // Esta versión de la API sigue aceptando solo `is_favs`
        // porque es la dimensión que afecta el footer (Tools se
        // renderiza igual que Apps en este aspecto).
        match (self, is_favs) {
            (Lang::En, false) => "↑↓ navigate · Enter open · a add · e edit · d del · r run · Tab switch · q quit",
            (Lang::Es, false) => "↑↓ navegar · Enter abrir · a agregar · e editar · d borrar · r ejecutar · Tab cambiar sección · q salir",
            (Lang::En, true) => "↑↓ navigate · Enter open · a add · e edit · d del · r run · g open term · c cd & exit · Tab switch · q quit",
            (Lang::Es, true) => "↑↓ navegar · Enter abrir · a agregar · e editar · d borrar · r ejecutar · g abrir terminal · c cd & salir · Tab cambiar sección · q salir",
        }
    }

    pub fn footer_submenu(self) -> &'static str {
        match self {
            Lang::En => "↑↓ navigate · Enter execute · 1-4 shortcuts · Esc back",
            Lang::Es => "↑↓ navegar · Enter ejecutar · 1-4 atajos · Esc volver",
        }
    }

    pub fn footer_form(self) -> &'static str {
        match self {
            Lang::En => "Tab next field · Enter confirm · Esc cancel",
            Lang::Es => "Tab siguiente campo · Enter confirmar · Esc cancelar",
        }
    }

    pub fn footer_message(self) -> &'static str {
        match self {
            Lang::En => "(press any key to continue)",
            Lang::Es => "(apretá cualquier tecla para continuar)",
        }
    }

    // -----------------------------------------------------------------
    //  TUI: mensajes flash
    // -----------------------------------------------------------------

    pub fn msg_error_title(self) -> &'static str {
        match self {
            Lang::En => " Error ",
            Lang::Es => " Error ",
        }
    }

    pub fn msg_ok_title(self) -> &'static str {
        match self {
            Lang::En => " OK ",
            Lang::Es => " OK ",
        }
    }

    pub fn msg_app_added_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ App '{name}' added"),
            Lang::Es => format!("✔ App '{name}' agregada"),
        }
    }

    pub fn msg_app_updated_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ App '{name}' updated"),
            Lang::Es => format!("✔ App '{name}' actualizada"),
        }
    }

    pub fn msg_app_deleted_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ App '{name}' deleted"),
            Lang::Es => format!("✔ App '{name}' borrada"),
        }
    }

    pub fn msg_fav_added_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Favorite '{name}' added"),
            Lang::Es => format!("✔ Favorito '{name}' agregado"),
        }
    }

    pub fn msg_fav_updated_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Favorite '{name}' updated"),
            Lang::Es => format!("✔ Favorito '{name}' actualizado"),
        }
    }

    pub fn msg_fav_deleted_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Favorite '{name}' deleted"),
            Lang::Es => format!("✔ Favorito '{name}' borrado"),
        }
    }

    pub fn msg_terminal_opened_flash(self, path: &str) -> String {
        match self {
            Lang::En => format!("✔ Terminal opened at {path}"),
            Lang::Es => format!("✔ Terminal abierta en {path}"),
        }
    }

    pub fn msg_path_copied_flash(self, path: &str) -> String {
        match self {
            Lang::En => format!("✔ Path copied to clipboard:\n  {path}"),
            Lang::Es => format!("✔ Path copiado al portapapeles:\n  {path}"),
        }
    }

    pub fn msg_path_copy_failed_flash(self, e: &str) -> String {
        match self {
            Lang::En => format!("Could not copy: {e}"),
            Lang::Es => format!("No se pudo copiar: {e}"),
        }
    }

    pub fn msg_tool_added_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Tool '{name}' added"),
            Lang::Es => format!("✔ Tool '{name}' agregado"),
        }
    }

    pub fn msg_tool_updated_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Tool '{name}' updated"),
            Lang::Es => format!("✔ Tool '{name}' actualizado"),
        }
    }

    pub fn msg_tool_deleted_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Tool '{name}' deleted"),
            Lang::Es => format!("✔ Tool '{name}' borrado"),
        }
    }

    pub fn msg_tool_opened_flash(self, name: &str) -> String {
        match self {
            Lang::En => format!("✔ Tool '{name}' opened"),
            Lang::Es => format!("✔ Tool '{name}' abierto"),
        }
    }

    // -----------------------------------------------------------------
    //  Meta entry "Open here & quit" (tab Favoritos)
    // -----------------------------------------------------------------

    /// Label que se muestra en la meta entry al tope de la lista de
    /// favoritos. El usuario la selecciona y apreta Enter para abrir
    /// una terminal en el favorito debajo + salir de mytuis.
    pub fn meta_open_here(self) -> &'static str {
        match self {
            Lang::En => "[↵] Open here",
            Lang::Es => "[↵] Abrir acá",
        }
    }

    /// Texto de búsqueda para que la meta entry sea filtrable. Mezcla
    /// ambas palabras (inglés y español) para que el usuario pueda
    /// tipear en cualquiera de los dos idiomas.
    pub fn meta_open_here_search(self) -> &'static str {
        match self {
            Lang::En => "open here go ir abrir acá",
            Lang::Es => "open here go ir abrir acá",
        }
    }

    /// Mensaje de error cuando el usuario intenta "open here" pero
    /// no hay ningún favorito seleccionado.
    pub fn err_no_fav_to_open(self) -> &'static str {
        match self {
            Lang::En => "no favorite selected to open here",
            Lang::Es => "no hay favorito seleccionado para abrir",
        }
    }

    /// Mensaje flash que se muestra brevemente antes de salir de
    /// mytuis cuando se abre una terminal con éxito.
    pub fn msg_opened_and_quitting(self, path: &str) -> String {
        match self {
            Lang::En => format!("✔ Opened {path} — exiting mytuis"),
            Lang::Es => format!("✔ Abierto {path} — saliendo de mytuis"),
        }
    }

    /// Mensaje de error que se muestra cuando el usuario aprieta `c`
    /// en la lista de favoritos pero fd 3 no está abierto. La idea
    /// es darle el snippet exacto para configurar el wrapper en su
    /// shell, así no tiene que ir a buscar documentación.
    ///
    /// Devolvemos un `String` (en vez de `&'static str`) porque cada
    /// idioma tiene su propio bloque multilínea.
    pub fn err_no_fd3_wrapper(self) -> String {
        // El snippet es el mismo en ambos idiomas (es código, no se
        // traduce). Solo cambia el texto explicativo.
        const SNIPPET: &str = "mytuis() {\n\
             local out\n\
             out=$(command mytuis \"$@\" 3>&1 1>&2 2>&3)\n\
             [ -n \"$out\" ] && eval \"$out\"\n\
             }";
        match self {
            Lang::En => format!(
                "fd 3 is not open — the shell wrapper is not configured.\n\
                 Add this to your ~/.bashrc (or ~/.zshrc):\n\
                 \n\
                 {SNIPPET}"
            ),
            Lang::Es => format!(
                "fd 3 no está abierto — el wrapper del shell no está configurado.\n\
                 Agregá esto a tu ~/.bashrc (o ~/.zshrc):\n\
                 \n\
                 {SNIPPET}"
            ),
        }
    }

    /// Mensaje flash de éxito cuando la tecla `c` emite `cd <path>`
    /// al fd 3. Se muestra brevemente antes de que la TUI salga.
    pub fn msg_cd_done_flash(self, path: &str) -> String {
        match self {
            Lang::En => format!("✔ cd {path} — exiting mytuis"),
            Lang::Es => format!("✔ cd {path} — saliendo de mytuis"),
        }
    }
}

// ============================================================================
//  Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_value_basic() {
        assert_eq!(from_env_value("en"), Some(Lang::En));
        assert_eq!(from_env_value("es"), Some(Lang::Es));
    }

    #[test]
    fn from_env_value_with_country() {
        assert_eq!(from_env_value("en_US"), Some(Lang::En));
        assert_eq!(from_env_value("es_AR"), Some(Lang::Es));
        assert_eq!(from_env_value("es_ES"), Some(Lang::Es));
    }

    #[test]
    fn from_env_value_with_encoding() {
        assert_eq!(from_env_value("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(from_env_value("es_AR.UTF-8"), Some(Lang::Es));
    }

    #[test]
    fn from_env_value_with_modifier() {
        assert_eq!(from_env_value("de_DE@euro"), None);
        assert_eq!(from_env_value("es_ES@euro"), Some(Lang::Es));
    }

    #[test]
    fn from_env_value_unsupported() {
        assert_eq!(from_env_value("fr"), None);
        assert_eq!(from_env_value("de_DE"), None);
        assert_eq!(from_env_value(""), None);
        assert_eq!(from_env_value("klingon_KH"), None);
    }

    #[test]
    fn from_env_value_case_insensitive() {
        assert_eq!(from_env_value("EN"), Some(Lang::En));
        assert_eq!(from_env_value("Es"), Some(Lang::Es));
        assert_eq!(from_env_value("ES_ar"), Some(Lang::Es));
    }

    #[test]
    fn name_display() {
        assert_eq!(Lang::En.name(), "English");
        assert_eq!(Lang::Es.name(), "Español");
    }

    #[test]
    fn both_languages_have_submenu_back() {
        // Si alguna vez nos olvidamos un brazo del match, esto falla
        // al compilar. Es un test de "compilación".
        assert!(!Lang::En.submenu_back().is_empty());
        assert!(!Lang::Es.submenu_back().is_empty());
    }

    #[test]
    fn interpolated_strings_include_name() {
        assert!(Lang::En.err_not_found("foo").contains("foo"));
        assert!(Lang::Es.err_not_found("foo").contains("foo"));
    }

    #[test]
    fn migration_message_plural_always() {
        // Decisión explícita del plan: forma plural siempre.
        let s1 = Lang::En.migration_message(1);
        let s3 = Lang::En.migration_message(3);
        assert!(s1.contains("1 app"));
        assert!(s3.contains("3 app"));
    }

    #[test]
    fn err_no_fd3_incluye_snippet_wrapper() {
        // El error tiene que incluir el snippet del wrapper para que
        // el usuario pueda copiarlo sin ir a buscar docs. Tiene que
        // aparecer en ambos idiomas porque es código literal.
        let en = Lang::En.err_no_fd3_wrapper();
        let es = Lang::Es.err_no_fd3_wrapper();
        assert!(en.contains("command mytuis \"$@\" 3>&1 1>&2 2>&3"));
        assert!(es.contains("command mytuis \"$@\" 3>&1 1>&2 2>&3"));
        assert!(en.contains("eval \"$out\""));
        assert!(es.contains("eval \"$out\""));
    }

    #[test]
    fn msg_cd_done_incluye_path() {
        let en = Lang::En.msg_cd_done_flash("/datos/pepe");
        let es = Lang::Es.msg_cd_done_flash("/datos/pepe");
        assert!(en.contains("/datos/pepe"));
        assert!(es.contains("/datos/pepe"));
        assert!(en.contains("cd"));
        assert!(es.contains("cd"));
    }

    #[test]
    fn footer_list_favs_incluye_c_y_apps_no() {
        // La tecla `c` solo aparece en el footer del tab Favoritos
        // porque no aplica al tab Apps (ahí sería confuso).
        let apps_en = Lang::En.footer_list(false);
        let favs_en = Lang::En.footer_list(true);
        assert!(!apps_en.contains(" cd "));
        assert!(favs_en.contains(" cd "));
        let apps_es = Lang::Es.footer_list(false);
        let favs_es = Lang::Es.footer_list(true);
        assert!(!apps_es.contains(" cd "));
        assert!(favs_es.contains(" cd "));
    }

    #[test]
    fn tool_strings_presentes_en_ambos_idiomas() {
        // Sanity: los strings del nuevo tab/tool existen en EN y ES
        // (si alguno está vacío en algún idioma, rompimos algo).
        for lang in [Lang::En, Lang::Es] {
            assert!(!lang.no_tools().is_empty());
            assert!(!lang.tab_tools().is_empty());
            assert!(!lang.list_title_tools(0, false).is_empty());
            assert!(!lang.list_title_tools(3, false).is_empty());
            assert!(!lang.form_new_tool_title().is_empty());
            assert!(!lang.field_url().is_empty());
            assert!(!lang.submenu_run_tool().is_empty());
            assert!(lang.tool_added("x", "https://x.test").contains("x"));
            assert!(lang.tool_removed("x").contains("x"));
            assert!(lang.msg_tool_added_flash("x").contains("x"));
            assert!(lang.msg_tool_opened_flash("x").contains("x"));
        }
    }

    #[test]
    fn tab_section_hint_menciona_1_2_3() {
        // El hint ahora debe incluir "3" porque hay tres tabs.
        assert!(Lang::En.tab_section_hint().contains('3'));
        assert!(Lang::Es.tab_section_hint().contains('3'));
    }
}