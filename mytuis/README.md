# mytuis (Rust)

Gestor de aplicaciones, rutas favoritas y tools remotos con TUI
(basada en [`ratatui`](https://ratatui.rs)) y CLI. Reimplementación
en Rust de [`mytuis.sh`](../README.md) con dos entidades nuevas:
**rutas favoritas** y **tools** (URLs que se abren en el navegador).

## Qué hay en este directorio

```
mytuis/
├── Cargo.toml          ← dependencias (ratatui, clap, serde, arboard, ...)
├── src/
│   ├── main.rs         ← entrypoint + dispatchers CLI
│   ├── cli.rs          ← definición clap de subcomandos
│   ├── config.rs       ← rutas de los YAML (~/.mytuis/)
│   ├── model.rs        ← structs App, FavoritePath y Tool
│   ├── resolve.rs      ← resolución de comandos, directorios y URLs
│   ├── storage.rs      ← load/save YAML atómico + migración bash
│   ├── open.rs         ← detección de terminal + opener de URLs + clipboard
│   ├── lang.rs         ← internacionalización (EN/ES)
│   ├── error.rs        ← AppError (thiserror) + localized()
│   └── tui/
│       ├── mod.rs      ← bootstrap + state machine + event loop
│       ├── theme.rs    ← paleta de colores (212/39/82/196/214/240/255)
│       ├── list_view.rs← lista filtrable reutilizable
│       └── form.rs     ← forms modales add/edit
└── AGENTS.md           ← notas internas del proyecto
```

## Compilar e instalar

```bash
# Build release (un solo binario, ~2.6 MB)
cargo build --release

# El binario queda en target/release/mytuis. Copialo a tu $PATH:
install -m 755 target/release/mytuis /usr/local/bin/

# O simplemente corrélo desde acá:
./target/release/mytuis
```

## Uso rápido

### TUI

```bash
mytuis                  # abre la TUI (tab Apps por defecto)
```

Una sola pantalla con tres tabs arriba: **Apps**, **Favoritos** y
**Tools**. Tab / `←→` cambia de tab, `1`/`2`/`3` van directo a uno,
las flechas o `j`/`k` navegan, typing filtra, Enter abre el submenú
del item seleccionado, `a`/`e`/`d`/`r` agregan/editan/borran/ejecutan,
`q` sale.

En el tab Favoritos la acción **"abrir terminal aquí"** lanza una
terminal nueva con `cwd` = el directorio del favorito. El submenú de
favoritos además tiene **"Copiar path al portapapeles"**.

En el tab Tools la acción **"Run"** abre la URL con el opener del
sistema (`xdg-open` / `gio open` / `open` en macOS). No hay meta entry:
los tools se abren directo.

### CLI

```bash
# Apps
mytuis apps list
mytuis apps add nvim "Editor modal" nvim
mytuis apps add lsl "Listado largo" "ls -lad"
mytuis apps remove nvim           # confirma en TTY
mytuis apps remove nvim --yes     # sin confirmación

# Favoritos (rutas a directorios)
mytuis paths list
mytuis paths add pepe /datos/pepe -d "Repo principal"
mytuis paths add docs ~/Documents -d "Documentos"
mytuis paths get pepe             # → /datos/pepe (para `cd` en shell)
mytuis paths go pepe              # abre terminal en /datos/pepe y sale
mytuis paths remove pepe

# Tools (aplicaciones remotas / URLs)
mytuis tools list
mytuis tools add grafana "Monitoring" https://grafana.example.com
mytuis tools add hub "Jupyter" https://jupyter.example.com
mytuis tools run grafana          # abre la URL en el navegador
mytuis tools remove grafana -y

# Aliases de compatibilidad con la versión bash
mytuis list                       # ≡ mytuis apps list
mytuis add nvim "Editor" nvim     # ≡ mytuis apps add ...
mytuis remove nvim                # ≡ mytuis apps remove ...
```

### Shell integration (recomendado)

Pegá esto en tu `.bashrc` / `.zshrc`:

```bash
# cd a un favorito por nombre
cdfav() {
    local dir
    dir=$(mytuis paths get "$1" 2>/dev/null) && cd "$dir" \
        || echo "mytuis: favorito '$1' no encontrado"
}

# Abrir un favorito en nueva terminal
gocd() {
    mytuis paths go "$1"
}
```

Uso: `cdfav pepe` → te lleva a `/datos/pepe` (en tu terminal actual).
`gocd pepe` → abre una terminal nueva en `/datos/pepe` y mytuis sale solo.

### Meta entry `[↵] Open here` en la TUI

En el tab **Favoritos**, al tope de la lista aparece una meta entry:

```
┌─ Favoritos (3) ──────────────────────────────────┐
│ ▶ [↵] Open here                                  │  ← meta entry
│   pepe       — Repo principal                     │
│   docs       — Documentos                         │
│   ...                                            │
└──────────────────────────────────────────────────┘
```

Seleccioná la meta y apretá `Enter` (o presioná `g` desde cualquier
favorito) para abrir una terminal en el favorito y salir de mytuis. La
acción es la misma que `mytuis paths go <name>` pero desde la TUI.

Si no hay favoritos, la meta entry no se muestra (no tendría sentido).

## Localization (EN / ES)

Todos los mensajes user-facing (CLI y TUI) están traducidos a
**inglés** y **español**. El idioma se detecta automáticamente con la
jerarquía estándar de Unix:

1. `$MYTUIS_LANG` — override explícito del usuario.
2. `$LC_ALL` — variable estándar POSIX de mayor prioridad.
3. `$LC_MESSAGES` — específica para mensajes.
4. `$LANG` — la más común.
5. Default: **English**.

Los valores se parsean con el formato POSIX: `en`, `en_US`, `en_US.UTF-8`,
`es_AR`, `es_ES@euro`, etc. Solo importa el prefijo de dos letras.
Cualquier idioma no soportado cae a English.

```bash
# Forzar español:
LANG=es_AR.UTF-8 mytuis apps list
LC_ALL=es_ES.UTF-8 mytuis apps list
MYTUIS_LANG=es mytuis apps list

# Forzar inglés:
LANG=C mytuis apps list
MYTUIS_LANG=en mytuis apps list

# Default (si no hay variables): English
mytuis apps list
```

**Lo que se traduce:** mensajes de error, mensajes de éxito, headers
de tablas, prompts de confirmación, el texto completo de la TUI
(header, tabs, submenús, formularios, mensajes flash, footer).

**Lo que NO se traduce:**
- `clap --help` (clap no soporta i18n nativo).
- Nombres de subcomandos (`apps`, `paths`, `tools`, `list`, `add`,
  `remove`, `get`, `run`).
- Nombres de campos YAML (`name`, `description`, `path`, `url`).
- Comentarios del código fuente (son para devs).

Para agregar un idioma nuevo, ver [AGENTS.md](AGENTS.md#internacionalización).

## Storage

La versión Rust guarda los datos en un **directorio** en vez de un
solo archivo:

```
~/.mytuis/
├── apps.yaml      ← apps (mismo formato que la versión bash)
├── favs.yaml      ← favoritos
└── tools.yaml     ← tools (aplicaciones remotas / URLs)
```

### Migración automática desde `mytuis.sh`

Si tenés un `~/.mytuis.yaml` de la versión bash, **al primer
arranque** se importa automáticamente a `~/.mytuis/apps.yaml` y el
archivo original se renombra a `~/.mytuis.yaml.bak` para no perder
datos. Vas a ver un mensaje al stderr tipo:

```
mytuis: migradas 3 app(s) desde ~/.mytuis.yaml (backup en ~/.mytuis.yaml.bak)
```

## Formato de los archivos

`apps.yaml`:

```yaml
apps:
  - name: 'nvim'
    description: 'Editor modal'
    path: '/usr/bin/nvim'
    args: '-p'                 # opcional
    created: '2026-06-26 10:00:00'
    last_used: '2026-06-26 12:00:00'   # opcional
```

`favs.yaml`:

```yaml
favorites:
  - name: 'pepe'
    description: 'Repo principal'
    path: '/datos/pepe'
    created: '2026-06-26 11:00:00'
    last_used: '2026-06-26 12:30:00'   # opcional
```

`tools.yaml`:

```yaml
tools:
  - name: 'grafana'
    description: 'Monitoring dashboard'   # opcional
    url: 'https://grafana.example.com'
    created: '2026-07-10 12:00:00'
    last_used: '2026-07-10 12:30:00'    # opcional
```

Nota: para tools, `last_used` arranca igual que `created` cuando se
guarda por primera vez (el tool "nació" en ese momento y ese fue su
primer uso).

Los campos opcionales se omiten del YAML cuando están vacíos (igual
que la versión bash, para mantener el archivo compacto).

## Features

- **TUI con tres tabs** (Apps, Favoritos y Tools), filtrable.
- **Formularios modales** para add/edit de las tres entidades.
- **Validación de URLs** al guardar un tool (solo http/https + host
  no vacío).
- **Opener de URLs** del sistema: prueba `xdg-open`, `gio open`,
  `open` (macOS) en ese orden.
- **Migración transparente** desde `mytuis.sh`.
- **Atomic writes**: los YAML se escriben primero a `.tmp` y después
  se hace `rename`, así un corte de luz a mitad de guardado no rompe
  el archivo.
- **Resolución de paths**: acepta `firefox` (busca en `$PATH`),
  `/usr/bin/firefox`, `~/bin/foo`, `./scripts/myscript.sh`.
- **Clipboard** vía `arboard` (X11/Wayland en Linux, NSPasteboard en
  macOS, OLE en Windows).
- **Detección de terminal** inteligente: respeta `$TERMINAL` y, si no,
  prueba `gnome-terminal`, `konsole`, `xfce4-terminal`, `alacritty`,
  `kitty`, `foot`, `wezterm`, `xterm` en ese orden.
- **Tests unitarios** con `cargo test` (61 tests sobre model/storage/
  resolve/open/lang/config/TUI).

## Dependencias clave

| Crate | Versión | Para qué |
|-------|---------|----------|
| `ratatui` | 0.29 | TUI widgets + rendering |
| `crossterm` | 0.28 | Backend de terminal (raw mode, input) |
| `clap` | 4 (derive) | Parsing de CLI |
| `serde` + `serde_yaml` | 1 / 0.9 | (De)serialización YAML |
| `chrono` | 0.4 | Timestamps `created` / `last_used` |
| `arboard` | 3 | Portapapeles del sistema |
| `which` | 6 | Buscar ejecutables en `$PATH` |
| `dirs` | 5 | Localizar `$HOME` |
| `thiserror` + `anyhow` | 1 | Manejo de errores |

## Licencia

MIT. Mismo proyecto que `mytuis.sh`.