//! # Definición de la CLI con clap
//!
//! `clap` con el feature `derive` nos permite describir toda la CLI
//! usando structs y enums: clap genera el parser y el `--help`
//! automáticamente.
//!
//! ## Estructura de comandos
//!
//! ```text
//! mytuis                       ← abre TUI (default)
//! mytuis tui                   ← alias explícito
//! mytuis apps list             ← lista apps
//! mytuis apps add NAME DESC CMD ← agrega app
//! mytuis apps remove NAME      ← borra app
//! mytuis paths list            ← lista favoritos
//! mytuis paths add NAME PATH [-d DESC] ← agrega favorito
//! mytuis paths remove NAME     ← borra favorito
//! mytuis paths get NAME        ← imprime path (para `cd`)
//! mytuis help                  ← ayuda
//! ```
//!
//! También mantenemos **aliases** para compat con el script bash
//! original: `mytuis list` ≡ `mytuis apps list`, `mytuis add`
//! ≡ `mytuis apps add`, etc.
//!
//! ## Glosario de clap
//!
//! - `#[command(...)]`: atributos a nivel de `Command` (struct raíz).
//! - `#[derive(Parser)]`: genera el parser para el struct raíz.
//! - `#[derive(Subcommand)]`: enum donde cada variante es un subcomando.
//! - `#[arg(...)]`: configura un argumento (corto, largo, default, etc.).

use clap::{Parser, Subcommand};

/// Parser raíz: argumentos a nivel de programa.
#[derive(Debug, Parser)]
#[command(
    name = "mytuis",
    version,
    about = "Application and favorite-paths manager with a TUI",
    long_about = "mytuis — gestor de aplicaciones y rutas favoritas. \
                   Corre sin argumentos para abrir la TUI, o usá los \
                   subcomandos para operarlo desde la línea de comandos."
)]
pub struct Cli {
    /// Subcomando a ejecutar. Si es `None`, abrimos la TUI.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Todos los subcomandos disponibles. Cada variante es un comando
/// diferente. Los aliases (como `ls` para `list`) se declaran con
/// `#[command(alias = "...")]`.
///
/// Nota: NO tenemos una variante `Help` propia porque clap auto-genera
/// `--help` y `-h` por su cuenta (sería un duplicado y panickea).
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Abre la TUI interactiva (equivalente a no pasar argumentos).
    Tui,

    /// Gestiona el catálogo de aplicaciones.
    #[command(subcommand)]
    Apps(AppsCmd),

    /// Gestiona las rutas favoritas.
    #[command(subcommand)]
    Paths(PathsCmd),
}

/// Subcomandos para `apps`.
#[derive(Debug, Subcommand)]
pub enum AppsCmd {
    /// Lista todas las apps registradas.
    #[command(alias = "ls")]
    List,

    /// Agrega una app. Si los argumentos son insuficientes, abre el
    /// form interactivo de la TUI.
    ///
    /// Ejemplos:
    ///   mytuis apps add nvim "Editor modal" nvim
    ///   mytuis apps add lsl "Listado largo" "ls -lad"
    Add {
        /// Nombre único (la "key") de la app.
        name: String,

        /// Descripción libre.
        description: String,

        /// Comando: ejecutable solo (`firefox`) o con args (`ls -lad`).
        command: String,
    },

    /// Borra una app por nombre. Si no se pasa `--yes`, pide
    /// confirmación cuando está conectado a una TTY.
    #[command(alias = "rm", alias = "del")]
    Remove {
        /// Nombre de la app a borrar.
        name: String,

        /// No pedir confirmación (útil para scripts).
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// Subcomandos para `paths` (favoritos).
#[derive(Debug, Subcommand)]
pub enum PathsCmd {
    /// Lista todos los favoritos.
    #[command(alias = "ls")]
    List,

    /// Agrega un favorito. El path se valida: tiene que existir y
    /// ser un directorio.
    Add {
        /// Nombre único (la "key") del favorito.
        name: String,

        /// Path al directorio. Acepta `~` y paths relativos.
        path: String,

        /// Descripción libre.
        #[arg(short = 'd', long)]
        description: Option<String>,
    },

    /// Borra un favorito por nombre.
    #[command(alias = "rm", alias = "del")]
    Remove {
        /// Nombre del favorito a borrar.
        name: String,

        /// No pedir confirmación.
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Imprime el path al stdout. Pensado para `cd` desde shell:
    ///   cdfav() { cd "$(mytuis paths get "$1")"; }
    Get {
        /// Nombre del favorito a buscar.
        name: String,
    },
}