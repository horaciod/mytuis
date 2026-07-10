//! # Entrypoint de `mytuis`
//!
//! La función `main` es el punto de entrada que el sistema operativo
//! llama cuando ejecutamos el binario. Acá hacemos:
//!
//! 1. Detectar el idioma del usuario (de variables de entorno).
//! 2. Migrar datos de la versión bash si hace falta.
//! 3. Parsear los argumentos de la CLI con clap.
//! 4. Despachar al handler correspondiente: TUI o subcomando CLI.
//!
//! ## Manejo de errores
//!
//! Rust no tiene "excepciones top-level": si algo falla, devolvemos un
//! `Result` desde `main`. `std::process::ExitCode` se usa para
//! terminar el proceso con un código de salida específico. La
//! convención es: `0` = éxito, `!= 0` = error.
//!
//! Los mensajes de error al usuario se imprimen **localizados**
//! usando `AppError::localized(lang)`.

use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

mod cli;
mod config;
mod error;
mod lang;
mod model;
mod open;
mod resolve;
mod storage;
mod tui;

use cli::{AppsCmd, Cli, Command, PathsCmd, ToolsCmd};
use error::{AppError, Result};
use lang::Lang;

/// `main` devuelve `ExitCode` para poder mapear nuestros errores a
/// códigos de salida Unix. Esto es equivalente a `fn main() -> Result<...>`
/// pero más explícito.
fn main() -> ExitCode {
    // Detectamos el idioma ANTES de cualquier otra cosa. La detección
    // es barata (lee 3 variables de entorno) y nos permite localizar
    // incluso mensajes tempranos como el de la migración bash.
    let lang = Lang::detect();

    match run(lang) {
        Ok(()) => ExitCode::from(0),
        Err(e) => {
            // Si el error es nuestro (AppError), lo localizamos. Si es
            // un anyhow::Error (envoltura de algo externo), mostramos
            // el mensaje tal cual.
            if let Some(app_err) = e.downcast_ref::<AppError>() {
                eprintln!("mytuis: {}", app_err.localized(lang));
            } else {
                eprintln!("mytuis: {e:#}");
            }
            ExitCode::from(1)
        }
    }
}

/// Función interna que devuelve `anyhow::Result`. Usamos `anyhow`
/// porque queremos poder mezclar nuestro `AppError` con errores de
/// librerías externas (clap, serde_yaml, etc.) sin andar haciendo
/// conversiones a mano.
///
/// Recibe `lang` para localizar todos los mensajes que imprime.
fn run(lang: Lang) -> anyhow::Result<()> {
    // ------------------------------------------------------------------
    // 1. Migración silenciosa desde la versión bash.
    // ------------------------------------------------------------------
    // Si el usuario tenía `~/.mytuis.yaml` y todavía no hay
    // `~/.mytuis/apps.yaml`, lo importamos. Esto se hace **antes** de
    // parsear la CLI porque la CLI ya podría querer leer apps.
    if let Some(count) = storage::migrate_from_bash_if_needed()
        .context("migrating bash legacy data")?
    {
        eprintln!("{}", lang.migration_message(count));
    }

    // ------------------------------------------------------------------
    // 2. Compat con la versión bash: rewrite de argv.
    // ------------------------------------------------------------------
    // La versión bash aceptaba `mytuis list`, `mytuis add ...` y
    // `mytuis remove <name>` a nivel top-level. Acá nuestra estructura
    // canónica es `mytuis apps list`, etc. Para mantener la
    // compatibilidad, reescribimos los aliases antes de pasar a clap.
    let argv = rewrite_legacy_argv(std::env::args().collect());

    // ------------------------------------------------------------------
    // 3. Parseo de la CLI con clap.
    // ------------------------------------------------------------------
    let cli = Cli::parse_from(argv);

    // ------------------------------------------------------------------
    // 4. Despacho al subcomando (o TUI si no hay ninguno).
    // ------------------------------------------------------------------
    match cli.command {
        // Sin subcomando → abrir TUI.
        None => {
            tui::run(lang).context("TUI error")?;
        }

        // TUI explícito.
        Some(Command::Tui) => {
            tui::run(lang).context("TUI error")?;
        }

        // Apps.
        Some(Command::Apps(cmd)) => handle_apps(lang, cmd)?,

        // Paths (favoritos).
        Some(Command::Paths(cmd)) => handle_paths(lang, cmd)?,

        // Tools (aplicaciones remotas / URLs).
        Some(Command::Tools(cmd)) => handle_tools(lang, cmd)?,
    }

    Ok(())
}

/// Reescribe los aliases top-level de la versión bash al esquema
/// actual (`apps`/`paths`). Por ejemplo:
///
/// ```text
/// mytuis list                  →  mytuis apps list
/// mytuis ls                    →  mytuis apps list
/// mytuis add NAME DESC CMD     →  mytuis apps add NAME DESC CMD
/// mytuis remove NAME           →  mytuis apps remove NAME
/// ```
///
/// Si el primer argumento no es uno de estos aliases, devuelve el
/// argv sin tocar (caso normal).
fn rewrite_legacy_argv(argv: Vec<String>) -> Vec<String> {
    if argv.len() < 2 {
        return argv;
    }
    match argv[1].as_str() {
        "list" | "ls" => {
            let mut out = vec![argv[0].clone(), "apps".to_string(), "list".to_string()];
            out.extend_from_slice(&argv[2..]);
            out
        }
        "add" => {
            let mut out = vec![argv[0].clone(), "apps".to_string(), "add".to_string()];
            out.extend_from_slice(&argv[2..]);
            out
        }
        "remove" | "rm" | "del" => {
            let mut out = vec![argv[0].clone(), "apps".to_string(), "remove".to_string()];
            out.extend_from_slice(&argv[2..]);
            out
        }
        _ => argv,
    }
}

// ============================================================================
//  Handlers de subcomandos APPS
// ============================================================================

fn handle_apps(lang: Lang, cmd: AppsCmd) -> anyhow::Result<()> {
    match cmd {
        AppsCmd::List => cmd_apps_list(lang)?,
        AppsCmd::Add { name, description, command } => {
            cmd_apps_add(lang, &name, &description, &command)?
        }
        AppsCmd::Remove { name, yes } => cmd_apps_remove(lang, &name, yes)?,
    }
    Ok(())
}

/// `mytuis apps list` — imprime la tabla de apps.
///
/// Si stdout es una TTY, intenta colorear. Si no (pipe a `grep`, etc.),
/// imprime texto plano tabulado para que sea fácil de procesar.
fn cmd_apps_list(lang: Lang) -> Result<()> {
    let apps = storage::load_apps()?;

    if apps.is_empty() {
        println!("{}", lang.no_apps());
        return Ok(());
    }

    // Construimos filas tabuladas. Truncamos descripción y args a 40
    // caracteres para mantener la tabla compacta.
    let rows: Vec<String> = apps
        .iter()
        .map(|a| {
            let desc = truncate(&a.description, 40);
            let args = truncate(&a.args, 40);
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                a.name, desc, a.path, args, a.created, a.last_used,
            )
        })
        .collect();

    if atty_stdout() {
        // TTY → tabla con borde. Sin crate extra, hacemos nuestra
        // propia versión minimalista con líneas.
        print_table(
            &lang.table_header_apps()
                .split('\t')
                .collect::<Vec<_>>(),
            &rows,
        );
    } else {
        // Pipe → TSV.
        println!("{}", lang.table_header_apps());
        for r in &rows {
            println!("{r}");
        }
    }
    Ok(())
}

/// `mytuis apps add <name> <desc> <command>` — agrega una app.
fn cmd_apps_add(lang: Lang, name: &str, description: &str, command: &str) -> Result<()> {
    // 1. Resolver el comando a un path absoluto.
    let resolved = resolve::resolve_command(command);
    if !resolved.is_ok() {
        return Err(AppError::InvalidCommand(command.to_string()));
    }

    // 2. Verificar duplicado.
    let mut apps = storage::load_apps()?;
    if apps.iter().any(|a| a.name == name) {
        return Err(AppError::Duplicate(name.to_string()));
    }

    // 3. Construir la app y guardarla.
    let now = model::now_string();
    let app = model::App::new(name, description, &resolved.path, &resolved.args, now);
    apps.push(app);
    storage::save_apps(&apps)?;

    println!("{}", lang.app_added(name, &resolved.path, &resolved.args));
    Ok(())
}

/// `mytuis apps remove <name>` — borra una app.
fn cmd_apps_remove(lang: Lang, name: &str, yes: bool) -> Result<()> {
    let mut apps = storage::load_apps()?;

    let pos = apps.iter().position(|a| a.name == name);
    let pos = match pos {
        Some(p) => p,
        None => return Err(AppError::NotFound(name.to_string())),
    };

    // Pedir confirmación si estamos en TTY y no se pasó --yes.
    if !yes && atty_stdin() && atty_stdout() {
        if !confirm(&lang.confirm_remove_app(name), lang)? {
            println!("{}", lang.cancelled());
            return Ok(());
        }
    }

    apps.remove(pos);
    storage::save_apps(&apps)?;
    println!("{}", lang.app_removed(name));
    Ok(())
}

// ============================================================================
//  Handlers de subcomandos PATHS (favoritos)
// ============================================================================

fn handle_paths(lang: Lang, cmd: PathsCmd) -> anyhow::Result<()> {
    match cmd {
        PathsCmd::List => cmd_paths_list(lang)?,
        PathsCmd::Add { name, path, description } => {
            cmd_paths_add(lang, &name, &path, description.as_deref().unwrap_or(""))?
        }
        PathsCmd::Remove { name, yes } => cmd_paths_remove(lang, &name, yes)?,
        PathsCmd::Get { name } => cmd_paths_get(&name)?,
        PathsCmd::Go { name } => cmd_paths_go(lang, &name)?,
        PathsCmd::Cd { name } => cmd_paths_cd(lang, &name)?,
    }
    Ok(())
}

/// `mytuis paths list` — lista los favoritos.
fn cmd_paths_list(lang: Lang) -> Result<()> {
    let favs = storage::load_favs()?;
    if favs.is_empty() {
        println!("{}", lang.no_favs());
        return Ok(());
    }

    let rows: Vec<String> = favs
        .iter()
        .map(|f| {
            let desc = truncate(&f.description, 40);
            format!("{}\t{}\t{}\t{}\t{}", f.name, desc, f.path, f.created, f.last_used)
        })
        .collect();

    if atty_stdout() {
        print_table(
            &lang.table_header_favs()
                .split('\t')
                .collect::<Vec<_>>(),
            &rows,
        );
    } else {
        println!("{}", lang.table_header_favs());
        for r in &rows {
            println!("{r}");
        }
    }
    Ok(())
}

/// `mytuis paths add <name> <path> [-d desc]` — agrega un favorito.
fn cmd_paths_add(lang: Lang, name: &str, path: &str, description: &str) -> Result<()> {
    let resolved = resolve::resolve_favorite_dir(path)?;
    let mut favs = storage::load_favs()?;
    if favs.iter().any(|f| f.name == name) {
        return Err(AppError::Duplicate(name.to_string()));
    }
    let now = model::now_string();
    let fav = model::FavoritePath::new(
        name,
        description,
        resolved.to_string_lossy().as_ref(),
        now,
    );
    favs.push(fav);
    storage::save_favs(&favs)?;
    println!("{}", lang.fav_added(name, &resolved.to_string_lossy()));
    Ok(())
}

/// `mytuis paths remove <name>` — borra un favorito.
fn cmd_paths_remove(lang: Lang, name: &str, yes: bool) -> Result<()> {
    let mut favs = storage::load_favs()?;
    let pos = favs.iter().position(|f| f.name == name);
    let pos = match pos {
        Some(p) => p,
        None => return Err(AppError::NotFound(name.to_string())),
    };

    if !yes && atty_stdin() && atty_stdout() {
        if !confirm(&lang.confirm_remove_fav(name), lang)? {
            println!("{}", lang.cancelled());
            return Ok(());
        }
    }

    favs.remove(pos);
    storage::save_favs(&favs)?;
    println!("{}", lang.fav_removed(name));
    Ok(())
}

/// `mytuis paths get <name>` — imprime el path al stdout. Pensado para
/// `cd "$(mytuis paths get mi-proyecto)"`.
///
/// No recibe `lang` porque el output (el path) es siempre literal —
/// no se traduce. Si el favorito no existe, devolvemos `AppError::NotFound`
/// que será localizado en `main`.
fn cmd_paths_get(name: &str) -> Result<()> {
    let favs = storage::load_favs()?;
    match favs.iter().find(|f| f.name == name) {
        Some(f) => {
            println!("{}", f.path);
            Ok(())
        }
        None => Err(AppError::NotFound(name.to_string())),
    }
}

/// `mytuis paths go <name>` — equivalente CLI de la meta entry
/// `[↵] Open here` de la TUI. Abre una terminal en el directorio
/// del favorito y sale.
///
/// Útil para keybindings del shell:
///
/// ```bash
/// # En .bashrc:
/// gocd() { mytuis paths go "$1"; }
/// ```
///
/// Pasos:
/// 1. Carga los favoritos y busca el nombre.
/// 2. Valida que el path todavía exista como directorio.
/// 3. Actualiza `last_used`.
/// 4. Lanza la terminal (vía `open::open_terminal_in`).
/// 5. Sale silenciosamente (exit 0). Los errores se reportan al
///    stderr vía `AppError::localized(lang)`.
fn cmd_paths_go(lang: Lang, name: &str) -> Result<()> {
    let mut favs = storage::load_favs()?;
    let fav = favs
        .iter()
        .find(|f| f.name == name)
        .cloned()
        .ok_or_else(|| AppError::NotFound(name.to_string()))?;

    // Resolvemos y validamos el path. Esto también expande `~` por
    // si el usuario editó el YAML a mano.
    let resolved = resolve::resolve_favorite_dir(&fav.path)?;

    // Actualizamos `last_used` antes de abrir la terminal (por si
    // falla el spawn, al menos queda registrado).
    let now = model::now_string();
    if let Some(f) = favs.iter_mut().find(|f| f.name == name) {
        f.last_used = now.clone();
        f.path = resolved.to_string_lossy().to_string();
    }
    storage::save_favs(&favs)?;

    // Mensaje breve al stderr (la stdout queda limpia para pipes).
    eprintln!("{}", lang.msg_opened_and_quitting(&resolved.to_string_lossy()));

    // Abrimos la terminal. Si no hay emulador, devolvemos
    // AppError::NoTerminal que será localizado en main.
    open::open_terminal_in(&resolved)?;

    Ok(())
}

/// `mytuis paths cd <name>` — equivalente CLI de la tecla `c` de la
/// TUI: emite `cd <path>` al descriptor de archivo 3 (side channel
/// estándar) y sale. NO abre una terminal nueva.
///
/// A diferencia de `paths go`, este subcomando está pensado para
/// integrarse en un wrapper del shell:
///
/// ```bash
/// # En .bashrc / .zshrc:
/// mytuis() {
///     local out
///     out=$(command mytuis "$@" 3>&1 1>&2 2>&3)
///     [ -n "$out" ] && eval "$out"
/// }
/// ```
///
/// Pasos:
/// 1. Carga los favoritos y busca el nombre.
/// 2. Valida y resuelve el path.
/// 3. Emite `cd <path>\n` al fd 3 (vía `open::emit_cd_to_fd3`).
/// 4. Sale silenciosamente (exit 0) si la emisión fue exitosa.
///
/// Si fd 3 no está abierto (no hay wrapper configurado), devuelve
/// error con el snippet del wrapper. La stdout queda limpia para
/// que el `$(...)` del wrapper solo vea el `cd <path>`.
fn cmd_paths_cd(lang: Lang, name: &str) -> Result<()> {
    let favs = storage::load_favs()?;
    let fav = favs
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| AppError::NotFound(name.to_string()))?;

    // Resolvemos para expandir `~` y validar que el dir exista.
    let resolved = resolve::resolve_favorite_dir(&fav.path)?;

    // Emitimos al fd 3. Esto puede fallar si el shell no configuró
    // el wrapper; en ese caso, dejamos que el error suba con un
    // mensaje localizado.
    open::emit_cd_to_fd3(&resolved)?;

    // Mensaje de despedida al stderr (NO stdout — esa va al fd 3
    // para que el wrapper la evalúe, y no queremos contaminarla).
    eprintln!("{}", lang.msg_cd_done_flash(&resolved.to_string_lossy()));

    Ok(())
}

// ============================================================================
//  Handlers de subcomandos TOOLS (aplicaciones remotas)
// ============================================================================

fn handle_tools(lang: Lang, cmd: ToolsCmd) -> anyhow::Result<()> {
    match cmd {
        ToolsCmd::List => cmd_tools_list(lang)?,
        ToolsCmd::Add {
            name,
            description,
            url,
        } => cmd_tools_add(lang, &name, &description, &url)?,
        ToolsCmd::Remove { name, yes } => cmd_tools_remove(lang, &name, yes)?,
        ToolsCmd::Run { name } => cmd_tools_run(lang, &name)?,
    }
    Ok(())
}

/// `mytuis tools list` — imprime la tabla de tools.
fn cmd_tools_list(lang: Lang) -> Result<()> {
    let tools = storage::load_tools()?;

    if tools.is_empty() {
        println!("{}", lang.no_tools());
        return Ok(());
    }

    let rows: Vec<String> = tools
        .iter()
        .map(|t| {
            let desc = truncate(&t.description, 40);
            format!(
                "{}\t{}\t{}\t{}\t{}",
                t.name, desc, t.url, t.created, t.last_used,
            )
        })
        .collect();

    if atty_stdout() {
        print_table(
            &lang.table_header_tools()
                .split('\t')
                .collect::<Vec<_>>(),
            &rows,
        );
    } else {
        println!("{}", lang.table_header_tools());
        for r in &rows {
            println!("{r}");
        }
    }
    Ok(())
}

/// `mytuis tools add <name> <desc> <url>` — agrega un tool.
///
/// Validamos la URL con `resolve::resolve_tool_url` (esquema http/https
/// y host no vacío) **antes** de tocar storage, así no se queda un
/// archivo inconsistente si la URL es inválida.
fn cmd_tools_add(lang: Lang, name: &str, description: &str, url: &str) -> Result<()> {
    // 1. Validar URL (devuelve String normalizada).
    let url = resolve::resolve_tool_url(url)?;

    // 2. Verificar duplicado.
    let mut tools = storage::load_tools()?;
    if tools.iter().any(|t| t.name == name) {
        return Err(AppError::Duplicate(name.to_string()));
    }

    // 3. Construir el tool y guardarlo.
    let now = model::now_string();
    let tool = model::Tool::new(name, description, &url, now);
    tools.push(tool);
    storage::save_tools(&tools)?;

    println!("{}", lang.tool_added(name, &url));
    Ok(())
}

/// `mytuis tools remove <name>` — borra un tool.
fn cmd_tools_remove(lang: Lang, name: &str, yes: bool) -> Result<()> {
    let mut tools = storage::load_tools()?;

    let pos = tools.iter().position(|t| t.name == name);
    let pos = match pos {
        Some(p) => p,
        None => return Err(AppError::NotFound(name.to_string())),
    };

    if !yes && atty_stdin() && atty_stdout() {
        if !confirm(&lang.confirm_remove_tool(name), lang)? {
            println!("{}", lang.cancelled());
            return Ok(());
        }
    }

    tools.remove(pos);
    storage::save_tools(&tools)?;
    println!("{}", lang.tool_removed(name));
    Ok(())
}

/// `mytuis tools run <name>` — abre la URL del tool con el opener del
/// sistema y actualiza `last_used`. Equivalente a la acción Run de la
/// TUI.
fn cmd_tools_run(lang: Lang, name: &str) -> Result<()> {
    let mut tools = storage::load_tools()?;
    let tool = tools
        .iter()
        .find(|t| t.name == name)
        .cloned()
        .ok_or_else(|| AppError::NotFound(name.to_string()))?;

    // Validamos la URL antes de abrir (por si el usuario editó el YAML
    // a mano). Si está malformada, fallamos sin tocar `last_used`.
    let url = resolve::resolve_tool_url(&tool.url)?;

    // Actualizamos `last_used` antes de abrir (si falla el opener, al
    // menos queda registrado).
    let now = model::now_string();
    if let Some(t) = tools.iter_mut().find(|t| t.name == name) {
        t.last_used = now;
    }
    storage::save_tools(&tools)?;

    eprintln!("{}", lang.msg_tool_opened_flash(name));
    open::open_url(&url)?;
    Ok(())
}

// ============================================================================
//  Helpers de I/O / formateo
// ============================================================================

/// Trunca `s` a `max` caracteres y agrega `…` si se cortó.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

/// Detecta si stdout es una terminal (true → TTY).
fn atty_stdout() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

/// Detecta si stdin es una terminal.
fn atty_stdin() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
}

/// Imprime una tabla minimalista con líneas. Anchos calculados a
/// partir del contenido.
fn print_table(headers: &[&str], rows: &[String]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.split('\t').enumerate() {
            if i >= widths.len() {
                widths.push(0);
            }
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let format_row = |cells: &[&str]| {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let w = widths.get(i).copied().unwrap_or(0);
                format!("{:<w$}", c)
            })
            .collect::<Vec<_>>()
            .join("  ")
    };

    let total_width: usize = widths.iter().sum::<usize>() + 2 * (widths.len().saturating_sub(1));
    println!("{}", "─".repeat(total_width));
    println!("{}", format_row(headers));
    println!("{}", "─".repeat(total_width));
    for row in rows {
        let cells: Vec<&str> = row.split('\t').collect();
        println!("{}", format_row(&cells));
    }
    println!("{}", "─".repeat(total_width));
}

/// Pregunta de sí/no por stdin. Las respuestas válidas se eligen según
/// el idioma: en inglés `y`/`yes`; en español `s`/`si`.
fn confirm(prompt: &str, lang: Lang) -> Result<bool> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut buf = String::new();
    let n = std::io::stdin()
        .read_line(&mut buf)
        .map_err(|e| AppError::other(format!("reading stdin: {e}")))?;
    if n == 0 {
        return Ok(false);
    }
    let ans = buf.trim().to_lowercase();
    let yes_set: &[&str] = match lang {
        Lang::En => &["y", "yes"],
        Lang::Es => &["s", "si", "y", "yes"],
    };
    Ok(yes_set.contains(&ans.as_str()))
}