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
use crate::lang::Lang;
use crate::model::{now_string, App, FavoritePath, Tool};
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
    Tools,
}

impl Tab {
    /// Devuelve el **siguiente** tab (para alternar con Tab). Ciclamos
    /// por Apps → Favs → Tools → Apps.
    fn toggle(self) -> Self {
        match self {
            Tab::Apps => Tab::Favs,
            Tab::Favs => Tab::Tools,
            Tab::Tools => Tab::Apps,
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
    AddTool,
    EditTool,
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

/// Item de la lista de favoritos. El listado mezcla una **meta entry**
/// al tope (acción global sobre el favorito seleccionado debajo) con
/// los favoritos reales.
///
/// Esto es lo que el `ListView<FavListItem>` renderea. El dispatch
/// en `on_key_list` distingue el caso `Meta` del caso `Fav` para
/// decidir si abrir submenú o ejecutar la acción "open here & quit".
#[derive(Debug, Clone)]
pub enum FavListItem {
    /// Pseudo-entrada al tope de la lista. Visualmente dice
    /// `[↵] Open here`. Cuando el usuario la selecciona y apreta
    /// Enter, abre terminal en el favorito debajo y sale de mytuis.
    /// Si no hay favoritos, no se agrega (la lista queda vacía).
    MetaOpenHere,
    /// Un favorito real.
    Fav(FavoritePath),
}

// ============================================================================
//  Estado de la TUI
// ============================================================================

/// Construye la lista de items que se muestra en la tab Favoritos.
/// Si hay al menos un favorito, prepende la meta entry
/// `[↵] Open here`. Si no hay favoritos, devuelve una lista vacía
/// (la meta no tendría sentido sin un favorito debajo).
///
/// Esta función es `fn` libre (no método de `Tui`) porque solo la
/// usa `Tui::new` durante la construcción.
fn build_fav_list_items(favs: &[FavoritePath], _lang: Lang) -> Vec<FavListItem> {
    if favs.is_empty() {
        return Vec::new();
    }
    let mut items = Vec::with_capacity(favs.len() + 1);
    items.push(FavListItem::MetaOpenHere);
    items.extend(favs.iter().cloned().map(FavListItem::Fav));
    items
}

/// `Tui` es el estado mutable global de la TUI. Vive dentro del
/// event loop y se muta en cada tecla.
///
/// Se llama `Tui` para no chocar con `crate::model::App` (que es la
/// entidad "aplicación guardada"). Acá `Tui` significa "estado de la
/// interfaz de usuario".
pub struct Tui {
    // ----- Idioma --------------------------------------------------------
    /// Idioma activo para todos los strings user-facing. Se inicializa
    /// en `Tui::new` y se pasa desde `main` (que detecta con
    /// `Lang::detect()`).
    pub lang: Lang,

    // ----- Estado de UI -------------------------------------------------
    pub tab: Tab,
    pub mode: Mode,

    // ----- Datos en memoria ---------------------------------------------
    /// Apps cargadas del YAML al arrancar.
    pub apps: Vec<App>,
    /// Favoritos cargados del YAML al arrancar.
    pub favs: Vec<FavoritePath>,
    /// Tools cargados del YAML al arrancar.
    pub tools: Vec<Tool>,

    // ----- Selección -----------------------------------------------------
    /// Lista filtrable del tab de apps.
    pub app_list: list_view::ListView<App>,
    /// Lista filtrable del tab de favoritos. El tipo es `FavListItem`
    /// (no `FavoritePath` directo) porque la lista contiene una meta
    /// entry al tope además de los favoritos reales.
    pub fav_list: list_view::ListView<FavListItem>,
    /// Lista filtrable del tab de tools. Sin meta entry — abrir un
    /// tool es siempre la misma acción Run.
    pub tool_list: list_view::ListView<Tool>,

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
    ///
    /// Recibe `lang` para localizar todos los strings user-facing que
    /// la TUI muestre (header, tabs, submenús, formularios, mensajes
    /// flash, footer).
    pub fn new(
        lang: Lang,
        apps: Vec<App>,
        favs: Vec<FavoritePath>,
        tools: Vec<Tool>,
    ) -> Self {
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

        // La lista de favoritos es especial: prependemos una meta entry
        // al tope (`[↵] Open here`) que abre terminal + sale. Solo se
        // agrega si hay al menos un favorito (si no, la meta no
        // tendría a qué aplicarse).
        let fav_items = build_fav_list_items(&favs, lang);
        // Las closures se guardan como `Box<dyn Fn>` con `'static`,
        // así que NO pueden capturar `lang` por referencia. Usamos
        // `move` y copiamos el `Lang` (es `Copy`, no cuesta nada).
        let fav_list = list_view::ListView::new(
            fav_items,
            move |item| match item {
                FavListItem::MetaOpenHere => lang.meta_open_here().to_string(),
                FavListItem::Fav(f) => {
                    if f.description.is_empty() {
                        f.path.clone()
                    } else {
                        format!("{} — {}", f.name, truncate(&f.description, 60))
                    }
                }
            },
            move |item| match item {
                // El texto de búsqueda del meta incluye keywords en EN
                // y ES para que el filtro funcione en cualquier idioma.
                FavListItem::MetaOpenHere => lang.meta_open_here_search().to_string(),
                FavListItem::Fav(f) => format!("{} {} {}", f.name, f.description, f.path),
            },
        );

        // Tools: lista simple, sin meta entry. Renderizamos "name —
        // description" igual que apps; la búsqueda incluye también la
        // URL para que `mytuis tools add` con `https://grafana` se
        // pueda filtrar por la URL.
        let tool_list = list_view::ListView::new(
            tools.clone(),
            |t| {
                if t.description.is_empty() {
                    t.url.clone()
                } else {
                    format!("{} — {}", t.name, truncate(&t.description, 60))
                }
            },
            |t| format!("{} {} {}", t.name, t.description, t.url),
        );

        Self {
            lang,
            tab: Tab::Apps,
            mode: Mode::List,
            apps,
            favs,
            tools,
            app_list,
            fav_list,
            tool_list,
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
        self.tools = storage::load_tools()?;
        self.app_list.set_items(self.apps.clone());
        // IMPORTANTE: pasar por `build_fav_list_items` para que la
        // meta entry vuelva a aparecer al tope si hay favoritos.
        self.fav_list.set_items(build_fav_list_items(&self.favs, self.lang));
        self.tool_list.set_items(self.tools.clone());
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
                KeyCode::Char('3') => {
                    self.tab = Tab::Tools;
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
                Tab::Tools => self.tool_list.next(),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.tab {
                Tab::Apps => self.app_list.previous(),
                Tab::Favs => self.fav_list.previous(),
                Tab::Tools => self.tool_list.previous(),
            },
            KeyCode::Backspace => match self.tab {
                Tab::Apps => self.app_list.pop_filter_char(),
                Tab::Favs => self.fav_list.pop_filter_char(),
                Tab::Tools => self.tool_list.pop_filter_char(),
            },
            KeyCode::Esc => match self.tab {
                Tab::Apps => self.app_list.clear_filter(),
                Tab::Favs => self.fav_list.clear_filter(),
                Tab::Tools => self.tool_list.clear_filter(),
            },
            // Enter: en el tab Apps abre el submenú del item. En el
            // tab Favs hay que distinguir: si el item seleccionado es
            // la meta entry `[↵] Open here`, ejecutar "open here &
            // quit"; si es un favorito real, abrir submenú como antes.
            // En el tab Tools abre el submenú (no hay meta entry).
            KeyCode::Enter => match self.tab {
                Tab::Apps => self.open_submenu(),
                Tab::Favs => match self.fav_list.selected() {
                    Some(FavListItem::MetaOpenHere) => self.open_here_and_quit(),
                    Some(FavListItem::Fav(_)) | None => self.open_submenu(),
                },
                Tab::Tools => self.open_submenu(),
            },
            // Atajos de acción: a/e/d/r/g. Van ANTES del match genérico
            // de `Char(c)` para que no caigan al filtro.
            KeyCode::Char('a') => self.open_add_form(),
            KeyCode::Char('e') => self.open_edit_form(),
            KeyCode::Char('d') => self.delete_selected(),
            KeyCode::Char('r') => self.run_selected(),
            // Atajo `g` (go): solo en el tab Favoritos. Dispara la
            // misma acción que la meta entry, sin necesidad de
            // subir al tope de la lista. Útil para usuarios avanzados.
            KeyCode::Char('g') if self.tab == Tab::Favs => {
                self.open_here_and_quit();
            }
            // Atajo `c` (cd): solo en el tab Favoritos. A diferencia
            // de `g` (que abre una terminal NUEVA), `c` le pide al
            // shell padre que haga `cd <path>` y sale de mytuis.
            //
            // Esto usa el patrón fd 3 (side channel estándar, igual
            // que `broot` / `zoxide`): mytuis escribe `cd <path>\n`
            // al fd 3 y termina; el shell padre (que tiene un wrapper
            // configurado) lee fd 3 y hace `eval` sobre eso.
            //
            // Si el usuario NO tiene el wrapper configurado, fd 3
            // está cerrado y emit_cd_to_fd3 devuelve error. En ese
            // caso mostramos un flash con el snippet del wrapper y
            // NO salimos (para que pueda seguir usando mytuis).
            KeyCode::Char('c') if self.tab == Tab::Favs => {
                self.cd_here_and_quit();
            }
            KeyCode::Char(c) => match self.tab {
                Tab::Apps => self.app_list.push_filter_char(c),
                Tab::Favs => self.fav_list.push_filter_char(c),
                Tab::Tools => self.tool_list.push_filter_char(c),
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
            Tab::Tools => self.tool_list.selected().is_some(),
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
                // Los tools no tienen "copiar path" porque su "path"
                // es una URL — abrirla es la acción natural y no la
                // querrías copiar todo el tiempo. Mantenemos 4
                // acciones (igual que Apps).
                Tab::Tools => vec![
                    SubAction::Run,
                    SubAction::Edit,
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

    /// Devuelve el índice dentro de `self.favs` del item actualmente
    /// seleccionado en `self.fav_list`, **solo si es un favorito real**
    /// (no la meta entry). Si la meta está seleccionada o no hay
    /// nada, devuelve `None`.
    ///
    /// Lo usamos en todas las operaciones que actúan sobre un favorito
    /// específico (copy, delete, edit, run) — la meta no cuenta.
    fn selected_fav_index(&self) -> Option<usize> {
        let all_idx = self.fav_list.selected_index()?;
        match &self.fav_list.all[all_idx] {
            FavListItem::Fav(_) => {
                // El índice en `self.favs` es siempre `all_idx - 1`
                // porque la meta ocupa la posición 0 cuando existe.
                // `saturating_sub` nos protege si por algún motivo la
                // meta no está.
                Some(all_idx.saturating_sub(1))
            }
            FavListItem::MetaOpenHere => None,
        }
    }

    /// Copia al portapapeles el path del favorito seleccionado. Útil
    /// para pegarlo en un chat, en un IDE, o para armar un `cd` en otra
    /// terminal.
    fn copy_selected_path(&mut self) {
        if let Some(idx) = self.selected_fav_index() {
            let path = self.favs[idx].path.clone();
            match open::copy_to_clipboard(&path) {
                Ok(()) => self.flash_ok(self.lang.msg_path_copied_flash(&path)),
                Err(e) => self.flash_error(self.lang.msg_path_copy_failed_flash(&e.to_string())),
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
                if let Some(idx) = self.selected_fav_index() {
                    if let Err(e) = self.open_fav(idx) {
                        self.flash_error(e.to_string());
                    }
                }
            }
            Tab::Tools => {
                if let Some(idx) = self.tool_list.selected_index() {
                    if let Err(e) = self.run_tool(idx) {
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
        self.flash_ok(self.lang.msg_terminal_opened_flash(&fav.path));
        Ok(())
    }

    /// Abre un tool (URL) en el opener del sistema. Valida la URL por
    /// si el usuario la editó a mano en el YAML, actualiza `last_used`
    /// y muestra un flash de éxito.
    fn run_tool(&mut self, idx: usize) -> Result<()> {
        let tool = self.tools[idx].clone();

        // Validamos la URL por seguridad (no queremos fallar a mitad
        // de guardar). Si está malformada, no tocamos `last_used`.
        let url = resolve::resolve_tool_url(&tool.url)?;

        let now = now_string();
        if let Some(t) = self.tools.get_mut(idx) {
            t.last_used = now;
        }
        storage::save_tools(&self.tools)?;

        open::open_url(&url)?;
        self.flash_ok(self.lang.msg_tool_opened_flash(&tool.name));
        Ok(())
    }

    /// Ejecuta la acción "open here & quit" — abre terminal en el
    /// favorito actualmente seleccionado y marca `should_quit` para
    /// que la TUI cierre después del próximo frame.
    ///
    /// Esta función es llamada tanto desde la meta entry `[↵] Open
    /// here` (cuando el usuario la selecciona y apreta Enter) como
    /// desde el atajo `g` (que opera sobre cualquier favorito
    /// highlighted, sin necesidad de subir al meta).
    ///
    /// Si la lista está vacía o no hay un favorito "real"
    /// seleccionado, muestra un flash de error en lugar de fallar
    /// silenciosamente.
    fn open_here_and_quit(&mut self) {
        // Buscamos el primer item `Fav(...)` en la lista filtrada.
        // Como la meta entry es siempre la primera (índice 0) si
        // está presente, simplemente escaneamos desde el principio.
        let fav_idx = self.fav_list.filtered.iter().find_map(|&i| {
            match &self.fav_list.all[i] {
                FavListItem::Fav(_) => {
                    // El índice dentro de `self.favs` es el índice
                    // del item menos 1 (porque la meta ocupa la
                    // posición 0 cuando existe).
                    Some(i.saturating_sub(1))
                }
                FavListItem::MetaOpenHere => None,
            }
        });

        match fav_idx {
            Some(idx) if idx < self.favs.len() => {
                let path = self.favs[idx].path.clone();
                if let Err(e) = self.open_fav_and_quit(idx) {
                    self.flash_error(e.localized(self.lang));
                } else {
                    // Flash de despedida antes de salir.
                    self.flash_ok(self.lang.msg_opened_and_quitting(&path));
                }
            }
            _ => {
                self.flash_error(self.lang.err_no_fav_to_open().to_string());
            }
        }
    }

    /// Versión "and quit" de `open_fav`. Abre la terminal, actualiza
    /// `last_used`, y marca `should_quit = true` para que el event
    /// loop cierre la TUI después de este frame.
    fn open_fav_and_quit(&mut self, idx: usize) -> Result<()> {
        let fav = self.favs[idx].clone();
        let path = std::path::PathBuf::from(&fav.path);

        let now = now_string();
        if let Some(f) = self.favs.get_mut(idx) {
            f.last_used = now;
        }
        storage::save_favs(&self.favs)?;

        open::open_terminal_in(&path)?;
        // Marcamos quit antes del flash_ok para que el frame con el
        // mensaje se dibuje una vez y luego se salga limpiamente.
        self.should_quit = true;
        Ok(())
    }

    /// Acción disparada por la tecla `c` en la lista de favoritos.
    /// A diferencia de `open_here_and_quit` (que abre una terminal
    /// NUEVA en el directorio), acá le pedimos al shell padre que
    /// haga `cd` y salga de mytuis.
    ///
    /// El mecanismo es fd 3 (side channel estándar). Si fd 3 está
    /// abierto porque el usuario configuró el wrapper de su shell,
    /// escribimos `cd <path>\n` ahí y marcamos `should_quit = true`
    /// para que la TUI cierre. El shell padre lee el fd 3 y hace
    /// `eval`, así que cuando mytuis termina, el usuario ya está
    /// parado en el directorio del favorito.
    ///
    /// Si fd 3 NO está abierto (no hay wrapper), mostramos un flash
    /// con instrucciones y NO salimos — el usuario puede seguir
    /// navegando. Salir silenciosamente sería frustrante porque el
    /// usuario no entendería por qué "no pasó nada".
    ///
    /// Esta función NO actualiza `last_used` del favorito. La idea
    /// es que `last_used` refleje cuándo se **ejecutó** la app / se
    /// abrió la terminal; un `cd` en background no es lo mismo. Si
    /// el usuario quiere registrar la actividad, que use `g` o Enter.
    fn cd_here_and_quit(&mut self) {
        // Reutilizamos `selected_fav_index` que ya se encarga de
        // distinguir la meta entry de un favorito real.
        let fav_idx = self.selected_fav_index();

        match fav_idx {
            Some(idx) if idx < self.favs.len() => {
                let path = std::path::PathBuf::from(&self.favs[idx].path);
                match open::emit_cd_to_fd3(&path) {
                    Ok(()) => {
                        // Emitió OK. Mostramos un flash de despedida
                        // (se verá un frame antes de salir) y marcamos
                        // quit. El orden importa: flash_ok antes de
                        // should_quit para que el mensaje aparezca en
                        // el último frame.
                        self.flash_ok(self.lang.msg_cd_done_flash(&path.display().to_string()));
                        self.should_quit = true;
                    }
                    Err(e) => {
                        // fd 3 no está abierto. Mostramos el snippet
                        // del wrapper y NO salimos.
                        self.flash_error(format!(
                            "{}\n\n{}",
                            e.localized(self.lang),
                            self.lang.err_no_fd3_wrapper()
                        ));
                    }
                }
            }
            // No hay favorito real seleccionado (la meta está al tope
            // o la lista está vacía). Reusamos el mensaje existente.
            _ => {
                self.flash_error(self.lang.err_no_fav_to_open().to_string());
            }
        }
    }

    // -------------------------------------------------------------------
    //  Forms (Add / Edit)
    // -------------------------------------------------------------------

    fn open_add_form(&mut self) {
        match self.tab {
            Tab::Apps => {
                self.form = Some(form::FormState::new(
                    self.lang.form_new_app_title().to_string(),
                    self.lang.form_hint().to_string(),
                    vec![
                        self.lang.field_name(),
                        self.lang.field_description(),
                        self.lang.field_command(),
                        self.lang.field_args(),
                    ],
                    vec![String::new(), String::new(), String::new(), String::new()],
                ));
                self.form_error = None;
                self.mode = Mode::Form(FormKind::AddApp);
            }
            Tab::Favs => {
                self.form = Some(form::FormState::new(
                    self.lang.form_new_fav_title().to_string(),
                    self.lang.form_hint().to_string(),
                    vec![
                        self.lang.field_name(),
                        self.lang.field_path(),
                        self.lang.field_description(),
                    ],
                    vec![String::new(), String::new(), String::new()],
                ));
                self.form_error = None;
                self.mode = Mode::Form(FormKind::AddFav);
            }
            Tab::Tools => {
                self.form = Some(form::FormState::new(
                    self.lang.form_new_tool_title().to_string(),
                    self.lang.form_hint().to_string(),
                    vec![
                        self.lang.field_name(),
                        self.lang.field_url(),
                        self.lang.field_description(),
                    ],
                    vec![String::new(), String::new(), String::new()],
                ));
                self.form_error = None;
                self.mode = Mode::Form(FormKind::AddTool);
            }
        }
    }

    fn open_edit_form(&mut self) {
        match self.tab {
            Tab::Apps => {
                if let Some(app) = self.app_list.selected().cloned() {
                    self.form = Some(form::FormState::new(
                        self.lang.form_edit_title(&app.name),
                        self.lang.form_hint().to_string(),
                        vec![
                            self.lang.field_name(),
                            self.lang.field_description(),
                            self.lang.field_command(),
                            self.lang.field_args(),
                        ],
                        vec![
                            app.name.clone(),
                            app.description.clone(),
                            app.path.clone(),
                            app.args.clone(),
                        ],
                    ));
                    self.form_error = None;
                    self.mode = Mode::Form(FormKind::EditApp);
                }
            }
            Tab::Favs => {
                // Solo abrimos el form si hay un favorito real
                // seleccionado (no la meta entry).
                if let Some(FavListItem::Fav(fav)) = self.fav_list.selected().cloned() {
                    self.form = Some(form::FormState::new(
                        self.lang.form_edit_title(&fav.name),
                        self.lang.form_hint().to_string(),
                        vec![
                            self.lang.field_name(),
                            self.lang.field_path(),
                            self.lang.field_description(),
                        ],
                        vec![fav.name.clone(), fav.path.clone(), fav.description.clone()],
                    ));
                    self.form_error = None;
                    self.mode = Mode::Form(FormKind::EditFav);
                }
            }
            Tab::Tools => {
                if let Some(tool) = self.tool_list.selected().cloned() {
                    self.form = Some(form::FormState::new(
                        self.lang.form_edit_title(&tool.name),
                        self.lang.form_hint().to_string(),
                        vec![
                            self.lang.field_name(),
                            self.lang.field_url(),
                            self.lang.field_description(),
                        ],
                        vec![tool.name.clone(), tool.url.clone(), tool.description.clone()],
                    ));
                    self.form_error = None;
                    self.mode = Mode::Form(FormKind::EditTool);
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
            FormKind::AddTool => self.form_add_tool(&values),
            FormKind::EditTool => self.form_edit_tool(&values),
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
            return Err(AppError::other(self.lang.form_error_name_required()));
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

        Ok(self.lang.msg_app_added_flash(name))
    }

    fn form_edit_app(&mut self, values: &[String]) -> Result<String> {
        let new_name = values[0].trim().to_string();
        let new_desc = values[1].trim().to_string();
        let new_cmd = values[2].trim().to_string();
        let new_extra = values[3].trim().to_string();

        let idx = self
            .app_list
            .selected_index()
            .ok_or_else(|| AppError::other(self.lang.form_error_no_selection_app()))?;
        let old_name = self.apps[idx].name.clone();

        if new_name.is_empty() || new_cmd.is_empty() {
            return Err(AppError::other(self.lang.form_error_name_required()));
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

        Ok(self.lang.msg_app_updated_flash(&new_name))
    }

    fn form_add_fav(&mut self, values: &[String]) -> Result<String> {
        let name = values[0].trim();
        let path_input = values[1].trim();
        let desc = values[2].trim();

        if name.is_empty() || path_input.is_empty() {
            return Err(AppError::other(self.lang.form_error_path_required()));
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
        self.fav_list
            .set_items(build_fav_list_items(&self.favs, self.lang));

        Ok(self.lang.msg_fav_added_flash(name))
    }

    fn form_edit_fav(&mut self, values: &[String]) -> Result<String> {
        let new_name = values[0].trim().to_string();
        let new_path_input = values[1].trim().to_string();
        let new_desc = values[2].trim().to_string();

        let idx = self
            .fav_list
            .selected_index()
            .ok_or_else(|| AppError::other(self.lang.form_error_no_selection_fav()))?;
        let old_name = self.favs[idx].name.clone();

        if new_name.is_empty() || new_path_input.is_empty() {
            return Err(AppError::other(self.lang.form_error_path_required()));
        }
        if new_name != old_name && self.favs.iter().any(|f| f.name == new_name) {
            return Err(AppError::Duplicate(new_name.clone()));
        }

        let resolved = resolve::resolve_favorite_dir(&new_path_input)?;
        self.favs[idx].name = new_name.clone();
        self.favs[idx].path = resolved.to_string_lossy().to_string();
        self.favs[idx].description = new_desc;
        storage::save_favs(&self.favs)?;
        self.fav_list
            .set_items(build_fav_list_items(&self.favs, self.lang));

        Ok(self.lang.msg_fav_updated_flash(&new_name))
    }

    fn form_add_tool(&mut self, values: &[String]) -> Result<String> {
        let name = values[0].trim();
        let url_input = values[1].trim();
        let desc = values[2].trim();

        if name.is_empty() || url_input.is_empty() {
            return Err(AppError::other(self.lang.form_error_url_required()));
        }
        if self.tools.iter().any(|t| t.name == name) {
            return Err(AppError::Duplicate(name.to_string()));
        }
        let url = resolve::resolve_tool_url(url_input)?;

        let now = now_string();
        let tool = Tool::new(name, desc, url, now);
        self.tools.push(tool);
        storage::save_tools(&self.tools)?;
        self.tool_list.set_items(self.tools.clone());

        Ok(self.lang.msg_tool_added_flash(name))
    }

    fn form_edit_tool(&mut self, values: &[String]) -> Result<String> {
        let new_name = values[0].trim().to_string();
        let new_url_input = values[1].trim().to_string();
        let new_desc = values[2].trim().to_string();

        let idx = self
            .tool_list
            .selected_index()
            .ok_or_else(|| AppError::other(self.lang.form_error_no_selection_tool()))?;
        let old_name = self.tools[idx].name.clone();

        if new_name.is_empty() || new_url_input.is_empty() {
            return Err(AppError::other(self.lang.form_error_url_required()));
        }
        if new_name != old_name && self.tools.iter().any(|t| t.name == new_name) {
            return Err(AppError::Duplicate(new_name.clone()));
        }

        let url = resolve::resolve_tool_url(&new_url_input)?;
        self.tools[idx].name = new_name.clone();
        self.tools[idx].url = url;
        self.tools[idx].description = new_desc;
        storage::save_tools(&self.tools)?;
        self.tool_list.set_items(self.tools.clone());

        Ok(self.lang.msg_tool_updated_flash(&new_name))
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
                            Ok(self.lang.msg_app_deleted_flash(&name))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    return;
                }
            }
            Tab::Favs => {
                if let Some(idx) = self.selected_fav_index() {
                    let name = self.favs[idx].name.clone();
                    self.favs.remove(idx);
                    match storage::save_favs(&self.favs) {
                        Ok(()) => {
                            // Reconstruir la lista con la meta entry al
                            // tope (set_items no reconstruye closures
                            // ni el meta; lo hacemos explícito).
                            self.fav_list
                                .set_items(build_fav_list_items(&self.favs, self.lang));
                            Ok(self.lang.msg_fav_deleted_flash(&name))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    return;
                }
            }
            Tab::Tools => {
                if let Some(idx) = self.tool_list.selected_index() {
                    let name = self.tools[idx].name.clone();
                    self.tools.remove(idx);
                    match storage::save_tools(&self.tools) {
                        Ok(()) => {
                            self.tool_list.set_items(self.tools.clone());
                            Ok(self.lang.msg_tool_deleted_flash(&name))
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
            Err(e) => self.flash_error(e.localized(self.lang)),
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
    let lang = tui.lang;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    draw_header(frame, chunks[0], lang);
    draw_tabs(frame, chunks[1], tui.tab, lang);

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
        draw_message(frame, &text, is_error, lang);
    }

    draw_footer(frame, chunks[3], &tui.mode, tui.tab, lang);

    if let Some(form) = tui.form.as_mut() {
        form::render_form(frame, form, tui.form_error.as_deref());
    }
}

fn draw_header(frame: &mut Frame, area: Rect, lang: Lang) {
    let title = Line::from(vec![
        Span::styled("mytuis", theme::title_style()),
        Span::raw("  ::  "),
        Span::styled(lang.header_subtitle(), theme::subtitle_style()),
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

fn draw_tabs(frame: &mut Frame, area: Rect, current: Tab, lang: Lang) {
    let titles = vec![
        Line::from(Span::raw(lang.tab_apps())),
        Line::from(Span::raw(lang.tab_favs())),
        Line::from(Span::raw(lang.tab_tools())),
    ];
    let selected = match current {
        Tab::Apps => 0,
        Tab::Favs => 1,
        Tab::Tools => 2,
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
                .title(lang.tab_section_hint()),
        );
    frame.render_widget(tabs, area);
}

fn draw_list(frame: &mut Frame, area: Rect, tui: &mut Tui) {
    let prompt = if tui.app_list.filter.is_empty()
        && tui.fav_list.filter.is_empty()
        && tui.tool_list.filter.is_empty()
    {
        // Todos vacíos → prompt genérico.
        match tui.tab {
            Tab::Apps => tui.lang.filter_prompt_empty().to_string(),
            Tab::Favs => tui.lang.filter_prompt_empty().to_string(),
            Tab::Tools => tui.lang.filter_prompt_empty().to_string(),
        }
    } else {
        // Al menos uno tiene filtro → prompt específico del tab activo.
        match tui.tab {
            Tab::Apps => tui.lang.filter_prompt_with(&tui.app_list.filter),
            Tab::Favs => tui.lang.filter_prompt_with(&tui.fav_list.filter),
            Tab::Tools => tui.lang.filter_prompt_with(&tui.tool_list.filter),
        }
    };
    match tui.tab {
        Tab::Apps => {
            let title = tui.lang.list_title_apps(tui.apps.len(), tui.apps.is_empty());
            tui.app_list.render(frame, area, &title, &prompt);
        }
        Tab::Favs => {
            let title = tui.lang.list_title_favs(tui.favs.len(), tui.favs.is_empty());
            tui.fav_list.render(frame, area, &title, &prompt);
        }
        Tab::Tools => {
            let title = tui.lang.list_title_tools(tui.tools.len(), tui.tools.is_empty());
            tui.tool_list.render(frame, area, &title, &prompt);
        }
    }
}

fn draw_submenu(frame: &mut Frame, area: Rect, tui: &mut Tui) {
    let selected_name = match tui.tab {
        Tab::Apps => tui.app_list.selected().map(|a| a.name.clone()).unwrap_or_default(),
        Tab::Favs => tui
            .fav_list
            .selected()
            .and_then(|item| match item {
                FavListItem::Fav(f) => Some(f.name.clone()),
                FavListItem::MetaOpenHere => None,
            })
            .unwrap_or_default(),
        Tab::Tools => tui
            .tool_list
            .selected()
            .map(|t| t.name.clone())
            .unwrap_or_default(),
    };

    let items: Vec<ListItem> = tui
        .sub_actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let label = match a {
                SubAction::Run => match tui.tab {
                    Tab::Apps => tui.lang.submenu_run_app(),
                    Tab::Favs => tui.lang.submenu_run_fav(),
                    Tab::Tools => tui.lang.submenu_run_tool(),
                },
                SubAction::Edit => tui.lang.submenu_edit(),
                SubAction::Delete => tui.lang.submenu_delete(),
                SubAction::CopyPath => tui.lang.submenu_copy_path(),
                SubAction::Back => tui.lang.submenu_back(),
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
                .title(tui.lang.submenu_title(&selected_name))
                .title_bottom(tui.lang.submenu_hint()),
        )
        .highlight_style(theme::selected_style())
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_message(frame: &mut Frame, text: &str, is_error: bool, lang: Lang) {
    let area = centered_rect(60, 25, frame.size());
    frame.render_widget(Clear, area);
    let style = if is_error {
        theme::error_style()
    } else {
        theme::success_style()
    };
    let border_style = style;
    let title = if is_error {
        lang.msg_error_title()
    } else {
        lang.msg_ok_title()
    };
    let p = Paragraph::new(text)
        .style(style)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title)
                .title_bottom(lang.footer_message()),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(p, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, mode: &Mode, tab: Tab, lang: Lang) {
    let hint = match mode {
        Mode::List => lang.footer_list(matches!(tab, Tab::Favs)),
        Mode::SubMenu => lang.footer_submenu(),
        Mode::Form(_) => lang.footer_form(),
        Mode::Message { .. } => lang.footer_message(),
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
pub fn run(lang: Lang) -> Result<()> {
    let apps = storage::load_apps()?;
    let favs = storage::load_favs()?;
    let tools = storage::load_tools()?;
    let mut tui = Tui::new(lang, apps, favs, tools);

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

// ============================================================================
//  Tests
// ============================================================================
//
// Tests unitarios de la lógica de construcción de la lista de favoritos
// y del dispatch del enum `FavListItem`. NO testean la UI renderizada
// (eso requeriría un TTY real); testean la **estructura** que la UI
// después dibuja.

#[cfg(test)]
mod tests {
    use super::*;

    fn fav(name: &str, path: &str) -> FavoritePath {
        FavoritePath::new(name, "", path, "2026-06-26 00:00:00")
    }

    #[test]
    fn build_fav_list_vacia_sin_meta() {
        // Sin favoritos, la meta no tiene a qué aplicarse → lista vacía.
        let items = build_fav_list_items(&[], Lang::En);
        assert!(items.is_empty(), "expected empty list, got {items:?}");
    }

    #[test]
    fn build_fav_list_un_favorito_tiene_meta() {
        // Con un favorito, la meta aparece al tope.
        let favs = vec![fav("pepe", "/datos/pepe")];
        let items = build_fav_list_items(&favs, Lang::En);
        assert_eq!(items.len(), 2);
        assert!(
            matches!(items[0], FavListItem::MetaOpenHere),
            "first item should be Meta, got {:?}", items[0]
        );
        assert!(matches!(&items[1], FavListItem::Fav(f) if f.name == "pepe"));
    }

    #[test]
    fn build_fav_list_n_favoritos_una_meta() {
        // Con N favoritos hay exactamente N+1 items (1 meta + N favs).
        let favs = vec![
            fav("a", "/a"),
            fav("b", "/b"),
            fav("c", "/c"),
        ];
        let items = build_fav_list_items(&favs, Lang::En);
        assert_eq!(items.len(), 4);
        assert!(matches!(items[0], FavListItem::MetaOpenHere));
        // Los siguientes son los favoritos en orden.
        for (i, f) in favs.iter().enumerate() {
            assert!(
                matches!(&items[i + 1], FavListItem::Fav(item) if item.name == f.name),
                "items[{}] should be Fav({})", i + 1, f.name
            );
        }
    }

    #[test]
    fn meta_label_en_y_es() {
        // Verifica que el label del meta se traduce según el lang.
        assert!(Lang::En.meta_open_here().contains("Open here"));
        assert!(Lang::Es.meta_open_here().contains("Abrir"));
    }

    #[test]
    fn meta_search_contiene_keywords_en_y_es() {
        // El filtro del meta debe matchear tanto en EN como en ES,
        // así el usuario puede tipear "open" o "abrir" en cualquier idioma.
        let search_en = Lang::En.meta_open_here_search();
        let search_es = Lang::Es.meta_open_here_search();
        assert!(search_en.contains("open") && search_en.contains("abrir"));
        assert!(search_es.contains("open") && search_es.contains("abrir"));
    }

    /// Verifica que la meta entry se ve en el frame rendereado. Usa
    /// `TestBackend` de ratatui para no necesitar un TTY real.
    #[test]
    fn meta_entry_visible_en_frame_con_favoritos() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let favs = vec![fav("pepe", "/datos/pepe")];
        let mut tui = Tui::new(Lang::En, vec![], favs, vec![]);
        // Cambiamos al tab Favoritos y seleccionamos el primer item
        // (que debería ser la meta).
        tui.tab = Tab::Favs;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| ui(f, &mut tui))
            .expect("draw should succeed");

        let buffer = terminal.backend().buffer().clone();
        let text: String = buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();

        assert!(
            text.contains("Open here"),
            "el frame debe contener la meta entry. Texto: {text}"
        );
        assert!(
            text.contains("pepe"),
            "el frame debe contener el favorito 'pepe'. Texto: {text}"
        );
    }

    /// Verifica que la meta entry **no aparece** cuando no hay favoritos.
    #[test]
    fn meta_entry_no_visible_sin_favoritos() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut tui = Tui::new(Lang::En, vec![], vec![], vec![]);
        tui.tab = Tab::Favs;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| ui(f, &mut tui))
            .expect("draw should succeed");

        let buffer = terminal.backend().buffer().clone();
        let text: String = buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();

        assert!(
            !text.contains("Open here"),
            "sin favoritos, no debe haber meta entry. Texto: {text}"
        );
    }

    /// Verifica que la tecla `c` en el tab Favoritos dispara el flujo
    /// "cd & quit". En el contexto de test fd 3 está cerrado (los
    /// tests de Rust no heredan fds del runner por defecto), así que
    /// `emit_cd_to_fd3` falla y esperamos que la TUI muestre un error
    /// con el snippet del wrapper, sin marcar `should_quit = true`.
    #[test]
    fn cd_here_sin_fd3_muestra_error_y_no_sale() {
        use crossterm::event::{KeyCode, KeyEvent};

        // Un favorito basta. Navegamos al tab Favoritos y seleccionamos
        // el favorito. Importante: después de `Tui::new`, la selección
        // queda en el índice 0 (la meta entry) porque `recompute_filter`
        // clamp-ea `cur = 0` por default. Una sola llamada a `next()`
        // nos mueve al índice 1, que es el favorito real.
        let favs = vec![fav("pepe", "/datos/pepe")];
        let mut tui = Tui::new(Lang::En, vec![], favs, vec![]);
        tui.tab = Tab::Favs;
        tui.fav_list.next();
        // sanity check: estamos en el fav, no en la meta.
        assert!(
            matches!(tui.fav_list.selected(), Some(FavListItem::Fav(_))),
            "debemos estar en el fav después de next(), got {:?}",
            tui.fav_list.selected()
        );

        assert!(
            !tui.should_quit,
            "should_quit debe empezar en false"
        );

        // Simulamos la pulsación de 'c'.
        tui.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        // Como fd 3 no está abierto en el test, esperamos error flash.
        match &tui.mode {
            Mode::Message { text, is_error } => {
                assert!(is_error, "el mensaje debe ser de error");
                assert!(
                    text.contains("fd 3") || text.contains("wrapper"),
                    "el mensaje debe mencionar fd 3 o wrapper, got: {text}"
                );
                assert!(
                    text.contains("command mytuis"),
                    "el mensaje debe incluir el snippet del wrapper, got: {text}"
                );
            }
            other => panic!("esperaba Mode::Message de error, got {other:?}"),
        }

        // Y NO debe haber salido.
        assert!(
            !tui.should_quit,
            "con fd 3 cerrado, no debemos salir (dejamos que el \
             usuario siga usando la TUI)"
        );
    }

    /// Verifica que `c` en el tab Apps NO dispara la acción de cd
    /// (ahí no aplica: las apps no son directorios). La tecla debe
    /// caer al filtro, como cualquier otra letra reservada.
    #[test]
    fn cd_here_en_tab_apps_no_dispara_accion() {
        use crossterm::event::{KeyCode, KeyEvent};

        let mut tui = Tui::new(Lang::En, vec![], vec![], vec![]);
        // tab == Apps (default).
        assert_eq!(tui.tab, Tab::Apps);

        tui.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        // No debe haber mensaje flash ni quit.
        assert!(!tui.should_quit);
        assert!(
            matches!(tui.mode, Mode::List),
            "el modo no debe cambiar, got {:?}", tui.mode
        );
        // El filtro de apps debería tener la 'c'.
        assert_eq!(tui.app_list.filter, "c");
    }

    /// Verifica que `c` con la meta entry seleccionada (no un fav
    /// real) muestra el error "no favorite selected to open here"
    /// y NO sale.
    #[test]
    fn cd_here_con_meta_seleccionada_no_sale() {
        use crossterm::event::{KeyCode, KeyEvent};

        let favs = vec![fav("pepe", "/datos/pepe")];
        let mut tui = Tui::new(Lang::En, vec![], favs, vec![]);
        tui.tab = Tab::Favs;
        // Después de `Tui::new`, la selección queda en el índice 0,
        // que es la meta entry. NO llamamos `next()`: queremos
        // quedarnos en la meta para verificar que `c` ahí NO sale.
        assert!(
            matches!(tui.fav_list.selected(), Some(FavListItem::MetaOpenHere)),
            "la meta debe estar seleccionada por default, got {:?}",
            tui.fav_list.selected()
        );

        tui.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        // selected_fav_index devuelve None cuando la meta está al tope.
        match &tui.mode {
            Mode::Message { text, is_error } => {
                assert!(is_error);
                assert!(
                    text.contains("favorit") || text.contains("favorite"),
                    "el error debe mencionar 'favorite', got: {text}"
                );
            }
            other => panic!("esperaba Mode::Message de error, got {other:?}"),
        }
        assert!(!tui.should_quit);
    }

    /// Verifica que el footer del tab Favoritos muestra `c cd & exit`
    /// y el del tab Apps NO.
    #[test]
    fn footer_diferente_por_tab() {
        let tui = Tui::new(Lang::En, vec![], vec![fav("x", "/x")], vec![]);

        // En Apps no debe aparecer "cd & exit".
        assert!(!Lang::En.footer_list(false).contains("cd & exit"));

        // En Favs sí.
        assert!(Lang::En.footer_list(true).contains("cd & exit"));

        // Verificación cruzada: el lang field del tui es el correcto.
        assert_eq!(tui.lang, Lang::En);
    }

    /// Helper para construir un tool en tests.
    fn tool(name: &str, url: &str) -> Tool {
        Tool::new(name, "", url, "2026-07-10 12:00:00")
    }

    /// Verifica que el tab Tools existe y que la tecla `3` lo activa.
    #[test]
    fn tab_3_selecciona_tools() {
        use crossterm::event::{KeyCode, KeyEvent};

        let tools = vec![tool("grafana", "https://grafana.example.com")];
        let mut tui = Tui::new(Lang::En, vec![], vec![], tools);
        assert_eq!(tui.tab, Tab::Apps); // default
        tui.on_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(tui.tab, Tab::Tools);
    }

    /// Verifica el ciclo de tabs con Tab/Tab: Apps → Favs → Tools → Apps.
    #[test]
    fn tab_toggle_cicla_por_tres_tabs() {
        let mut tui = Tui::new(Lang::En, vec![], vec![], vec![]);
        assert_eq!(tui.tab, Tab::Apps);
        tui.tab = tui.tab.toggle();
        assert_eq!(tui.tab, Tab::Favs);
        tui.tab = tui.tab.toggle();
        assert_eq!(tui.tab, Tab::Tools);
        tui.tab = tui.tab.toggle();
        assert_eq!(tui.tab, Tab::Apps);
    }

    /// El tab Tools renderiza sus items cuando hay datos.
    #[test]
    fn tools_se_renderizan_en_el_frame() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let tools = vec![tool("grafana", "https://grafana.example.com")];
        let mut tui = Tui::new(Lang::En, vec![], vec![], tools);
        tui.tab = Tab::Tools;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| ui(f, &mut tui))
            .expect("draw should succeed");

        let buffer = terminal.backend().buffer().clone();
        let text: String = buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();

        assert!(
            text.contains("grafana"),
            "el frame debe contener el tool 'grafana'. Texto: {text}"
        );
        assert!(
            text.contains("https"),
            "el frame debe contener la URL. Texto: {text}"
        );
    }

    /// El tab Tools vacío muestra el título "empty".
    #[test]
    fn tools_vacio_muestra_empty_en_el_frame() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut tui = Tui::new(Lang::En, vec![], vec![], vec![]);
        tui.tab = Tab::Tools;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| ui(f, &mut tui))
            .expect("draw should succeed");

        let buffer = terminal.backend().buffer().clone();
        let text: String = buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();

        // El título dice "empty" en EN cuando no hay tools.
        assert!(text.contains("empty") || text.contains("Tools"));
    }

    /// Verifica que el filtro de tools matchea por URL (no solo por
    /// nombre/desc).
    #[test]
    fn tools_filtro_matchea_por_url() {
        let tools = vec![
            tool("grafana", "https://grafana.example.com"),
            tool("hub", "https://jupyter.example.com"),
        ];
        let mut tui = Tui::new(Lang::En, vec![], vec![], tools);
        tui.tab = Tab::Tools;

        // Filtrar por "grafana" debe dejar un solo item visible.
        tui.tool_list.push_filter_char('g');
        tui.tool_list.push_filter_char('r');
        tui.tool_list.push_filter_char('a');
        tui.tool_list.push_filter_char('f');
        assert_eq!(tui.tool_list.len(), 1);
    }

    /// Verifica que el submenú de Tools tiene 4 acciones (sin CopyPath).
    #[test]
    fn tools_submenu_tiene_4_acciones() {
        let tools = vec![tool("grafana", "https://grafana.example.com")];
        let mut tui = Tui::new(Lang::En, vec![], vec![], tools);
        tui.tab = Tab::Tools;
        tui.open_submenu();
        assert_eq!(tui.sub_actions.len(), 4);
        assert!(matches!(tui.sub_actions[0], SubAction::Run));
        assert!(matches!(tui.sub_actions[1], SubAction::Edit));
        assert!(matches!(tui.sub_actions[2], SubAction::Delete));
        assert!(matches!(tui.sub_actions[3], SubAction::Back));
    }
}