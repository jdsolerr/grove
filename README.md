<div align="center">

<img src="assets/grove-logo.png" alt="Grove" width="160" />

# Grove

**Un cliente Git de escritorio, rápido y ligero, hecho en Rust.**<br/>
Cambios, historial, ramas, stash, Pull Requests y resolución de conflictos en una sola ventana,<br/>
con tus repositorios organizados en grupos y mensajes de commit asistidos por IA (opcional).

[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-edición%202024-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![SolidJS](https://img.shields.io/badge/SolidJS-1.9-2C4F7C?logo=solid&logoColor=white)](https://www.solidjs.com)
[![Git](https://img.shields.io/badge/Git-%E2%89%A5%202.40-F05032?logo=git&logoColor=white)](https://git-scm.com)
[![Licencia](https://img.shields.io/badge/licencia-MIT-1c9a5b)](#licencia)

</div>

---

## Contenido

- [¿Por qué Grove?](#por-qué-grove)
- [Funciones](#funciones)
- [Primeros pasos](#primeros-pasos)
- [Mensajes de commit con IA](#mensajes-de-commit-con-ia)
- [Cómo está hecho](#cómo-está-hecho)
- [Privacidad y seguridad](#privacidad-y-seguridad)
- [Desarrollo](#desarrollo)
- [Estado del proyecto y hoja de ruta](#estado-del-proyecto-y-hoja-de-ruta)
- [Licencia y agradecimientos](#licencia-y-agradecimientos)

---

## ¿Por qué Grove?

- **Ligero.** Backend 100 % Rust sobre [Tauri 2](https://tauri.app): usa el webview del sistema en lugar de empaquetar un navegador completo.
- **Git de verdad.** Lecturas rápidas con [`gix`](https://github.com/GitoxideLabs/gitoxide) y escrituras/red con tu binario `git`. Por eso respetan tus *hooks*, la firma de commits, tus *credential helpers* y tu configuración, sin reimplementar nada.
- **Organizado.** Agrupa repositorios por cliente, equipo o proyecto (arrastrando o desde el menú) y ponles alias.
- **Familiar.** La disposición (barra superior, lista a la izquierda, detalle a la derecha) resulta natural si vienes de otros clientes Git de escritorio, con identidad visual propia en tema claro y oscuro.
- **En español.** Interfaz y mensajes en español.

---

## Funciones

### Repositorios y grupos
- **Paleta de comandos** (⌘/Ctrl+K): salta a cualquier repo o rama y lanza acciones escribiendo unas letras; sin texto muestra primero los repos con novedades.
- Añadir repositorios existentes, **clonar** (URL o lista de tus repos de GitHub, con progreso) y **publicar** un repo local en GitHub (público o privado).
- **Grupos** creables de antemano, aunque estén vacíos; mover repos arrastrándolos sobre el encabezado de un grupo, con el menú de clic derecho, o con el botón **+** de cada grupo.
- **Alias** por repositorio. Por defecto se muestra el nombre del repo en GitHub (según `origin`), no el de la carpeta.
- **Actualizar un grupo entero** (botón en su encabezado): fetch en paralelo y avance rápido seguro, sin fusiones ni conflictos. Filtro **Novedades** para ver solo repos con algo pendiente.
- **Fetch automático** en segundo plano al abrir la app, al volver a la ventana o al cambiar de repo (solo los que llevan más de 5 min sin actualizar) y cada 10 minutos, para que «Pull origin» aparezca solo.
- Lista siempre en **orden alfabético**, con indicadores como en los clientes de escritorio: punto = cambios sin commitear, **↑N** = commits por subir, **↓N** = commits por traer.
- Candado para repositorios privados.
- **Información** del repo (clic derecho): nombre en GitHub, alias, dueño, carpeta, remoto (sin credenciales), visibilidad, rama, nº de commits, último commit con fecha y autor, cambios locales y stashes.
- Abrir en el editor (VS Code, Cursor, Zed o Sublime Text), en la terminal o en el explorador de archivos.
- **Worktrees:** crear y quitar.

### Cambios y commits
- Lista de archivos modificados con casillas; al abrir un repo se muestra el diff del primer archivo.
- **Staging parcial por línea y por bloque:** elige qué líneas entran al commit, o un bloque entero con la casilla de su cabecera `@@` (queda a medias si solo hay algunas líneas).
- Diff **unificado o lado a lado**, con numeración, visor de imágenes (antes y ahora) y render virtual para diffs enormes.
- **Descartar** cambios con confirmación, y menú de clic derecho por archivo: descartar, ignorar (añade a `.gitignore`), copiar ruta, mostrar en el explorador y abrir en el editor.
- **macOS:** al arrancar, Grove añade `.DS_Store` al ignore **global** de Git (`core.excludesFile`, o `~/.config/git/ignore`), así nunca aparece como cambio ni se sube. No modifica los `.gitignore` de tus proyectos.
- Con **un solo archivo** marcado, el resumen se puede dejar vacío: Grove usa «Crea / Actualiza / Elimina &lt;archivo&gt;», sin gastar IA.
- Commit con resumen y descripción, **enmendar** el último commit y **deshacer** el último commit. Pasa por `git commit`, así que se respetan hooks y firma.
- Botón ✨ para **redactar el commit con IA** (opcional, ver [más abajo](#mensajes-de-commit-con-ia)).

### Historial
- **Recuperar** (reflog): lista de todos los sitios donde ha estado HEAD y «Crear rama…» en cualquiera, para rescatar commits tras un reset o un borrado.
- Clic derecho en un commit: **Crear rama aquí…** y **Volver a este commit…** (conservando los cambios o descartándolo todo, con aviso de cuántos commits y archivos se pierden).
- **Buscar** en los mensajes y ver el **historial de un archivo** (clic derecho en el archivo).
- **Ver el historial de otra rama** (selector arriba, con opción «solo lo que falta en la actual»): ahí el clic derecho ofrece **cherry-pick** para copiar un commit a la rama actual.
- Clic derecho en un commit de la rama actual: **revertir** (también commits de fusión) y **crear tag** (anotado, con opción de subirlo). Los tags se ven como etiquetas en la lista.
- Lista de commits paginada, detalle y diff por archivo.
- Filtros por **autor** y por **rango de fechas**.
- Fotos de los autores (ver [Privacidad](#privacidad-y-seguridad)), o iniciales si no hay sesión.

### Ramas
- **Ramas obsoletas** (su remota ya se borró): etiqueta «remota eliminada» o «sin fusionar» y botón **Limpiar…**. Las fusionadas se preseleccionan (borrarlas no pierde commits); las que tienen commits propios nunca se marcan solas. Los worktrees cuya carpeta se borró se limpian solos; los que están en un disco o recurso de red desconectado se conservan y solo se olvidan si lo pides.
- **Comparar ramas:** commits de diferencia en cada sentido y diff por archivo desde el ancestro común (como un Pull Request).
- **Fusionar** una rama en la actual; con conflictos aparece un diálogo para resolver archivo a archivo (con vista previa de las dos versiones), concluir o abortar (igual para cherry-pick y revert).
- Crear (con opción de publicar al crear), cambiar, **renombrar** y **borrar**, locales y remotas.
- Al cambiar de rama con cambios locales: dejarlos en stash o llevarlos contigo.

### Stash
- Guardar con mensaje, ver su lista, **renombrar**, eliminar, **aplicar** o **restaurar**, y ver el contenido y el diff de cada archivo del stash.

### Sincronización
- **Pull con rebase** opcional (menú junto al botón de sincronizar): historial lineal, con `--autostash`. En un rebase «Mía» sigue siendo tu commit aunque git invierta los lados.
- Tras subir una rama que no es la principal, aviso para **crear el Pull Request**.
- Un solo botón superior que se adapta al estado: **Publicar**, **Push origin**, **Pull origin** o **Fetch origin**, con insignia de commits por subir/traer.
- Desplegable con solo las acciones que aplican en ese momento.
- Progreso en vivo de fetch, pull, push y clone.
- Si un push es rechazado porque el remoto tiene commits nuevos, se ofrece hacer pull primero. No hay push forzado en la interfaz, a propósito.

### Conflictos
- Detección de fusiones con conflicto tras un pull o merge.
- Resolución dentro de la app por archivo: quedarte con **lo tuyo**, con **lo de ellos** o editar a mano; luego **continuar** o **abortar** la fusión.

### Pull Requests (GitHub)
- Listar (abiertos, cerrados, todos), revisar con **diff por archivo**, ver estado de **checks** y **comentarios**, comentar.
- **Crear**, **fusionar** (merge, squash o rebase), **cerrar** y hacer *checkout* de un PR.

### Interfaz
- Tema **claro y oscuro** según el sistema, con paleta verde propia.
- Arranca maximizada y sin destellos de color.
- Iconos [Lucide](https://lucide.dev).

---

## Primeros pasos

### Descargar

Los instaladores están en [**Releases**](https://github.com/jdsolerr/grove/releases/latest):

| Sistema | Archivo |
|---|---|
| macOS (Apple Silicon: M1 o posterior) | `grove_macos_arm64_vX.Y.Z.dmg` |
| macOS (Intel) | `grove_macos_x64_vX.Y.Z.dmg` |
| Windows (64 bits) | `grove_windows_x64_vX.Y.Z.exe` |
| Linux (64 bits) | `grove_linux_x64_vX.Y.Z.AppImage` (dale permiso de ejecución: `chmod +x`) |

Grove usa el Git que tengas instalado, así que necesitas [Git ≥ 2.40](https://git-scm.com).

> Los instaladores **aún no están firmados**. macOS puede decir que la app "está dañada" o es de un desarrollador no identificado: ábrela con clic derecho → *Abrir*, o ejecuta `xattr -cr /Applications/Grove.app`. Windows puede mostrar el aviso de SmartScreen: *Más información* → *Ejecutar de todos modos*.

### Requisitos

| Herramienta | Versión | Para qué |
|---|---|---|
| [Git](https://git-scm.com) | ≥ 2.40 | Escrituras y red (Grove usa el que tengas instalado) |
| [Rust](https://rustup.rs) | estable reciente (edición 2024) | Backend |
| [Node.js](https://nodejs.org) | 20 o superior | Interfaz (Vite + SolidJS) |
| [Prerrequisitos de Tauri 2](https://tauri.app/start/prerequisites/) | según tu sistema | Webview y herramientas de compilación |

### Ejecutar en desarrollo

```bash
git clone https://github.com/jdsolerr/grove.git
cd grove/app
npm install
npm run tauri dev
```

La primera compilación tarda un poco; las siguientes son incrementales.

### Variables de entorno

Copia `.env.example` a `.env` en la raíz del repo y rellena los valores (el `.env` está en `.gitignore`).

| Variable | Obligatoria | Para qué |
|---|---|---|
| `GROVE_GITHUB_CLIENT_ID` | Solo para iniciar sesión en GitHub | Client ID de tu OAuth App (no es un secreto). |

Se lee **al compilar** (`npm run tauri dev` o `build`) y queda fijado en el binario, así que los usuarios del instalador no configuran nada. Una variable de entorno con el mismo nombre tiene prioridad sobre el `.env` (es lo que usa el CI) y, al ejecutar, también manda sobre el valor compilado. Del `.env` solo se lee esa clave; nada más entra en el binario.

### Generar el instalador

```bash
cd app
npm run tauri build
```

El instalador queda en `target/release/bundle/` (el formato depende de tu sistema). Para otra arquitectura añade `-- --target <triple>` (por ejemplo `aarch64-apple-darwin`); entonces la ruta es `target/<triple>/release/bundle/`.

Sin el client ID el instalador se genera igualmente, pero el inicio de sesión de GitHub mostrará un error.

### Iniciar sesión en GitHub

Abre el selector de repositorios y pulsa **Iniciar sesión con GitHub**. Grove usa el *device flow* de OAuth: te muestra un código, lo introduces en GitHub y listo. El token se guarda en el llavero del sistema (Keychain, Credential Manager o Secret Service), nunca en un archivo.

Sin sesión, Grove funciona igual para todo lo local (cambios, historial, ramas, stash). La sesión añade fotos de autores, repos privados en la lista de clonado, publicar, Pull Requests y credenciales para red por HTTPS.

Los instaladores oficiales traen el client ID de la OAuth App de Grove. Si compilas desde el código o haces un *fork*, registra tu propia OAuth App en GitHub (*Settings → Developer settings → OAuth Apps*, con *Device Flow* activado) y define `GROVE_GITHUB_CLIENT_ID` (ver [Variables de entorno](#variables-de-entorno)). Así tu fork nunca aparece ante los usuarios como «Grove».

---

## Mensajes de commit con IA

Un botón ✨ junto al resumen del commit redacta **título y descripción en español**, breves y directos, a partir de los archivos que marcaste.

**Activarlo:** pulsa ⚙ junto al resumen del commit (o ✨ mientras la IA esté apagada) y elige proveedor, modelo y clave. Sirve **cualquier API compatible con OpenAI**; Grove trae listos:

| Proveedor | Modelo sugerido |
|---|---|
| [Groq](https://console.groq.com/keys) (plan gratuito) | `openai/gpt-oss-20b` |
| [OpenAI](https://platform.openai.com/api-keys) | `gpt-4o-mini` |
| [OpenRouter](https://openrouter.ai/keys) | el que elijas |
| [Gemini](https://aistudio.google.com/apikey) | `gemini-2.5-flash` |
| Ollama (local, sin clave) | tu modelo local |

Para otro proveedor elige **Personalizado** y escribe su URL. La clave se guarda en el llavero del sistema; la URL y el modelo, en la base local de Grove. **Apagar IA** borra la clave.

**Cuida tu cuota.** Lo que se envía está acotado:

- Se manda la **lista completa de archivos** con sus líneas añadidas/quitadas, y el **diff** de los más pequeños primero, recortado por archivo, hasta un tope de 12 000 caracteres.
- Lockfiles, binarios y archivos minificados van solo en la lista, sin su diff.
- Si hay demasiados cambios, lo que no cabe se resume como "y otros N archivos". Nunca se rinde por tamaño.
- Los límites gratuitos de cada proveedor son aparte; si se agotan, Grove muestra "cuota agotada, espera un momento".

> Los modelos de razonamiento, como `gpt-oss`, gastan cuota pensando. Grove pide el esfuerzo mínimo, pero conviene vigilar los límites de tu plan.

---

## Cómo está hecho

```
grove/
├── Cargo.toml                 # workspace de Rust
├── crates/
│   ├── grove-git/             # Git: gix (lectura) + binario git (escritura/red)
│   ├── grove-github/          # API de GitHub: OAuth, PRs, checks, avatares, repos
│   ├── grove-ai/              # Mensajes de commit vía API compatible con OpenAI
│   └── grove-store/           # SQLite: repos, alias, grupos y ajustes
├── app/
│   ├── src-tauri/             # Comandos Tauri, eventos, watcher, ventana
│   └── src/                   # Interfaz SolidJS + TypeScript
├── fixtures/                  # Scripts que generan repos de prueba
└── assets/                    # Logo
```

| Capa | Tecnología |
|---|---|
| Shell de la app | Tauri 2 |
| Interfaz | SolidJS, TypeScript, Vite |
| Git (lectura) | `gix` |
| Git (escritura y red) | el binario `git` del sistema |
| GitHub | REST con `ureq` y OAuth *device flow* |
| Credenciales | `keyring` (llavero del sistema) |
| Persistencia | SQLite con `rusqlite` (modo WAL) |
| Tipos Rust ↔ TypeScript | `tauri-specta` (se generan solos, sin duplicar modelos) |
| Listas grandes | `@tanstack/solid-virtual` |

**Principios:**

- Los *crates* `grove-git`, `grove-github`, `grove-ai` y `grove-store` **no conocen Tauri**: se prueban por separado.
- La interfaz es delgada: toda la lógica vive en Rust.
- Las operaciones de red emiten **progreso por eventos** y la interfaz se actualiza sola cuando el repo cambia en disco (*watcher* con *debounce*).
- Los errores de Git se traducen a mensajes y acciones concretas (conflicto, rechazo de push, autenticación).

---

## Privacidad y seguridad

- **Tu token de GitHub** vive en el llavero del sistema. No se escribe en disco ni se registra.
- **IA apagada por defecto.** Solo se envía algo a un proveedor externo si lo configuras en ⚙, y únicamente el diff recortado de los archivos que marcaste. Su clave vive en el llavero del sistema, nunca en un archivo, y no se reenvía si cambias a otro proveedor.
- **Fotos de autores:** con sesión iniciada, Grove consulta a GitHub quién es el autor de cada commit y carga su avatar desde `github.com`/`avatars.githubusercontent.com`. Para correos `noreply` de GitHub usa el identificador del propio correo; nunca envía tu correo real ni su hash (Gravatar). Sin sesión se muestran iniciales y no se pide ninguna imagen.
- **Remotos con credenciales** en la URL se muestran sin ellas.
- **Sin push forzado** en la interfaz, para evitar sobrescribir historia remota por accidente.

---

## Desarrollo

```bash
# Pruebas de todos los crates
cargo test --workspace

# Comprobar tipos y construir la interfaz
cd app
npx tsc --noEmit
npx vite build
```

**Notas útiles:**

- `app/src/bindings.ts` lo **genera** la app al arrancar en modo debug a partir de los comandos de Rust. Si añades un comando, ábrela una vez (`npm run tauri dev`) para regenerarlo.
- Los parámetros de los comandos no pueden llamarse como palabras reservadas de JavaScript (`new`, `private`…).
- Los tiempos se exponen como `f64`: `tauri-specta` no permite enteros de 64 bits.

---

## Estado del proyecto y hoja de ruta

Grove está en desarrollo activo (v0.1). Lo anterior funciona hoy; esto es lo que falta o está pendiente:

- [ ] Rebase y *squash* desde la interfaz
- [ ] Pull Requests desde *forks*, paginación de más de 50 PR, re-ejecutar checks fallidos, reviews y comentarios en línea
- [ ] Publicar en organizaciones
- [ ] Varias cuentas de GitHub y GitHub Enterprise Server
- [ ] Submódulos y Git LFS
- [ ] Atajos de teclado completos (ya hay ⌘K paleta, ⌘⏎ commit, ⌘⇧P sincronizar, ⌘⇧F fetch, ⌘1/2/3 pestañas)
- [ ] Selector manual de tema, i18n (es/en) y accesibilidad completa
- [ ] Actualización automática firmada e integración CLI (`grove .`)

**Metas de rendimiento** (objetivos de diseño, aún sin medir de forma sistemática): arranque en frío por debajo de 400 ms, menos de 150 MB de RAM en reposo y scroll fluido en historiales muy largos.

---

## Licencia y agradecimientos

Grove se distribuye bajo licencia **MIT** (ver [`LICENSE`](LICENSE)). El nombre y el logo de Grove no van incluidos en esa licencia: ver [`TRADEMARK.md`](TRADEMARK.md). Para contribuir, [`CONTRIBUTING.md`](CONTRIBUTING.md); para reportar vulnerabilidades, [`SECURITY.md`](SECURITY.md).

Gracias a los proyectos en los que se apoya: [Tauri](https://tauri.app), [SolidJS](https://www.solidjs.com), [gitoxide](https://github.com/GitoxideLabs/gitoxide) y [Lucide](https://lucide.dev).

La estructura de paneles y algunas medidas de la interfaz se inspiran en [GitHub Desktop](https://github.com/desktop/desktop) (MIT). Los avisos de licencia de terceros están en [`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES).

Grove es un proyecto independiente y **no está afiliado ni respaldado por GitHub**. "GitHub" es una marca de GitHub, Inc.
