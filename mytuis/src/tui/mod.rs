//! # TUI: terminal interactiva con `ratatui`
//!
//! Acá vive toda la lógica de la interfaz. La organización interna:
//!
//! 1. **`run()`** — punto de entrada. Configura la terminal (raw mode,
//!    alternate screen, captura el mouse y el teclado), llama al
//!    event loop, y se asegura de restaurar la terminal **siempre**,
//!    incluso si el programa entra en panic.
//!
//! 2. **`Tui`** — struct que guarda TODO el estado mutable de la TUI:
//!    qué tab está activo, qué lista se muestra, qué formulario está
//!    abierto, qué items hay en memoria, etc.
//!
//! 3. **`Mode`** — enum que describe en qué "pantalla" estamos:
//!    viendo la lista, en un submenú, en un form o en un mensaje.
//!
//! 4. **`Tab`** — enum que dice si estamos en apps o en favoritos.
//!
//! 5. **`ui()`** — función que, dado el estado actual, dibuja la
//!    pantalla. Se llama una vez por frame.
//!
//! ## Patrón de event loop
//!
//! ```text
//! loop {
//!     terminal.draw(|f| ui(f, &mut tui))?;
//!     match read_event()? {
//!         Event::Key(k) => tui.on_key(k),
//!         _ => {}
//!     }
//!     if tui.should_quit { break; }
//! }
//! ```
//!
//! `crossterm` lee una tecla por vez con `event::read()`. Es bloqueante
//! pero está bien para nuestro caso: la TUI solo hace I/O cuando el
//! usuario aprieta algo.
//!
//! ## Glosario rápido de ratatui
//!
//! - `Frame`: el "lienzo" sobre el que dibujamos widgets.
//! - `Widget`: cualquier cosa renderizable (`Paragraph`, `List`, etc.).
//! - `Layout`: divide un `Rect` en sub-rectángulos.
//! - `Constraint::Length(n)` / `Percentage(p)` / `Min(0)`: cómo se
//!   reparte el espacio entre los hijos del layout.

use std::io::{stdout, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::prelude::*;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap};

use crate::error::{AppError, Result};
use crate::model::{now_string, App, FavoritePath};
use crate::{open, resolve, storage};

/// `mod` internos del submódulo TUI. Los declaramos acá para que
/// estén visibles como `tui::list_view`, `tui::form`, etc.
mod form;
mod list_view;
pub mod theme;

// ============================================================================
//  Alias del tipo de terminal
// ============================================================================
//
// `Terminal<CrosstermBackend<Stdout>>` es el tipo concreto de terminal
// que vamos a usar. Hacer un type alias evita repetirlo en cada
// función.
type Terminal = ratatui::Terminal<CrosstermBackend<Stdout>>;

// ============================================================================
//  Enums de estado
// ============================================================================

/// Tab activo. Cada tab muestra una lista distinta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Apps,
    Favs,
}

impl Tab {
    /// Devuelve el **otro** tab (para alternar con Tab).
    fn toggle(self) -> Self {
        match self {
            Tab::Apps => Tab::Favs,
            Tab::Favs => Tab::Apps,
        }
    }
}

/// Sub-pantalla actual dentro de un tab.
#[derive(Debug)]
pub enum Mode {
    /// Viendo la lista filtrable.
    List,
    /// Submenú de acciones sobre un item seleccionado.
    SubMenu,
    /// Form modal (Add / Edit).
    Form(FormKind),
    /// Mensaje efímero (popup) — se cierra con cualquier tecla.
    Message { text: String, is_error: bool },
}

/// Qué form está abierto. Acá decidimos qué labels usar y qué hacer
/// al confirmar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    AddApp,
    EditApp,
    AddFav,
    EditFav,
}

/// Acciones del submenú. Para favoritos hay una acción extra:
/// `CopyPath` (copiar el path al portapapeles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubAction {
    Run,
    Edit,
    Delete,
    CopyPath,
    Back,
}

// ============================================================================
//  Estado de la TUI
// ============================================================================

/// `Tui` es el estado mutable global de la TUI. Vive dentro del
/// event loop y se muta en cada tecla.
///
/// Se llama `Tui` para no chocar con `crate::model::App` (que es la
/// entidad "aplicación guardada"). Acá `Tui` significa "estado de la
/// interfaz de usuario".
pub struct Tui {
    // ----- Estado de UI -------------------------------------------------
    pub tab: Tab,
    pub mode: Mode,

    // ----- Datos en memoria ---------------------------------------------
    /// Apps cargadas del YAML al arrancar.
    pub apps: Vec<App>,
    /// Favoritos cargados del YAML al arrancar.
    pub favs: Vec<FavoritePath>,

    // ----- Selección -----------------------------------------------------
    /// Lista filtrable del tab de apps.
    pub app_list: list_view::ListView<App>,
    /// Lista filtrable del tab de favoritos.
    pub fav_list: list_view::ListView<FavoritePath>,

    // ----- Submenú -------------------------------------------------------
    pub sub_actions: Vec<SubAction>,
    pub sub_selected: usize,

    // ----- Form activo ---------------------------------------------------
    pub form: Option<form::FormState>,
    pub form_error: Option<String>,

    // ----- Estado general -----------------------------------------------
    pub should_quit: bool,
}

impl Tui {
    /// Construye un `Tui` nuevo cargando datos desde disco.
    pub fn new(apps: Vec<App>, favs: Vec<FavoritePath>) -> Self {
        // Closures para `ListView`: cómo mostrar cada item y contra qué
        // texto filtra. Las closures se guardan como `Box<dyn Fn>` en
        // el struct, por eso pedimos `'static` (no capturan refs).
        let app_list = list_view::ListView::new(
            apps.clone(),
            |a| {
                if a.description.is_empty() {
                    a.name.clone()
                } else {
                    format!("{} — {}", a.name, truncate(&a.description, 60))
                }
            },
            |a| format!("{} {}", a.name, a.description),
        );

        let fav_list = list_view::ListView::new(
            favs.clone(),
            |f| {
                if f.description.is_empty() {
                    f.path.clone()
                } else {
                    format!("{} — {}", f.name, truncate(&f.description, 60))
                }
            },
            |f| format!("{} {} {}", f.name, f.description, f.path),
        );

        Self {
            tab: Tab::Apps,
            mode: Mode::List,
            apps,
            favs,
            app_list,
            fav_list,
            sub_actions: Vec::new(),
            sub_selected: 0,
            form: None,
            form_error: None,
            should_quit: false,
        }
    }

    /// Recarga todos los datos del disco (después de un add/edit/delete).
    fn reload(&mut self) -> Result<()> {
        self.apps = storage::load_apps()?;
        self.favs = storage::load_favs()?;
        self.app_list.set_items(self.apps.clone());
        self.fav_list.set_items(self.favs.clone());
        Ok(())
    }

    // -------------------------------------------------------------------
    //  Dispatcher de teclas
    // -------------------------------------------------------------------

    /// Maneja una tecla. Decide qué hacer según el modo actual.
    pub fn on_key(&mut self, key: KeyEvent) {
        let in_form = matches!(self.mode, Mode::Form(_));

        // Atajos globales (no cuando estamos tipeando en un form).
        if !in_form {
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.tab = self.tab.toggle();
                    return;
                }
                KeyCode::Char('1') => {
                    self.tab = Tab::Apps;
                    return;
                }
                KeyCode::Char('2') => {
                    self.tab = Tab::Favs;
                    return;
                }
                _ => {}
            }
        }

        match &self.mode {
            Mode::List => self.on_key_list(key),
            Mode::SubMenu => self.on_key_submenu(key),
            Mode::Form(kind) => self.on_key_form(*kind, key),
            Mode::Message { .. } => {
                self.mode = Mode::List;
            }
        }
    }

    fn on_key_list(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => match self.tab {
                Tab::Apps => self.app_list.next(),
                Tab::Favs => self.fav_list.next(),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.tab {
                Tab::Apps => self.app_list.previous(),
                Tab::Favs => self.fav_list.previous(),
            },
            KeyCode::Backspace => match self.tab {
                Tab::Apps => self.app_list.pop_filter_char(),
                Tab::Favs => self.fav_list.pop_filter_char(),
            },
            KeyCode::Esc => match self.tab {
                Tab::Apps => self.app_list.clear_filter(),
                Tab::Favs => self.fav_list.clear_filter(),
            },
            KeyCode::Enter => self.open_submenu(),
            // Atajos de acción: a/e/d/r. Van ANTES del match genérico
            // de `Char(c)` para que no caigan al filtro.
            KeyCode::Char('a') => self.open_add_form(),
            KeyCode::Char('e') => self.open_edit_form(),
            KeyCode::Char('d') => self.delete_selected(),
            KeyCode::Char('r') => self.run_selected(),
            KeyCode::Char(c) => match self.tab {
                Tab::Apps => self.app_list.push_filter_char(c),
                Tab::Favs => self.fav_list.push_filter_char(c),
            },
            _ => {}
        }
    }

    fn on_key_submenu(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.sub_actions.is_empty() {
                    self.sub_selected = (self.sub_selected + 1) % self.sub_actions.len();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.sub_actions.is_empty() {
                    self.sub_selected = if self.sub_selected == 0 {
                        self.sub_actions.len() - 1
                    } else {
                        self.sub_selected - 1
                    };
                }
            }
            KeyCode::Esc | KeyCode::Char('b') | KeyCode::Backspace => {
                self.mode = Mode::List;
            }
            KeyCode::Enter => self.run_submenu_action(),
            KeyCode::Char('1') => {
                if self.sub_actions.len() > 0 {
                    self.sub_selected = 0;
                    self.run_submenu_action();
                }
            }
            KeyCode::Char('2') => {
                if self.sub_actions.len() > 1 {
                    self.sub_selected = 1;
                    self.run_submenu_action();
                }
            }
            KeyCode::Char('3') => {
                if self.sub_actions.len() > 2 {
                    self.sub_selected = 2;
                    self.run_submenu_action();
                }
            }
            KeyCode::Char('4') => {
                if self.sub_actions.len() > 3 {
                    self.sub_selected = 3;
                    self.run_submenu_action();
                }
            }
            _ => {}
        }
    }

    fn on_key_form(&mut self, _kind: FormKind, key: KeyEvent) {
        let form = match self.form.as_mut() {
            Some(f) => f,
            None => {
                self.mode = Mode::List;
                return;
            }
        };

        match key.code {
            KeyCode::Esc => {
                self.form = None;
                self.form_error = None;
                self.mode = Mode::List;
            }
            KeyCode::Tab => form.next_field(),
            KeyCode::BackTab => form.previous_field(),
            KeyCode::Backspace => form.backspace(),
            KeyCode::Enter => self.submit_form(),
            KeyCode::Char(c) => form.insert_char(c),
            _ => {}
        }
    }

    // -------------------------------------------------------------------
    //  Acciones de lista
    // -------------------------------------------------------------------

    fn open_submenu(&mut self) {
        let has_selection = match self.tab {
            Tab::Apps => self.app_list.selected().is_some(),
            Tab::Favs => self.fav_list.selected().is_some(),
        };
        if has_selection {
            self.sub_actions = match self.tab {
                Tab::Apps => vec![
                    SubAction::Run,
                    SubAction::Edit,
                    SubAction::Delete,
                    SubAction::Back,
                ],
                // Los favoritos además tienen "copiar path".
                Tab::Favs => vec![
                    SubAction::Run,
                    SubAction::Edit,
                    SubAction::CopyPath,
                    SubAction::Delete,
                    SubAction::Back,
                ],
            };
            self.sub_selected = 0;
            self.mode = Mode::SubMenu;
        }
    }

    fn run_submenu_action(&mut self) {
        let action = self.sub_actions[self.sub_selected];
        self.mode = Mode::List;
        match action {
            SubAction::Run => self.run_selected(),
            SubAction::Edit => self.open_edit_form(),
            SubAction::Delete => self.delete_selected(),
            SubAction::CopyPath => self.copy_selected_path(),
            SubAction::Back => {}
        }
    }

    /// Copia al portapapeles el path del favorito seleccionado. Útil
    /// para pegarlo en un chat, en un IDE, o para armar un `cd` en otra
    /// terminal.
    fn copy_selected_path(&mut self) {
        if let Some(idx) = self.fav_list.selected_index() {
            let path = self.favs[idx].path.clone();
            match open::copy_to_clipboard(&path) {
                Ok(()) => self.flash_ok(format!("✔ Path copiado al portapapeles:\n  {path}")),
                Err(e) => self.flash_error(format!("No se pudo copiar: {e}")),
            }
        }
    }

    /// Ejecuta la acción "Run" del item seleccionado.
    fn run_selected(&mut self) {
        match self.tab {
            Tab::Apps => {
                if let Some(idx) = self.app_list.selected_index() {
                    if let Err(e) = self.run_app(idx) {
                        self.flash_error(e.to_string());
                    }
                }
            }
            Tab::Favs => {
                if let Some(idx) = self.fav_list.selected_index() {
                    if let Err(e) = self.open_fav(idx) {
                        self.flash_error(e.to_string());
                    }
                }
            }
        }
    }

    /// Lanza una app por índice. Actualiza `last_used` y hace `exec`.
    ///
    /// `exec` reemplaza el proceso de la TUI por la app lanzada, igual
    /// que hacía el bash. Si exec falla, devolvemos un error y la TUI
    /// sigue viva (no se puede volver atrás después de un exec).
    fn run_app(&mut self, idx: usize) -> Result<()> {
        use std::os::unix::process::CommandExt;
        let app = self.apps[idx].clone();

        // Actualizamos `last_used` en disco.
        let now = now_string();
        if let Some(a) = self.apps.get_mut(idx) {
            a.last_used = now;
        }
        storage::save_apps(&self.apps)?;

        eprintln!("mytuis: lanzando '{}'...", app.name);

        let mut cmd = std::process::Command::new(&app.path);
        if !app.args.is_empty() {
            cmd.args(app.args.split_whitespace());
        }
        let err = cmd.exec();
        Err(AppError::other(format!(
            "no se pudo lanzar '{}': {}",
            app.path, err
        )))
    }

    /// Abre una terminal en el directorio del favorito.
    fn open_fav(&mut self, idx: usize) -> Result<()> {
        let fav = self.favs[idx].clone();
        let path = std::path::PathBuf::from(&fav.path);

        let now = now_string();
        if let Some(f) = self.favs.get_mut(idx) {
            f.last_used = now;
        }
        storage::save_favs(&self.favs)?;

        open::open_terminal_in(&path)?;
        self.flash_ok(format!("✔ Terminal abierta en {}", fav.path));
        Ok(())
    }

    // -------------------------------------------------------------------
    //  Forms (Add / Edit)
    // -------------------------------------------------------------------

    fn open_add_form(&mut self) {
        match self.tab {
            Tab::Apps => {
                self.form = Some(form::FormState::new(
                    "Nueva app",
                    "Tab para mover · Enter para confirmar · Esc para cancelar",
                    vec!["Nombre", "Descripción", "Comando", "Args extra"],
                    vec![String::new(), String::new(), String::new(), String::new()],
                ));
                self.form_error = None;
                self.mode = Mode::Form(FormKind::AddApp);
            }
            Tab::Favs => {
                self.form = Some(form::FormState::new(
                    "Nuevo favorito",
                    "Tab para mover · Enter para confirmar · Esc para cancelar",
                    vec!["Nombre", "Path", "Descripción"],
                    vec![String::new(), String::new(), String::new()],
                ));
                self.form_error = None;
                self.mode = Mode::Form(FormKind::AddFav);
            }
        }
    }

    fn open_edit_form(&mut self) {
        match self.tab {
            Tab::Apps => {
                if let Some(app) = self.app_list.selected().cloned() {
                    self.form = Some(form::FormState::new(
                        format!("Editar '{}'", app.name),
                        "Tab para mover · Enter para confirmar · Esc para cancelar",
                        vec!["Nombre", "Descripción", "Comando", "Args extra"],
                        vec![app.name.clone(), app.description.clone(), app.path.clone(), app.args.clone()],
                    ));
                    self.form_error = None;
                    self.mode = Mode::Form(FormKind::EditApp);
                }
            }
            Tab::Favs => {
                if let Some(fav) = self.fav_list.selected().cloned() {
                    self.form = Some(form::FormState::new(
                        format!("Editar '{}'", fav.name),
                        "Tab para mover · Enter para confirmar · Esc para cancelar",
                        vec!["Nombre", "Path", "Descripción"],
                        vec![fav.name.clone(), fav.path.clone(), fav.description.clone()],
                    ));
                    self.form_error = None;
                    self.mode = Mode::Form(FormKind::EditFav);
                }
            }
        }
    }

    fn submit_form(&mut self) {
        let kind = match &self.mode {
            Mode::Form(k) => *k,
            _ => return,
        };
        let values = self.form.as_ref().unwrap().values();
        self.form_error = None;

        let result = match kind {
            FormKind::AddApp => self.form_add_app(&values),
            FormKind::EditApp => self.form_edit_app(&values),
            FormKind::AddFav => self.form_add_fav(&values),
            FormKind::EditFav => self.form_edit_fav(&values),
        };

        match result {
            Ok(msg) => {
                self.form = None;
                self.mode = Mode::List;
                self.flash_ok(msg);
            }
            Err(e) => {
                self.form_error = Some(e.to_string());
            }
        }
    }

    fn form_add_app(&mut self, values: &[String]) -> Result<String> {
        let name = values[0].trim();
        let desc = values[1].trim();
        let cmd = values[2].trim();
        let extra_args = values[3].trim();

        if name.is_empty() || cmd.is_empty() {
            return Err(AppError::other("Nombre y Comando son obligatorios"));
        }
        if self.apps.iter().any(|a| a.name == name) {
            return Err(AppError::Duplicate(name.to_string()));
        }
        let resolved = resolve::resolve_command(cmd);
        if !resolved.is_ok() {
            return Err(AppError::InvalidCommand(cmd.to_string()));
        }
        let combined = if resolved.args.is_empty() {
            extra_args.to_string()
        } else if extra_args.is_empty() {
            resolved.args.clone()
        } else {
            format!("{} {}", resolved.args, extra_args)
        };

        let now = now_string();
        let app = App::new(name, desc, &resolved.path, &combined, now);
        self.apps.push(app);
        storage::save_apps(&self.apps)?;
        self.app_list.set_items(self.apps.clone());

        Ok(format!("✔ App '{name}' agregada"))
    }

    fn form_edit_app(&mut self, values: &[String]) -> Result<String> {
        let new_name = values[0].trim().to_string();
        let new_desc = values[1].trim().to_string();
        let new_cmd = values[2].trim().to_string();
        let new_extra = values[3].trim().to_string();

        let idx = self
            .app_list
            .selected_index()
            .ok_or_else(|| AppError::other("no hay app seleccionada"))?;
        let old_name = self.apps[idx].name.clone();

        if new_name.is_empty() || new_cmd.is_empty() {
            return Err(AppError::other("Nombre y Comando son obligatorios"));
        }
        if new_name != old_name && self.apps.iter().any(|a| a.name == new_name) {
            return Err(AppError::Duplicate(new_name.clone()));
        }

        let resolved = resolve::resolve_command(&new_cmd);
        if !resolved.is_ok() {
            return Err(AppError::InvalidCommand(new_cmd.clone()));
        }
        let combined = if resolved.args.is_empty() {
            new_extra.clone()
        } else if new_extra.is_empty() {
            resolved.args.clone()
        } else {
            format!("{} {}", resolved.args, new_extra)
        };

        self.apps[idx].name = new_name.clone();
        self.apps[idx].description = new_desc;
        self.apps[idx].path = resolved.path;
        self.apps[idx].args = combined;
        storage::save_apps(&self.apps)?;
        self.app_list.set_items(self.apps.clone());

        Ok(format!("✔ App '{new_name}' actualizada"))
    }

    fn form_add_fav(&mut self, values: &[String]) -> Result<String> {
        let name = values[0].trim();
        let path_input = values[1].trim();
        let desc = values[2].trim();

        if name.is_empty() || path_input.is_empty() {
            return Err(AppError::other("Nombre y Path son obligatorios"));
        }
        if self.favs.iter().any(|f| f.name == name) {
            return Err(AppError::Duplicate(name.to_string()));
        }

        let resolved = resolve::resolve_favorite_dir(path_input)?;
        let now = now_string();
        let fav = FavoritePath::new(
            name,
            desc,
            resolved.to_string_lossy().as_ref(),
            now,
        );
        self.favs.push(fav);
        storage::save_favs(&self.favs)?;
        self.fav_list.set_items(self.favs.clone());

        Ok(format!("✔ Favorito '{name}' agregado"))
    }

    fn form_edit_fav(&mut self, values: &[String]) -> Result<String> {
        let new_name = values[0].trim().to_string();
        let new_path_input = values[1].trim().to_string();
        let new_desc = values[2].trim().to_string();

        let idx = self
            .fav_list
            .selected_index()
            .ok_or_else(|| AppError::other("no hay favorito seleccionado"))?;
        let old_name = self.favs[idx].name.clone();

        if new_name.is_empty() || new_path_input.is_empty() {
            return Err(AppError::other("Nombre y Path son obligatorios"));
        }
        if new_name != old_name && self.favs.iter().any(|f| f.name == new_name) {
            return Err(AppError::Duplicate(new_name.clone()));
        }

        let resolved = resolve::resolve_favorite_dir(&new_path_input)?;
        self.favs[idx].name = new_name.clone();
        self.favs[idx].path = resolved.to_string_lossy().to_string();
        self.favs[idx].description = new_desc;
        storage::save_favs(&self.favs)?;
        self.fav_list.set_items(self.favs.clone());

        Ok(format!("✔ Favorito '{new_name}' actualizado"))
    }

    // -------------------------------------------------------------------
    //  Delete
    // -------------------------------------------------------------------

    fn delete_selected(&mut self) {
        let result = match self.tab {
            Tab::Apps => {
                if let Some(idx) = self.app_list.selected_index() {
                    let name = self.apps[idx].name.clone();
                    self.apps.remove(idx);
                    match storage::save_apps(&self.apps) {
                        Ok(()) => {
                            self.app_list.set_items(self.apps.clone());
                            Ok(format!("✔ App '{name}' borrada"))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    return;
                }
            }
            Tab::Favs => {
                if let Some(idx) = self.fav_list.selected_index() {
                    let name = self.favs[idx].name.clone();
                    self.favs.remove(idx);
                    match storage::save_favs(&self.favs) {
                        Ok(()) => {
                            self.fav_list.set_items(self.favs.clone());
                            Ok(format!("✔ Favorito '{name}' borrado"))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    return;
                }
            }
        };

        match result {
            Ok(msg) => self.flash_ok(msg),
            Err(e) => self.flash_error(e.to_string()),
        }
    }

    // -------------------------------------------------------------------
    //  Mensajes efímeros
    // -------------------------------------------------------------------

    fn flash_ok(&mut self, msg: String) {
        self.mode = Mode::Message {
            text: msg,
            is_error: false,
        };
    }

    fn flash_error(&mut self, msg: String) {
        self.mode = Mode::Message {
            text: msg,
            is_error: true,
        };
    }
}

// ============================================================================
//  Helpers de texto
// ============================================================================

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

// ============================================================================
//  UI: dibuja el frame
// ============================================================================

fn ui(frame: &mut Frame, tui: &mut Tui) {
    let area = frame.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    draw_header(frame, chunks[0]);
    draw_tabs(frame, chunks[1], tui.tab);

    // Copiamos datos del mensaje ANTES del match para no tener un
    // borrow inmutable y otro mutable al mismo tiempo.
    let msg: Option<(String, bool)> = if let Mode::Message { text, is_error } = &tui.mode {
        Some((text.clone(), *is_error))
    } else {
        None
    };

    match &tui.mode {
        Mode::List => draw_list(frame, chunks[2], tui),
        Mode::SubMenu => draw_submenu(frame, chunks[2], tui),
        Mode::Form(_) => draw_list(frame, chunks[2], tui),
        Mode::Message { .. } => draw_list(frame, chunks[2], tui),
    }

    if let Some((text, is_error)) = msg {
        draw_message(frame, &text, is_error);
    }

    draw_footer(frame, chunks[3], &tui.mode);

    if let Some(form) = tui.form.as_mut() {
        form::render_form(frame, form, tui.form_error.as_deref());
    }
}

fn draw_header(frame: &mut Frame, area: Rect) {
    let title = Line::from(vec![
        Span::styled("mytuis", theme::title_style()),
        Span::raw("  ::  "),
        Span::styled("Application & Paths Manager", theme::subtitle_style()),
    ]);
    let p = Paragraph::new(title)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::muted_border_style()),
        );
    frame.render_widget(p, area);
}

fn draw_tabs(frame: &mut Frame, area: Rect, current: Tab) {
    let titles = vec![
        Line::from(Span::raw("Apps")),
        Line::from(Span::raw("Favoritos")),
    ];
    let selected = match current {
        Tab::Apps => 0,
        Tab::Favs => 1,
    };
    let tabs = Tabs::new(titles)
        .select(selected)
        .style(theme::muted_style())
        .highlight_style(theme::selected_style())
        .divider(" | ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::muted_border_style())
                .title(" Sección (Tab / 1 / 2) "),
        );
    frame.render_widget(tabs, area);
}

fn draw_list(frame: &mut Frame, area: Rect, tui: &mut Tui) {
    match tui.tab {
        Tab::Apps => {
            let title = if tui.apps.is_empty() {
                "Apps (vacío — apretá 'a' para agregar)".to_string()
            } else {
                format!("Apps ({})", tui.apps.len())
            };
            tui.app_list.render(frame, area, &title);
        }
        Tab::Favs => {
            let title = if tui.favs.is_empty() {
                "Favoritos (vacío — apretá 'a' para agregar)".to_string()
            } else {
                format!("Favoritos ({})", tui.favs.len())
            };
            tui.fav_list.render(frame, area, &title);
        }
    }
}

fn draw_submenu(frame: &mut Frame, area: Rect, tui: &mut Tui) {
    let selected_name = match tui.tab {
        Tab::Apps => tui.app_list.selected().map(|a| a.name.clone()).unwrap_or_default(),
        Tab::Favs => tui.fav_list.selected().map(|f| f.name.clone()).unwrap_or_default(),
    };

    let items: Vec<ListItem> = tui
        .sub_actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let label = match a {
                SubAction::Run => match tui.tab {
                    Tab::Apps => "▶  Ejecutar esta app",
                    Tab::Favs => "▶  Abrir terminal aquí",
                },
                SubAction::Edit => "✎  Editar",
                SubAction::Delete => "🗑  Borrar",
                SubAction::CopyPath => "📋  Copiar path al portapapeles",
                SubAction::Back => "←  Volver",
            };
            let prefix = if i == tui.sub_selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{label}"))
        })
        .collect();

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(tui.sub_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::highlighted_border_style())
                .title(format!(" Acciones para '{selected_name}' "))
                .title_bottom(" ↑↓ navegar · Enter ejecutar · Esc volver "),
        )
        .highlight_style(theme::selected_style())
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_message(frame: &mut Frame, text: &str, is_error: bool) {
    let area = centered_rect(60, 25, frame.size());
    frame.render_widget(Clear, area);
    let style = if is_error {
        theme::error_style()
    } else {
        theme::success_style()
    };
    let border_style = style;
    let p = Paragraph::new(text)
        .style(style)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(if is_error { " Error " } else { " OK " })
                .title_bottom(" (apretá cualquier tecla) "),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(p, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, mode: &Mode) {
    let hint = match mode {
        Mode::List => "↑↓ navegar · Enter abrir · a agregar · e editar · d borrar · r ejecutar · Tab cambiar sección · q salir",
        Mode::SubMenu => "↑↓ navegar · Enter ejecutar · 1-4 atajos · Esc volver",
        Mode::Form(_) => "Tab siguiente campo · Enter confirmar · Esc cancelar",
        Mode::Message { .. } => "(cualquier tecla para continuar)",
    };
    let p = Paragraph::new(hint)
        .style(theme::muted_style())
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::muted_border_style()),
        );
    frame.render_widget(p, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}

// ============================================================================
//  Entry point público de la TUI
// ============================================================================

/// Arranca la TUI. Configura la terminal, carga datos, entra al
/// event loop. Devuelve `Result` para que el caller (`main`) reporte
/// el error si algo falla.
pub fn run() -> Result<()> {
    let apps = storage::load_apps()?;
    let favs = storage::load_favs()?;
    let mut tui = Tui::new(apps, favs);

    enable_raw_mode().map_err(|e| AppError::other(format!("raw mode: {e}")))?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| AppError::other(format!("alternate screen: {e}")))?;
    let backend = CrosstermBackend::new(out);
    let mut terminal =
        Terminal::new(backend).map_err(|e| AppError::other(format!("terminal: {e}")))?;

    let result = run_loop(&mut terminal, &mut tui);

    // Pase lo que pase, restauramos. Usamos `.ok()` para no enmascarar
    // el error original con un fallo en la restauración.
    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();

    result
}

/// El event loop propiamente dicho. Está separado para que `run()`
/// quede chico y el manejo de "siempre restaurar" sea claro.
fn run_loop(terminal: &mut Terminal, tui: &mut Tui) -> Result<()> {
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        terminal
            .draw(|f| ui(f, tui))
            .map_err(|e| AppError::other(format!("draw: {e}")))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).map_err(|e| AppError::other(format!("poll: {e}")))? {
            if let Event::Key(key) = event::read().map_err(|e| AppError::other(format!("read: {e}")))? {
                tui.on_key(key);
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if tui.should_quit {
            return Ok(());
        }
    }
}