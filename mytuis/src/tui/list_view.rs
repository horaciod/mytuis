//! # Widget de lista filtrable (reutilizable)
//!
//! Tanto el tab de apps como el de favoritos necesitan mostrar una
//! lista filtrable con las mismas reglas:
//!
//! - El usuario navega con ↑/↓ y con j/k (al estilo vim).
//! - Puede tipear para filtrar; el filtro es **case-insensitive** y
//!   matchea contra el nombre y la descripción.
//! - Enter selecciona el item actual.
//! - Esc / q limpia el filtro o vuelve.
//!
//! Para no duplicar lógica entre `apps_tab` y `favs_tab`, definimos
//! acá un `ListView<I>` genérico sobre el tipo de item, donde el
//! caller nos dice cómo "renderizar" cada item (qué texto mostrar)
//! y cómo extraer el "texto de búsqueda" para el filtro.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

/// `ListView` mantiene el estado de una lista filtrable.
///
/// ## Parámetros de genericidad
///
/// Rust usa genericidad con trait bounds. Acá pedimos:
///
/// - `I`: el tipo de item (`App`, `FavoritePath`, lo que sea).
/// - `F`: un closure `Fn(&I) -> String` que extrae el texto a mostrar.
/// - `G`: un closure `Fn(&I) -> String` que extrae el texto contra el
///        cual se filtra (nombre + descripción, por ejemplo).
///
/// Almacenamos los closures como `Box<dyn Fn>` porque las funciones
/// genéricas no pueden guardar genéricos en una struct. Esto tiene un
/// costo de performance mínimo (una indirección por call) pero hace
/// la API mucho más cómoda.
pub struct ListView<I> {
    /// Todos los items, sin filtrar.
    pub all: Vec<I>,
    /// Items que pasan el filtro actual (índices a `all`).
    pub filtered: Vec<usize>,
    /// Filtro actual (lo que el usuario tipeó).
    pub filter: String,
    /// Estado de selección que ratatui necesita para dibujar el
    /// highlight y scrollear.
    pub state: ListState,
    /// Closure que produce el texto a mostrar para cada item.
    render: Box<dyn Fn(&I) -> String>,
    /// Closure que produce el texto contra el que matchea el filtro.
    search: Box<dyn Fn(&I) -> String>,
}

impl<I> ListView<I> {
    /// Construye un `ListView` nuevo.
    ///
    /// Ejemplo:
    /// ```
    /// ListView::new(
    ///     apps,
    ///     |a| format!("{} — {}", a.name, a.description),
    ///     |a| format!("{} {}", a.name, a.description),
    /// )
    /// ```
    pub fn new<F, G>(items: Vec<I>, render: F, search: G) -> Self
    where
        F: Fn(&I) -> String + 'static,
        G: Fn(&I) -> String + 'static,
    {
        let mut view = Self {
            all: items,
            filtered: Vec::new(),
            filter: String::new(),
            state: ListState::default(),
            render: Box::new(render),
            search: Box::new(search),
        };
        view.recompute_filter();
        view
    }

    /// Reemplaza la lista de items (después de un add/edit/delete).
    pub fn set_items(&mut self, items: Vec<I>) {
        self.all = items;
        self.recompute_filter();
    }

    /// Devuelve la cantidad de items visibles (post-filtro).
    pub fn len(&self) -> usize {
        self.filtered.len()
    }

    /// `true` si no hay items visibles.
    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    /// Devuelve el item actualmente seleccionado (si hay alguno).
    pub fn selected(&self) -> Option<&I> {
        let idx = self.state.selected()?;
        let real_idx = *self.filtered.get(idx)?;
        self.all.get(real_idx)
    }

    /// Devuelve el índice dentro de `all` del item seleccionado.
    pub fn selected_index(&self) -> Option<usize> {
        let idx = self.state.selected()?;
        self.filtered.get(idx).copied()
    }

    /// Mueve la selección hacia abajo, con wrap-around.
    pub fn next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => (i + 1) % self.filtered.len(),
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Mueve la selección hacia arriba, con wrap-around.
    pub fn previous(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.filtered.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Aplica un caracter al filtro actual y re-filtra.
    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.recompute_filter();
    }

    /// Borra el último caracter del filtro.
    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.recompute_filter();
    }

    /// Limpia el filtro completo.
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.recompute_filter();
    }

    /// Recalcula `filtered` a partir del filtro y deja la selección en
    /// un lugar coherente (clamp al nuevo rango).
    fn recompute_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        self.filtered = self
            .all
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if needle.is_empty() {
                    return true;
                }
                let haystack = (self.search)(item).to_lowercase();
                haystack.contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();

        // Ajustar selección: clamp al nuevo rango.
        if self.filtered.is_empty() {
            self.state.select(None);
        } else {
            let cur = self.state.selected().unwrap_or(0);
            let new = cur.min(self.filtered.len() - 1);
            self.state.select(Some(new));
        }
    }

    /// Dibuja la lista en el área dada, con el título `title`.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, title: &str) {
        // Convertimos cada item visible a un `ListItem` (texto con
        // estilo). El highlight lo maneja ratatui vía `ListState`.
        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .map(|&i| {
                let text = (self.render)(&self.all[i]);
                ListItem::new(text)
            })
            .collect();

        let prompt = if self.filter.is_empty() {
            "Tipea para filtrar".to_string()
        } else {
            format!("Filtro: {}", self.filter)
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {title} "))
                    .title_bottom(format!(" {prompt} ")),
            )
            .highlight_style(super::theme::selected_style())
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}