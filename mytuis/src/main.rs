//! # Entrypoint de `mytuis`
//!
//! La función `main` es el punto de entrada que el sistema operativo
//! llama cuando ejecutamos el binario. Acá hacemos tres cosas:
//!
//! 1. Parsear los argumentos de la CLI con clap.
//! 2. Migrar datos de la versión bash si hace falta (silencioso, salvo
//!    que haya algo que reportar).
//! 3. Despachar al handler correspondiente: TUI o subcomando CLI.
//!
//! ## Manejo de errores
//!
//! Rust no tiene "excepciones top-level": si algo falla, devolvemos un
//! `Result` desde `main`. `std::process::exit(code)` se usa para
//! terminar el proceso con un código de salida específico. La
//! convención es: `0` = éxito, `!= 0` = error.
//!
//! Acá usamos el truco de `anyhow::Result` para `main`, lo que nos
//! permite usar `?` libremente sin preocuparnos por el tipo concreto
//! del error. El `Display` del error (que ya implementamos en
//! `error.rs` con `thiserror`) se imprime al stderr.

use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

mod cli;
mod config;
mod error;
mod model;
mod open;
mod resolve;
mod storage;
mod tui;

use cli::{AppsCmd, Cli, Command, PathsCmd};
use error::{AppError, Result};

/// `main` devuelve `ExitCode` para poder mapear nuestros errores a
/// códigos de salida Unix. Esto es equivalente a `fn main() -> Result<...>`
/// pero más explícito.
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::from(0),
        Err(e) => {
            // Imprimimos el error al stderr (no a stdout, que es para
            // datos que otros programas pueden consumir).
            eprintln!("mytuis: {e:#}");
            // Si fue un error nuestro, devolvemos 1. Si fue un error
            // de I/O del sistema, también 1.
            ExitCode::from(1)
        }
    }
}

/// Función interna que devuelve `anyhow::Result`. Usamos `anyhow`
/// porque queremos poder mezclar nuestro `AppError` con errores de
/// librerías externas (clap, serde_yaml, etc.) sin andar haciendo
/// conversiones a mano.
fn run() -> anyhow::Result<()> {
    // ------------------------------------------------------------------
    // 1. Migración silenciosa desde la versión bash.
    // ------------------------------------------------------------------
    // Si el usuario tenía `~/.mytuis.yaml` y todavía no hay
    // `~/.mytuis/apps.yaml`, lo importamos. Esto se hace **antes** de
    // parsear la CLI porque la CLI ya podría querer leer apps.
    if let Some(count) = storage::migrate_from_bash_if_needed()
        .context("migrando datos de la versión bash")?
    {
        eprintln!(
            "mytuis: migradas {count} app(s) desde ~/.mytuis.yaml \
             (backup en ~/.mytuis.yaml.bak)"
        );
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
            tui::run().context("error en la TUI")?;
        }

        // TUI explícito.
        Some(Command::Tui) => {
            tui::run().context("error en la TUI")?;
        }

        // Apps.
        Some(Command::Apps(cmd)) => handle_apps(cmd)?,

        // Paths (favoritos).
        Some(Command::Paths(cmd)) => handle_paths(cmd)?,
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
    // El argv siempre tiene al menos el nombre del binario en [0].
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

fn handle_apps(cmd: AppsCmd) -> anyhow::Result<()> {
    match cmd {
        AppsCmd::List => cmd_apps_list()?,
        AppsCmd::Add { name, description, command } => {
            cmd_apps_add(&name, &description, &command)?
        }
        AppsCmd::Remove { name, yes } => cmd_apps_remove(&name, yes)?,
    }
    Ok(())
}

/// `mytuis apps list` — imprime la tabla de apps.
///
/// Si stdout es una TTY, intenta colorear. Si no (pipe a `grep`, etc.),
/// imprime texto plano tabulado para que sea fácil de procesar.
fn cmd_apps_list() -> Result<()> {
    let apps = storage::load_apps()?;

    if apps.is_empty() {
        if atty_stdout() {
            println!("No hay aplicaciones registradas todavía.");
        } else {
            println!("No hay aplicaciones registradas todavía.");
        }
        return Ok(());
    }

    // Construimos filas tabuladas. Truncamos descripción y args a 40
    // caracteres para mantener la tabla compacta (igual que la versión
    // bash).
    let rows: Vec<String> = apps
        .iter()
        .map(|a| {
            let desc = truncate(&a.description, 40);
            let args = truncate(&a.args, 40);
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                a.name,
                desc,
                a.path,
                args,
                a.created,
                a.last_used,
            )
        })
        .collect();

    if atty_stdout() {
        // TTY → tabla con borde. `comfy-table` no está en deps, así
        // que hacemos nuestra propia versión minimalista con `|`.
        print_table(&[
            "Nombre", "Descripción", "Path", "Args", "Creado", "Último uso",
        ], &rows);
    } else {
        // Pipe → emitimos TSV (tab-separated values), fácil de grepear.
        println!("Nombre\tDescripción\tPath\tArgs\tCreado\tÚltimo uso");
        for r in &rows {
            println!("{r}");
        }
    }
    Ok(())
}

/// `mytuis apps add <name> <desc> <command>` — agrega una app.
fn cmd_apps_add(name: &str, description: &str, command: &str) -> Result<()> {
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

    if resolved.args.is_empty() {
        println!("✔ Agregada '{name}' → {}", resolved.path);
    } else {
        println!("✔ Agregada '{name}' → {} {}", resolved.path, resolved.args);
    }
    Ok(())
}

/// `mytuis apps remove <name>` — borra una app.
fn cmd_apps_remove(name: &str, yes: bool) -> Result<()> {
    let mut apps = storage::load_apps()?;

    let pos = apps.iter().position(|a| a.name == name);
    let pos = match pos {
        Some(p) => p,
        None => return Err(AppError::NotFound(name.to_string())),
    };

    // Pedir confirmación si estamos en TTY y no se pasó --yes.
    if !yes && atty_stdin() && atty_stdout() {
        if !confirm(&format!("¿Borrar la app '{name}'?"))? {
            println!("Cancelado.");
            return Ok(());
        }
    }

    apps.remove(pos);
    storage::save_apps(&apps)?;
    println!("✔ Borrada '{name}'");
    Ok(())
}

// ============================================================================
//  Handlers de subcomandos PATHS (favoritos)
// ============================================================================

fn handle_paths(cmd: PathsCmd) -> anyhow::Result<()> {
    match cmd {
        PathsCmd::List => cmd_paths_list()?,
        PathsCmd::Add { name, path, description } => {
            cmd_paths_add(&name, &path, description.as_deref().unwrap_or(""))?
        }
        PathsCmd::Remove { name, yes } => cmd_paths_remove(&name, yes)?,
        PathsCmd::Get { name } => cmd_paths_get(&name)?,
    }
    Ok(())
}

/// `mytuis paths list` — lista los favoritos.
fn cmd_paths_list() -> Result<()> {
    let favs = storage::load_favs()?;
    if favs.is_empty() {
        println!("No hay rutas favoritas todavía.");
        return Ok(());
    }

    let rows: Vec<String> = favs
        .iter()
        .map(|f| {
            let desc = truncate(&f.description, 40);
            format!(
                "{}\t{}\t{}\t{}\t{}",
                f.name, desc, f.path, f.created, f.last_used,
            )
        })
        .collect();

    if atty_stdout() {
        print_table(
            &["Nombre", "Descripción", "Path", "Creado", "Último uso"],
            &rows,
        );
    } else {
        println!("Nombre\tDescripción\tPath\tCreado\tÚltimo uso");
        for r in &rows {
            println!("{r}");
        }
    }
    Ok(())
}

/// `mytuis paths add <name> <path> [-d desc]` — agrega un favorito.
fn cmd_paths_add(name: &str, path: &str, description: &str) -> Result<()> {
    // 1. Resolver y validar el path.
    let resolved = resolve::resolve_favorite_dir(path)?;

    // 2. Verificar duplicado.
    let mut favs = storage::load_favs()?;
    if favs.iter().any(|f| f.name == name) {
        return Err(AppError::Duplicate(name.to_string()));
    }

    // 3. Guardar.
    let now = model::now_string();
    let fav = model::FavoritePath::new(
        name,
        description,
        resolved.to_string_lossy().as_ref(),
        now,
    );
    favs.push(fav);
    storage::save_favs(&favs)?;

    println!("✔ Agregado favorito '{name}' → {}", resolved.display());
    Ok(())
}

/// `mytuis paths remove <name>` — borra un favorito.
fn cmd_paths_remove(name: &str, yes: bool) -> Result<()> {
    let mut favs = storage::load_favs()?;
    let pos = favs.iter().position(|f| f.name == name);
    let pos = match pos {
        Some(p) => p,
        None => return Err(AppError::NotFound(name.to_string())),
    };

    if !yes && atty_stdin() && atty_stdout() {
        if !confirm(&format!("¿Borrar el favorito '{name}'?"))? {
            println!("Cancelado.");
            return Ok(());
        }
    }

    favs.remove(pos);
    storage::save_favs(&favs)?;
    println!("✔ Borrado '{name}'");
    Ok(())
}

/// `mytuis paths get <name>` — imprime el path al stdout. Pensado para
/// `cd "$(mytuis paths get mi-proyecto)"`.
fn cmd_paths_get(name: &str) -> Result<()> {
    let favs = storage::load_favs()?;
    match favs.iter().find(|f| f.name == name) {
        Some(f) => {
            // Imprimimos sin newline final extra para que sea más
            // fácil de capturar, pero en realidad `println!` siempre
            // pone uno. Si el usuario quiere sin newline, debería
            // usar `tr -d '\n'` o redireccionar.
            println!("{}", f.path);
            Ok(())
        }
        None => Err(AppError::NotFound(name.to_string())),
    }
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

/// Detecta si stdout es una terminal (true → TTY). Usamos `isatty` del
/// crate `std` directamente, sin crate externo, para no agregar deps.
fn atty_stdout() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

/// Detecta si stdin es una terminal.
fn atty_stdin() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
}

/// Imprime una tabla minimalista. Anchos calculados a partir del
/// contenido. No es la tabla más linda del mundo pero se ve decente y
/// no requiere dependencias.
fn print_table(headers: &[&str], rows: &[String]) {
    // Calcular ancho de cada columna.
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.split('\t').enumerate() {
            if i >= widths.len() {
                widths.push(0);
            }
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    // Helper para formatear una fila.
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

    // Borde superior.
    let total_width: usize = widths.iter().sum::<usize>()
        + 2 * (widths.len().saturating_sub(1));
    println!("{}", "─".repeat(total_width));

    // Header.
    println!("{}", format_row(headers));
    println!("{}", "─".repeat(total_width));

    // Filas.
    for row in rows {
        let cells: Vec<&str> = row.split('\t').collect();
        println!("{}", format_row(&cells));
    }
    println!("{}", "─".repeat(total_width));
}

/// Pregunta de sí/no por stdin. Lee una línea y considera "s", "S",
/// "si", "yes", "y" como sí; cualquier otra cosa como no.
///
/// Devuelve `true` si el usuario confirmó, `false` si dijo que no.
/// Si no se puede leer stdin (pipe), devuelve `false` por seguridad.
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::Write;
    print!("{prompt} [s/N] ");
    std::io::stdout().flush().ok();
    let mut buf = String::new();
    let n = std::io::stdin().read_line(&mut buf).map_err(|e| {
        AppError::other(format!("leyendo stdin: {e}"))
    })?;
    if n == 0 {
        // EOF → no.
        return Ok(false);
    }
    let ans = buf.trim().to_lowercase();
    Ok(matches!(ans.as_str(), "s" | "si" | "y" | "yes"))
}