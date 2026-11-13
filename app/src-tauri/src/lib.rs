mod launch;

use serde::{Deserialize, Serialize};
use specta::Type;
use std::{path::PathBuf, sync::Mutex};
use tauri::{Emitter, Manager};
use tauri_specta::{collect_commands, Builder};

#[derive(Serialize, Type)]
pub struct FileChange {
    path: String,
    kind: String,
}

#[derive(Serialize, Type)]
pub struct Commit {
    id: String,
    summary: String,
    author: String,
    email: String,
    time: u32,
}

// gix es bloqueante: corre en hilo aparte para no congelar la UI.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn changes(path: PathBuf) -> Result<Vec<FileChange>, String> {
    blocking(move || {
        Ok(grove_git::status_files(&path)?
            .into_iter()
            .map(|c| FileChange { path: c.path, kind: c.kind })
            .collect())
    })
    .await
}

#[derive(Deserialize, Type)]
pub struct HistFilter {
    author: Option<String>,
    since: Option<String>,
    until: Option<String>,
    search: Option<String>,
    file: Option<String>,
    rev: Option<String>,
    only_new: Option<bool>,
}

#[tauri::command]
#[specta::specta]
async fn history(path: PathBuf, skip: u32, limit: u32, filter: HistFilter) -> Result<Vec<Commit>, String> {
    blocking(move || {
        let f = grove_git::LogFilter { author: filter.author, since: filter.since, until: filter.until, search: filter.search, file: filter.file, rev: filter.rev, only_new: filter.only_new.unwrap_or(false) };
        let v = if !f.is_empty() { grove_git::log_filtered(&path, skip as usize, limit as usize, &f)? } else { grove_git::log_entries(&path, skip as usize, limit as usize)? };
        Ok(v.into_iter().map(|c| Commit { id: c.id, summary: c.summary, author: c.author, email: c.email, time: c.time as u32 }).collect())
    })
    .await
}

#[derive(Serialize, Type)]
pub struct Author {
    name: String,
    email: String,
    commits: u32,
}

#[tauri::command]
#[specta::specta]
async fn authors(path: PathBuf) -> Result<Vec<Author>, String> {
    blocking(move || Ok(grove_git::authors(&path)?.into_iter().map(|(name, email, commits)| Author { name, email, commits }).collect())).await
}

#[derive(Serialize, Type)]
pub struct Stash {
    index: u32,
    message: String,
    time: u32,
}

#[tauri::command]
#[specta::specta]
async fn stash_list(path: PathBuf) -> Result<Vec<Stash>, String> {
    blocking(move || Ok(grove_git::stash_list(&path)?.into_iter().map(|s| Stash { index: s.index, message: s.message, time: s.time as u32 }).collect())).await
}

#[tauri::command]
#[specta::specta]
async fn stash_push(path: PathBuf, message: String) -> Result<(), String> {
    blocking(move || grove_git::stash_push(&path, &message)).await
}

#[tauri::command]
#[specta::specta]
async fn stash_rename(path: PathBuf, index: u32, message: String) -> Result<(), String> {
    blocking(move || grove_git::stash_rename(&path, index, &message)).await
}

/// `op`: "pop" | "apply" | "drop".
#[tauri::command]
#[specta::specta]
async fn stash_op(path: PathBuf, index: u32, op: String) -> Result<(), String> {
    blocking(move || grove_git::stash_op(&path, index, &op)).await
}

#[derive(Serialize, Type)]
pub struct Branch {
    name: String,
    time: u32,
    remote: bool,
}

#[derive(Serialize, Type)]
pub struct RepoEntry {
    path: String,
    /// Dueño según la URL de `origin` (para agrupar la lista).
    owner: Option<String>,
    /// Nombre del repo según `origin` (el de GitHub); se muestra por defecto en vez del de la carpeta.
    repo: Option<String>,
    /// Nombre que puso el usuario (vacío = usa el de la carpeta).
    alias: String,
    /// Grupo que definió el usuario, p. ej. "Ascend" (vacío = agrupa por dueño del remoto).
    group: String,
}

#[derive(Serialize, Type)]
pub struct Worktree {
    path: String,
    branch: Option<String>,
}

#[tauri::command]
#[specta::specta]
async fn branches(path: PathBuf) -> Result<Vec<Branch>, String> {
    blocking(move || {
        Ok(grove_git::branch_list(&path)?
            .into_iter()
            .map(|b| Branch { name: b.name, time: b.time as u32, remote: b.remote })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn create_branch(path: PathBuf, name: String) -> Result<(), String> {
    blocking(move || grove_git::create_branch(&path, &name)).await
}

#[tauri::command]
#[specta::specta]
async fn rename_branch(path: PathBuf, old: String, to: String) -> Result<(), String> {
    blocking(move || grove_git::rename_branch(&path, &old, &to)).await
}

#[tauri::command]
#[specta::specta]
async fn delete_branch(path: PathBuf, name: String, force: bool) -> Result<(), String> {
    blocking(move || grove_git::delete_branch(&path, &name, force)).await
}

// ---- GitHub (OAuth device flow) ----
#[derive(Serialize, Type)]
pub struct GhCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u32,
}

#[derive(Serialize, Type)]
pub struct GhUser {
    login: String,
    avatar_url: String,
}

#[derive(Serialize, Type)]
pub struct GhPoll {
    /// "pending" | "slow" | "done"
    status: String,
    user: Option<GhUser>,
}

// Client ID público (no es secreto) de la OAuth App. Orden: variable de entorno en ejecución,
// luego el valor fijado al compilar los instaladores oficiales (workflow de release).
// Los forks no lo heredan: deben registrar su propia OAuth App.
// Limitación: un solo client_id, sin GitHub Enterprise Server ni varias cuentas.
fn client_id() -> anyhow::Result<String> {
    std::env::var("GROVE_GITHUB_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("GROVE_GITHUB_CLIENT_ID").map(str::to_owned))
        .ok_or_else(|| anyhow::anyhow!(
            "Falta el client ID de tu OAuth App de GitHub. Regístrala y define GROVE_GITHUB_CLIENT_ID (ver README)."
        ))
}

fn gh_user_dto(u: grove_github::User) -> GhUser {
    GhUser { login: u.login, avatar_url: u.avatar_url }
}

#[tauri::command]
#[specta::specta]
async fn gh_login_start() -> Result<GhCode, String> {
    blocking(|| {
        let c = grove_github::device_start(&client_id()?)?;
        Ok(GhCode { device_code: c.device_code, user_code: c.user_code, verification_uri: c.verification_uri, interval: c.interval })
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn gh_login_poll(device_code: String) -> Result<GhPoll, String> {
    blocking(move || {
        Ok(match grove_github::device_poll(&client_id()?, &device_code)? {
            grove_github::Poll::Pending => GhPoll { status: "pending".into(), user: None },
            grove_github::Poll::SlowDown => GhPoll { status: "slow".into(), user: None },
            grove_github::Poll::Done(t) => {
                let u = grove_github::user(&t)?;
                grove_github::save_token(&t)?;
                GhPoll { status: "done".into(), user: Some(gh_user_dto(u)) }
            }
        })
    })
    .await
}

/// Usuario de la sesión guardada; si el token fue revocado lo borra y devuelve `None`.
#[tauri::command]
#[specta::specta]
async fn gh_user() -> Result<Option<GhUser>, String> {
    blocking(|| {
        let Some(t) = grove_github::load_token() else { return Ok(None) };
        match grove_github::user(&t) {
            Ok(u) => Ok(Some(gh_user_dto(u))),
            Err(e) if e.to_string().contains("token inválido") => {
                grove_github::logout()?;
                Ok(None)
            }
            Err(e) => Err(e), // sin red: no cierra la sesión
        }
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn gh_logout() -> Result<(), String> {
    blocking(grove_github::logout).await
}

/// Fotos por autor: `picks` = (correo, varios sha de commits suyos, el más reciente primero). Solo viajan sha públicos.
/// Se prueban en orden hasta que GitHub conozca uno (los commits locales sin publicar dan 422).
#[tauri::command]
#[specta::specta]
async fn gh_avatars(path: PathBuf, picks: Vec<(String, Vec<String>)>) -> Result<Vec<(String, String)>, String> {
    blocking(move || {
        let Some(t) = grove_github::load_token() else { return Ok(vec![]) };
        let Some((owner, repo)) = grove_git::remote_slug(&path) else { return Ok(vec![]) };
        let (mut out, mut first_err) = (vec![], None);
        'authors: for (email, shas) in picks {
            for sha in shas {
                match grove_github::commit_avatar(&t, &owner, &repo, &sha) {
                    Ok(grove_github::Avatar::Url(u)) => { out.push((email, u)); continue 'authors; }
                    Ok(grove_github::Avatar::NoAccount) => continue 'authors,
                    Ok(grove_github::Avatar::Missing) => {}
                    Err(e) => { first_err.get_or_insert(e); continue 'authors; }
                }
            }
        }
        match (out.is_empty(), first_err) {
            (true, Some(e)) => Err(e),
            _ => Ok(out),
        }
    })
    .await
}

#[derive(Deserialize, Type)]
pub enum SwitchMode {
    Plain,
    Stash,
    Carry,
}

/// `false` = bloqueado por cambios locales; la UI debe preguntar Stash o Carry.
#[tauri::command]
#[specta::specta]
async fn switch_branch(path: PathBuf, branch: String, mode: SwitchMode) -> Result<bool, String> {
    let mode = match mode {
        SwitchMode::Plain => grove_git::SwitchMode::Plain,
        SwitchMode::Stash => grove_git::SwitchMode::Stash,
        SwitchMode::Carry => grove_git::SwitchMode::Carry,
    };
    blocking(move || grove_git::switch_branch(&path, &branch, mode)).await
}

#[tauri::command]
#[specta::specta]
async fn file_diff(path: PathBuf, file: String) -> Result<String, String> {
    blocking(move || grove_git::file_diff(&path, &file)).await
}

#[tauri::command]
#[specta::specta]
async fn commit(path: PathBuf, picks: Vec<Pick>, summary: String, body: String, amend: bool) -> Result<(), String> {
    let picks: Vec<_> = picks.into_iter().map(|p| grove_git::Pick { file: p.file, lines: p.lines }).collect();
    blocking(move || grove_git::commit(&path, &picks, &summary, &body, amend)).await
}

/// `lines: null` = archivo completo; si no, índices de líneas `+`/`-` del diff.
#[derive(Deserialize, Type)]
pub struct Pick {
    file: String,
    lines: Option<Vec<u32>>,
}

#[derive(Serialize, Type)]
pub struct Message {
    summary: String,
    body: String,
}

#[tauri::command]
#[specta::specta]
async fn last_message(path: PathBuf) -> Result<Message, String> {
    blocking(move || grove_git::last_message(&path).map(|(summary, body)| Message { summary, body })).await
}

#[tauri::command]
#[specta::specta]
async fn undo_commit(path: PathBuf) -> Result<Message, String> {
    blocking(move || grove_git::undo_commit(&path).map(|(summary, body)| Message { summary, body })).await
}

/// DESTRUCTIVO: la UI confirma antes de llamar.
#[tauri::command]
#[specta::specta]
async fn discard(path: PathBuf, files: Vec<String>) -> Result<(), String> {
    blocking(move || grove_git::discard(&path, &files)).await
}

type Watcher = Mutex<Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>>;

#[derive(Clone, Serialize)]
struct RepoChanged {
    path: String,
    /// HEAD, índice o refs cambiaron (rama, historial), no solo archivos del worktree.
    meta: bool,
}

/// Vigila un repo (reemplaza al anterior) y emite `repo-changed` con debounce.
/// Si no puede vigilar (p. ej. límite de inotify en Linux) devuelve error y la UI cae a refrescar al enfocar.
#[tauri::command]
#[specta::specta]
fn watch_repo(app: tauri::AppHandle, w: tauri::State<Watcher>, path: PathBuf) -> Result<(), String> {
    *app.state::<Refresh>().current.lock().unwrap() = Some(path.to_string_lossy().into_owned());
    use grove_git::RepoEvent;
    use notify::RecursiveMode::{NonRecursive, Recursive};
    let dirs = grove_git::watch_dirs(&path).map_err(|e| e.to_string())?;
    let (root, git_dir, common) = (dirs.root.clone(), dirs.git_dir.clone(), dirs.common_dir.clone());
    let key = path.to_string_lossy().into_owned();
    let mut deb = notify_debouncer_mini::new_debouncer(
        std::time::Duration::from_millis(400),
        move |res: notify_debouncer_mini::DebounceEventResult| {
            let Ok(events) = res else { return };
            let (mut meta, mut work) = (false, Vec::new());
            for e in events {
                match dirs.classify(&e.path) {
                    Some(RepoEvent::Meta) => meta = true,
                    Some(RepoEvent::Work(r)) => work.push(r),
                    None => {}
                }
            }
            let work = grove_git::not_ignored(&dirs.root, &work).unwrap_or(work);
            if meta || !work.is_empty() {
                let _ = app.emit("repo-changed", RepoChanged { path: key.clone(), meta });
            }
        },
    )
    .map_err(|e| e.to_string())?;
    let hint = |e: notify::Error| format!("no se pudo vigilar el repositorio ({e}); se refrescará al volver a la ventana");
    let wt = deb.watcher();
    wt.watch(&root, Recursive).map_err(hint)?;
    // Worktree enlazado: HEAD/índice viven fuera de la raíz y las refs en el git dir común.
    if !git_dir.starts_with(&root) {
        wt.watch(&git_dir, NonRecursive).map_err(hint)?;
        wt.watch(&common.join("refs"), Recursive).map_err(hint)?;
    }
    *w.lock().unwrap() = Some(deb); // soltar el anterior lo detiene
    Ok(())
}

/// Epoch (s) del último fetch, o null si nunca.
#[tauri::command]
#[specta::specta]
async fn last_fetched(path: PathBuf) -> Result<Option<u32>, String> {
    blocking(move || Ok(grove_git::last_fetched(&path)?.map(|t| t as u32))).await
}

#[derive(Serialize, Type, Clone)]
pub struct NetProgress {
    label: String,
    pct: u32,
}

fn progress(app: tauri::AppHandle) -> impl FnMut(&str, u32) {
    move |label, pct| {
        let _ = app.emit("net-progress", NetProgress { label: label.into(), pct });
    }
}

fn token() -> Option<String> {
    grove_github::load_token()
}

#[tauri::command]
#[specta::specta]
async fn fetch(app: tauri::AppHandle, path: PathBuf) -> Result<Option<u32>, String> {
    blocking(move || Ok(grove_git::fetch(&path, token().as_deref(), &mut progress(app))?.map(|t| t as u32))).await
}

#[tauri::command]
#[specta::specta]
async fn pull(app: tauri::AppHandle, db: tauri::State<'_, Db>, path: PathBuf) -> Result<(), String> {
    let rebase = pull_rebase_on(&db);
    blocking(move || grove_git::pull(&path, token().as_deref(), rebase, &mut progress(app))).await
}

fn pull_rebase_on(db: &Db) -> bool {
    db.lock().unwrap().get("pull_rebase").is_ok_and(|v| v == "1")
}

/// ¿«Pull» reaplica tus commits encima (rebase, historial lineal) en vez de crear una fusión?
#[tauri::command]
#[specta::specta]
fn pull_rebase(db: tauri::State<Db>) -> bool {
    pull_rebase_on(&db)
}

#[tauri::command]
#[specta::specta]
fn set_pull_rebase(db: tauri::State<Db>, on: bool) -> Result<(), String> {
    db.lock().unwrap().set("pull_rebase", if on { "1" } else { "0" }).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn push(app: tauri::AppHandle, path: PathBuf, force: bool) -> Result<(), String> {
    blocking(move || grove_git::push(&path, token().as_deref(), force, &mut progress(app))).await
}

#[tauri::command]
#[specta::specta]
async fn merge_abort(path: PathBuf) -> Result<(), String> {
    blocking(move || grove_git::merge_abort(&path)).await
}

#[derive(Serialize, Type)]
pub struct SyncState {
    has_origin: bool,
    upstream: bool,
    ahead: u32,
    behind: u32,
    merging: bool,
    op: String,
    conflicts: Vec<String>,
}

#[tauri::command]
#[specta::specta]
async fn sync_state(path: PathBuf) -> Result<SyncState, String> {
    blocking(move || {
        let s = grove_git::sync_state(&path)?;
        Ok(SyncState { has_origin: s.has_origin, upstream: s.upstream, ahead: s.ahead, behind: s.behind, merging: s.merging, op: s.op.into(), conflicts: s.conflicts })
    })
    .await
}

#[derive(Serialize, Type, Clone)]
pub struct RepoOverview {
    path: String,
    /// Archivos con cambios sin commitear.
    changes: u32,
    ahead: u32,
    behind: u32,
}

fn overview(p: &str) -> RepoOverview {
    let path = std::path::Path::new(p);
    let (ahead, behind) = grove_git::ahead_behind(path);
    RepoOverview { path: p.into(), changes: grove_git::status_count(path).unwrap_or(0) as u32, ahead, behind }
}

/// Ejecuta `f` sobre cada elemento con `n` hilos como máximo (cola compartida por índice atómico).
fn par_each<T: Sync>(items: &[T], n: usize, f: impl Fn(&T) + Sync) {
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..n.min(items.len()) {
            sc.spawn(|| {
                while let Some(it) = items.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                    f(it);
                }
            });
        }
    });
}

fn cpus() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// Estado de varios repos a la vez para los indicadores de la lista (solo datos locales: no hace fetch).
#[tauri::command]
#[specta::specta]
async fn repos_overview(paths: Vec<String>) -> Result<Vec<RepoOverview>, String> {
    blocking(move || {
        let out = Mutex::new(Vec::with_capacity(paths.len()));
        par_each(&paths, cpus(), |p| out.lock().unwrap().push(overview(p)));
        Ok(out.into_inner().unwrap())
    })
    .await
}

/// Refresco de todos los repos registrados al abrir la app y al volver a la ventana (minimizada/en segundo plano).
/// Sin timers: solo corre cuando el usuario vuelve. Fase 1 local (rápida, emite `repo-overview` por repo);
/// fase 2 fetch solo de los repos con `origin` cuyo último intento tiene más de `FETCH_EVERY`.
#[derive(Default)]
struct Refresh {
    ready: std::sync::atomic::AtomicBool,
    busy: std::sync::atomic::AtomicBool,
    last_local: Mutex<Option<std::time::Instant>>,
    tried: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    /// Repo abierto en la UI: su fetch ya lo gestiona el frontend.
    current: Mutex<Option<String>>,
}

const FETCH_EVERY: std::time::Duration = std::time::Duration::from_secs(300);
const LOCAL_EVERY: std::time::Duration = std::time::Duration::from_secs(30);
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const FETCH_WORKERS: usize = 4;

fn refresh_all(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering::{AcqRel, Acquire};
    let Some(st) = app.try_state::<Refresh>() else { return };
    if !st.ready.load(Acquire) || st.busy.swap(true, AcqRel) {
        return;
    }
    let Ok(paths) = app.state::<Db>().lock().unwrap().repos() else {
        st.busy.store(false, std::sync::atomic::Ordering::Release);
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let st = app.state::<Refresh>();
        let now = std::time::Instant::now();
        let local = st.last_local.lock().unwrap().is_none_or(|t| now.duration_since(t) > LOCAL_EVERY);
        if local {
            *st.last_local.lock().unwrap() = Some(now);
            par_each(&paths, cpus(), |p| {
                let _ = app.emit("repo-overview", overview(p));
            });
        }
        let cur = st.current.lock().unwrap().clone();
        let due: Vec<&String> = paths
            .iter()
            .filter(|p| Some(*p) != cur.as_ref())
            .filter(|p| st.tried.lock().unwrap().get(*p).is_none_or(|t| now.duration_since(*t) > FETCH_EVERY))
            .filter(|p| grove_git::has_origin(std::path::Path::new(p)))
            .collect();
        let tok = token();
        par_each(&due, FETCH_WORKERS, |p| {
            st.tried.lock().unwrap().insert((*p).clone(), std::time::Instant::now());
            let path = std::path::Path::new(p.as_str());
            // Sin red o sin permisos: se ignora y se reintenta pasado el intervalo.
            if grove_git::fetch_quiet(path, tok.as_deref(), FETCH_TIMEOUT).is_ok() {
                let _ = app.emit("repo-overview", overview(p));
            }
        });
        st.busy.store(false, std::sync::atomic::Ordering::Release);
    });
}

/// Merge de `branch` en la rama actual. `true` = hecho; `false` = quedó a medias con conflictos.
#[tauri::command]
#[specta::specta]
async fn merge_branch(path: PathBuf, branch: String) -> Result<bool, String> {
    blocking(move || Ok(grove_git::merge_branch(&path, &branch)? == grove_git::Outcome::Done)).await
}

/// Commit que deshace `id`. `true` = hecho; `false` = conflictos por resolver.
#[tauri::command]
#[specta::specta]
async fn revert_commit(path: PathBuf, id: String) -> Result<bool, String> {
    blocking(move || Ok(grove_git::revert_commit(&path, &id)? == grove_git::Outcome::Done)).await
}

/// Aplica `id` sobre la rama actual. `true` = hecho; `false` = conflictos por resolver.
#[tauri::command]
#[specta::specta]
async fn cherry_pick(path: PathBuf, id: String) -> Result<bool, String> {
    blocking(move || Ok(grove_git::cherry_pick(&path, &id)? == grove_git::Outcome::Done)).await
}

#[derive(Serialize, Type)]
pub struct RepoSync {
    path: String,
    /// `updated` (avanzó), `behind` (hay commits por traer que no se pudieron aplicar solos),
    /// `ok` (al día o sin remoto) o `failed`.
    status: String,
    detail: String,
}

/// Fetch (y con `pull`, avance rápido seguro) de varios repos a la vez, p. ej. un grupo entero.
/// El avance rápido nunca crea fusiones ni conflictos; lo que no sea seguro se deja como `behind`.
#[tauri::command]
#[specta::specta]
async fn sync_repos(app: tauri::AppHandle, paths: Vec<String>, pull: bool) -> Result<Vec<RepoSync>, String> {
    blocking(move || {
        let tok = token();
        let out = Mutex::new(Vec::with_capacity(paths.len()));
        par_each(&paths, FETCH_WORKERS, |p| {
            let path = std::path::Path::new(p.as_str());
            app.state::<Refresh>().tried.lock().unwrap().insert(p.clone(), std::time::Instant::now());
            let r = (|| -> anyhow::Result<(&str, String)> {
                if !grove_git::has_origin(path) {
                    return Ok(("ok", "sin remoto".into()));
                }
                grove_git::fetch_quiet(path, tok.as_deref(), FETCH_TIMEOUT)?;
                if pull && grove_git::fast_forward(path)? {
                    return Ok(("updated", String::new()));
                }
                let (ahead, behind) = grove_git::ahead_behind(path);
                Ok(match (behind, ahead) {
                    (0, _) => ("ok", String::new()),
                    (_, 0) => ("behind", String::new()),
                    _ => ("behind", "divergida: requiere pull manual".into()),
                })
            })();
            let _ = app.emit("repo-overview", overview(p));
            let (status, detail) = match r {
                Ok((s, d)) => (s.to_string(), d),
                Err(e) => ("failed".into(), e.to_string()),
            };
            out.lock().unwrap().push(RepoSync { path: p.clone(), status, detail });
        });
        Ok(out.into_inner().unwrap())
    })
    .await
}

#[derive(Serialize, Type)]
pub struct Tag {
    name: String,
    /// Commit (7 caracteres) al que apunta.
    id: String,
}

#[tauri::command]
#[specta::specta]
async fn tags(path: PathBuf) -> Result<Vec<Tag>, String> {
    blocking(move || Ok(grove_git::tags(&path)?.into_iter().map(|t| Tag { name: t.name, id: t.id }).collect())).await
}

/// Tag sobre `id`; con `message` anotado; con `push` se sube a origin.
#[tauri::command]
#[specta::specta]
async fn create_tag(path: PathBuf, id: String, name: String, message: String, push: bool) -> Result<(), String> {
    blocking(move || grove_git::create_tag(&path, &id, &name, &message, push, token().as_deref())).await
}

#[tauri::command]
#[specta::specta]
async fn delete_tag(path: PathBuf, name: String, remote: bool) -> Result<(), String> {
    blocking(move || grove_git::delete_tag(&path, &name, remote, token().as_deref())).await
}

#[derive(Serialize, Type)]
pub struct StaleBranch {
    name: String,
    /// Sus commits ya están en otra rama: borrarla no pierde nada.
    merged: bool,
}

/// Ramas locales cuya rama remota ya no existe.
#[tauri::command]
#[specta::specta]
async fn stale_branches(path: PathBuf) -> Result<Vec<StaleBranch>, String> {
    blocking(move || Ok(grove_git::stale_branches(&path)?.into_iter().map(|s| StaleBranch { name: s.name, merged: s.merged }).collect())).await
}

/// Borra las ramas obsoletas indicadas; devuelve cuántas.
#[tauri::command]
#[specta::specta]
async fn clean_branches(path: PathBuf, names: Vec<String>) -> Result<u32, String> {
    blocking(move || grove_git::clean_branches(&path, &names)).await
}

#[derive(Serialize, Type)]
pub struct GhostWorktree {
    path: String,
    branch: Option<String>,
}

/// Worktrees cuya carpeta falta porque su unidad no está conectada (no se olvidan solos).
#[tauri::command]
#[specta::specta]
async fn ghost_worktrees(path: PathBuf) -> Result<Vec<GhostWorktree>, String> {
    blocking(move || Ok(grove_git::ghost_worktrees(&path)?.into_iter().map(|w| GhostWorktree { path: w.path, branch: w.branch }).collect())).await
}

/// Olvida todos los worktrees ausentes (solo metadatos). Solo bajo petición expresa.
#[tauri::command]
#[specta::specta]
async fn forget_ghost_worktrees(path: PathBuf) -> Result<(), String> {
    blocking(move || grove_git::forget_ghost_worktrees(&path)).await
}

#[derive(Serialize, Type)]
pub struct ReflogEntry {
    id: String,
    summary: String,
    action: String,
    time: u32,
}

/// Últimos movimientos de HEAD, para recuperar commits tras un reset, borrado o aborto.
#[tauri::command]
#[specta::specta]
async fn reflog(path: PathBuf, limit: u32) -> Result<Vec<ReflogEntry>, String> {
    blocking(move || {
        Ok(grove_git::reflog(&path, limit as usize)?.into_iter().map(|e| ReflogEntry { id: e.id, summary: e.summary, action: e.action, time: e.time as u32 }).collect())
    })
    .await
}

/// Crea la rama `name` en el commit `rev`; con `activate` cambia a ella.
#[tauri::command]
#[specta::specta]
async fn branch_at(path: PathBuf, name: String, rev: String, activate: bool) -> Result<(), String> {
    blocking(move || grove_git::branch_at(&path, &name, &rev, activate)).await
}

/// Cuántos commits de la rama actual se perderían al volver a `rev`.
#[tauri::command]
#[specta::specta]
async fn commits_since(path: PathBuf, rev: String) -> Result<u32, String> {
    blocking(move || grove_git::commits_since(&path, &rev)).await
}

/// Mueve la rama actual a `rev`: `mixed` conserva los cambios, `hard` los descarta.
#[tauri::command]
#[specta::specta]
async fn reset_to(path: PathBuf, rev: String, mode: String) -> Result<(), String> {
    blocking(move || grove_git::reset_to(&path, &rev, &mode)).await
}

#[derive(Serialize, Type)]
pub struct CmpFile {
    path: String,
    kind: String,
    additions: u32,
    deletions: u32,
}

#[derive(Serialize, Type)]
pub struct Comparison {
    /// Commits que `other` tiene y `base` no.
    ahead: u32,
    /// Commits que `base` tiene y `other` no.
    behind: u32,
    files: Vec<CmpFile>,
}

/// Qué cambia `other` respecto a `base` (diff desde su ancestro común) y cuántos commits difieren.
#[tauri::command]
#[specta::specta]
async fn compare_branches(path: PathBuf, base: String, other: String) -> Result<Comparison, String> {
    blocking(move || {
        let c = grove_git::compare(&path, &base, &other)?;
        Ok(Comparison {
            ahead: c.ahead,
            behind: c.behind,
            files: c.files.into_iter().map(|f| CmpFile { path: f.path, kind: f.kind, additions: f.additions, deletions: f.deletions }).collect(),
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn compare_diff(path: PathBuf, base: String, other: String, file: String) -> Result<String, String> {
    blocking(move || grove_git::compare_diff(&path, &base, &other, &file)).await
}

/// Contenido de un archivo en conflicto por lado: (mía, suya). `None` = borrado, binario o muy grande.
#[tauri::command]
#[specta::specta]
async fn conflict_sides(path: PathBuf, file: String) -> Result<(Option<String>, Option<String>), String> {
    blocking(move || grove_git::conflict_sides(&path, &file)).await
}

#[derive(Serialize, Type)]
pub struct GhRepo {
    full_name: String,
    name: String,
    private: bool,
    clone_url: String,
}

#[tauri::command]
#[specta::specta]
async fn gh_repos() -> Result<Vec<GhRepo>, String> {
    blocking(|| {
        let t = token().ok_or_else(|| anyhow::anyhow!("inicia sesión en GitHub para ver tus repositorios"))?;
        Ok(grove_github::list_repos(&t)?
            .into_iter()
            .map(|r| GhRepo { full_name: r.full_name, name: r.name, private: r.private, clone_url: r.clone_url })
            .collect())
    })
    .await
}

/// Clona `url` en `parent/name`, lo registra en la lista y devuelve su ruta.
#[tauri::command]
#[specta::specta]
async fn clone_repo(app: tauri::AppHandle, db: tauri::State<'_, Db>, parent: PathBuf, url: String, name: String) -> Result<String, String> {
    let dest = blocking(move || grove_git::clone(&parent, url.trim(), name.trim(), token().as_deref(), &mut progress(app))).await?;
    let dest = dest.to_str().ok_or("ruta no UTF-8")?.to_string();
    db.lock().unwrap().add_repo(&dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

// ---- Pull Requests ----
#[derive(Serialize, Type)]
pub struct Pr {
    number: u32,
    title: String,
    author: String,
    avatar_url: String,
    head: String,
    base: String,
    draft: bool,
    state: String,
    updated_at: String,
    url: String,
}

#[derive(Serialize, Type)]
pub struct PrFile {
    path: String,
    status: String,
    additions: u32,
    deletions: u32,
    patch: Option<String>,
}

#[derive(Serialize, Type)]
pub struct PrDetail {
    pr: Pr,
    body: String,
    mergeable: Option<bool>,
    mergeable_state: String,
    commits: u32,
    head_sha: String,
    files: Vec<PrFile>,
}

fn pr_dto(p: grove_github::Pr) -> Pr {
    Pr { number: p.number, title: p.title, author: p.author, avatar_url: p.avatar_url, head: p.head, base: p.base, draft: p.draft, state: p.state, updated_at: p.updated_at, url: p.url }
}

/// (token, owner, repo) para hablar con la API sobre el repo abierto.
fn gh_ctx(path: &std::path::Path) -> anyhow::Result<(String, String, String)> {
    let t = token().ok_or_else(|| anyhow::anyhow!("inicia sesión en GitHub para usar Pull Requests"))?;
    let (o, r) = grove_git::remote_slug(path).ok_or_else(|| anyhow::anyhow!("este repositorio no tiene `origin` en github.com"))?;
    Ok((t, o, r))
}

#[tauri::command]
#[specta::specta]
async fn pr_list(path: PathBuf, state: String) -> Result<Vec<Pr>, String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        Ok(grove_github::pr_list(&t, &o, &r, &state)?.into_iter().map(pr_dto).collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn pr_detail(path: PathBuf, number: u32) -> Result<PrDetail, String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        let d = grove_github::pr_detail(&t, &o, &r, number)?;
        Ok(PrDetail {
            pr: pr_dto(d.pr),
            body: d.body,
            mergeable: d.mergeable,
            mergeable_state: d.mergeable_state,
            commits: d.commits,
            head_sha: d.head_sha,
            files: d.files.into_iter().map(|f| PrFile { path: f.path, status: f.status, additions: f.additions, deletions: f.deletions, patch: f.patch }).collect(),
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn pr_create(path: PathBuf, title: String, body: String, head: String, base: String, draft: bool) -> Result<Pr, String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        Ok(pr_dto(grove_github::pr_create(&t, &o, &r, &grove_github::NewPr { title: &title, body: &body, head: &head, base: &base, draft })?))
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn pr_merge(path: PathBuf, number: u32, method: String) -> Result<(), String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        grove_github::pr_merge(&t, &o, &r, number, &method)
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn pr_close(path: PathBuf, number: u32) -> Result<(), String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        grove_github::pr_close(&t, &o, &r, number)
    })
    .await
}

#[derive(Serialize, Type)]
pub struct Check {
    name: String,
    state: String,
    url: String,
}

#[derive(Serialize, Type)]
pub struct Comment {
    author: String,
    avatar_url: String,
    body: String,
    created_at: String,
}

fn comment_dto(c: grove_github::Comment) -> Comment {
    Comment { author: c.author, avatar_url: c.avatar_url, body: c.body, created_at: c.created_at }
}

#[tauri::command]
#[specta::specta]
async fn pr_checks(path: PathBuf, sha: String) -> Result<Vec<Check>, String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        Ok(grove_github::pr_checks(&t, &o, &r, &sha)?.into_iter().map(|c| Check { name: c.name, state: c.state, url: c.url }).collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn pr_comments(path: PathBuf, number: u32) -> Result<Vec<Comment>, String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        Ok(grove_github::pr_comments(&t, &o, &r, number)?.into_iter().map(comment_dto).collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn pr_comment(path: PathBuf, number: u32, body: String) -> Result<Comment, String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        Ok(comment_dto(grove_github::pr_comment(&t, &o, &r, number, &body)?))
    })
    .await
}

/// Crea el repo en GitHub, lo enlaza como `origin` y sube la rama actual.
#[tauri::command]
#[specta::specta]
async fn gh_publish(app: tauri::AppHandle, path: PathBuf, name: String, description: String, is_private: bool) -> Result<(), String> {
    blocking(move || {
        let t = token().ok_or_else(|| anyhow::anyhow!("inicia sesión en GitHub para publicar"))?;
        if grove_git::sync_state(&path)?.has_origin {
            anyhow::bail!("este repositorio ya tiene `origin`");
        }
        let (_, url) = grove_github::create_repo(&t, &name, &description, is_private)?;
        grove_git::add_origin(&path, &url)?;
        grove_git::push(&path, Some(&t), false, &mut progress(app))
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn wt_remove(path: PathBuf, target: String, force: bool) -> Result<(), String> {
    blocking(move || grove_git::worktree_remove(&path, &target, force)).await
}

#[tauri::command]
#[specta::specta]
async fn delete_remote_branch(path: PathBuf, name: String) -> Result<(), String> {
    blocking(move || grove_git::delete_remote_branch(&path, &name, token().as_deref())).await
}

#[tauri::command]
#[specta::specta]
async fn resolve_conflict(path: PathBuf, file: String, side: String) -> Result<(), String> {
    blocking(move || grove_git::resolve_conflict(&path, &file, &side)).await
}

#[tauri::command]
#[specta::specta]
/// `true` = commit creado; `false` = quedó vacío y se omitió.
async fn merge_continue(path: PathBuf) -> Result<bool, String> {
    blocking(move || grove_git::merge_continue(&path)).await
}

#[tauri::command]
#[specta::specta]
async fn default_branch(path: PathBuf) -> Result<String, String> {
    blocking(move || {
        let (t, o, r) = gh_ctx(&path)?;
        grove_github::default_branch(&t, &o, &r)
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn pr_checkout(app: tauri::AppHandle, path: PathBuf, number: u32) -> Result<String, String> {
    blocking(move || grove_git::checkout_pr(&path, number, token().as_deref(), &mut progress(app))).await
}

/// `Some(true)` privado, `Some(false)` público, `None` si no es github.com o no se pudo saber.
#[tauri::command]
#[specta::specta]
async fn repo_private(path: PathBuf) -> Result<Option<bool>, String> {
    blocking(move || {
        let Some((owner, repo)) = grove_git::remote_slug(&path) else { return Ok(None) };
        Ok(grove_github::repo_private(grove_github::load_token().as_deref(), &owner, &repo))
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn worktrees(path: PathBuf) -> Result<Vec<Worktree>, String> {
    blocking(move || {
        Ok(grove_git::worktrees(&path)?
            .into_iter()
            .map(|w| Worktree { path: w.path, branch: w.branch })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
fn open_in(target: launch::Target, path: PathBuf) -> Result<(), String> {
    launch::open(target, &path)
}

#[tauri::command]
#[specta::specta]
fn file_action(action: launch::FileAction, repo: PathBuf, file: String) -> Result<(), String> {
    launch::file_action(action, &repo, &file)
}

/// Añade el archivo a `.gitignore` del repo.
#[tauri::command]
#[specta::specta]
async fn ignore_file(path: PathBuf, file: String) -> Result<(), String> {
    blocking(move || grove_git::ignore_file(&path, &file)).await
}

/// Imagen antes (HEAD) y ahora (disco) como data URL, para el visor de diffs.
#[tauri::command]
#[specta::specta]
async fn image_versions(path: PathBuf, file: String) -> Result<(Option<String>, Option<String>), String> {
    blocking(move || grove_git::image_versions(&path, &file)).await
}

#[derive(Serialize, Type)]
pub struct CommitDetail {
    id: String,
    author: String,
    email: String,
    time: u32,
    summary: String,
    body: String,
    files: Vec<FileChange>,
    additions: u32,
    deletions: u32,
}

#[tauri::command]
#[specta::specta]
async fn commit_info(path: PathBuf, id: String) -> Result<CommitDetail, String> {
    blocking(move || {
        let i = grove_git::commit_info(&path, &id)?;
        Ok(CommitDetail {
            id: i.id,
            author: i.author,
            email: i.email,
            time: i.time as u32,
            summary: i.summary,
            body: i.body,
            additions: i.additions,
            deletions: i.deletions,
            files: i.files.into_iter().map(|c| FileChange { path: c.path, kind: c.kind }).collect(),
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn commit_diff(path: PathBuf, id: String, file: String) -> Result<String, String> {
    blocking(move || grove_git::commit_diff(&path, &id, &file)).await
}

/// (nombre, correo) configurados en git para este repo.
#[tauri::command]
#[specta::specta]
async fn identity(path: PathBuf) -> Result<(String, String), String> {
    blocking(move || grove_git::identity(&path)).await
}

type Db = Mutex<grove_store::Store>;

#[tauri::command]
#[specta::specta]
async fn list_repos(db: tauri::State<'_, Db>) -> Result<Vec<RepoEntry>, String> {
    let (paths, meta) = {
        let d = db.lock().unwrap();
        (d.repos().map_err(|e| e.to_string())?, d.meta().map_err(|e| e.to_string())?)
    };
    blocking(move || {
        Ok(paths
            .into_iter()
            .map(|p| {
                let (alias, group) = meta.iter().find(|m| m.0 == p).map(|m| (m.1.clone(), m.2.clone())).unwrap_or_default();
                RepoEntry { owner: grove_git::remote_owner(std::path::Path::new(&p)), repo: grove_git::remote_repo(std::path::Path::new(&p)), path: p, alias, group }
            })
            .collect())
    })
    .await
}

#[tauri::command]
#[specta::specta]
fn set_repo_meta(db: tauri::State<Db>, path: String, alias: String, group: String) -> Result<(), String> {
    db.lock().unwrap().set_meta(&path, &alias, &group).map_err(|e| e.to_string())
}

#[derive(Serialize, Type)]
pub struct AiMessage {
    summary: String,
    body: String,
}

/// Configuración de IA guardada. La clave nunca viaja a la UI: solo si existe.
#[derive(Serialize, Type)]
pub struct AiSettings {
    base_url: String,
    model: String,
    has_key: bool,
}

#[tauri::command]
#[specta::specta]
fn ai_settings(db: tauri::State<Db>) -> Result<AiSettings, String> {
    let db = db.lock().unwrap();
    let get = |k| db.get(k).map_err(|e| e.to_string());
    Ok(AiSettings { base_url: get("ai_base_url")?, model: get("ai_model")?, has_key: grove_ai::load_key().is_some() })
}

/// URL vacía = IA apagada. `key`: `None` conserva la guardada, `Some("")` la borra.
#[tauri::command]
#[specta::specta]
fn set_ai_settings(db: tauri::State<Db>, base_url: String, model: String, key: Option<String>) -> Result<(), String> {
    let (base_url, model) = (base_url.trim(), model.trim());
    if !base_url.is_empty() {
        grove_ai::check_url(base_url).map_err(|e| e.to_string())?;
    }
    if !base_url.is_empty() && model.is_empty() {
        return Err("falta el modelo".into());
    }
    let db = db.lock().unwrap();
    let prev = db.get("ai_base_url").map_err(|e| e.to_string())?;
    // Otro proveedor sin clave nueva: no reenviar la anterior a un host distinto.
    let key = key.or_else(|| (prev != base_url || base_url.is_empty()).then(String::new));
    if let Some(k) = key {
        grove_ai::save_key(k.trim()).map_err(|e| e.to_string())?;
    }
    db.set("ai_base_url", base_url).and_then(|_| db.set("ai_model", model)).map_err(|e| e.to_string())
}

/// Mensaje de commit para los archivos marcados; el diff que sale está acotado por `Config::max_chars`.
#[tauri::command]
#[specta::specta]
async fn ai_commit_message(db: tauri::State<'_, Db>, path: PathBuf, files: Vec<String>) -> Result<AiMessage, String> {
    let cfg = {
        let db = db.lock().unwrap();
        grove_ai::Config::new(db.get("ai_base_url").unwrap_or_default(), db.get("ai_model").unwrap_or_default())
    };
    blocking(move || {
        let cfg = cfg.ok_or_else(|| anyhow::anyhow!("IA sin configurar: ábrela con ⚙ junto al botón ✨"))?;
        if files.is_empty() {
            anyhow::bail!("no hay archivos seleccionados");
        }
        let digest = grove_git::ai_digest(&path, &files, cfg.max_chars)?;
        let (summary, body) = grove_ai::commit_message(&cfg, &digest)?;
        Ok(AiMessage { summary, body })
    })
    .await
}

#[tauri::command]
#[specta::specta]
async fn stash_files(path: PathBuf, index: u32) -> Result<Vec<FileChange>, String> {
    blocking(move || Ok(grove_git::stash_files(&path, index)?.into_iter().map(|f| FileChange { path: f.path, kind: f.kind }).collect())).await
}

#[tauri::command]
#[specta::specta]
async fn stash_diff(path: PathBuf, index: u32, file: String) -> Result<String, String> {
    blocking(move || grove_git::stash_diff(&path, index, &file)).await
}

#[tauri::command]
#[specta::specta]
async fn worktree_add(path: PathBuf, dest: String, branch: String, create: bool) -> Result<(), String> {
    blocking(move || grove_git::worktree_add(&path, &dest, &branch, create)).await
}

// Valida que sea un repo git antes de guardar (frontera de confianza: la ruta viene de la UI).
#[tauri::command]
#[specta::specta]
async fn add_repo(db: tauri::State<'_, Db>, path: PathBuf) -> Result<(), String> {
    let p = path.clone();
    blocking(move || grove_git::current_branch(&p)).await?;
    let path = path.to_str().ok_or("ruta no UTF-8")?;
    db.lock().unwrap().add_repo(path).map_err(|e| e.to_string())
}

#[derive(Serialize, Type)]
pub struct RepoInfo {
    branch: String,
    remote_url: Option<String>,
    commits: u32,
    last_commit: String,
    last_commit_time: Option<f64>,
    last_author: String,
    owner: Option<String>,
    repo: Option<String>,
    changes: u32,
    stashes: u32,
    branches: u32,
}

#[tauri::command]
#[specta::specta]
async fn repo_info(path: PathBuf) -> Result<RepoInfo, String> {
    blocking(move || {
        let i = grove_git::repo_info(&path)?;
        Ok(RepoInfo { branch: i.branch, remote_url: i.remote_url, commits: i.commits, last_commit: i.last_commit, last_commit_time: i.last_commit_time.map(|t| t as f64), last_author: i.last_author, owner: i.owner, repo: i.repo, changes: i.changes, stashes: i.stashes, branches: i.branches })
    })
    .await
}

/// La ventana nace oculta (sin destello blanco/oscuro) y el frontend la muestra al pintar el primer cuadro.
#[tauri::command]
#[specta::specta]
fn app_ready(window: tauri::WebviewWindow) {
    let _ = window.show();
    // Primer inicio: el frontend ya escucha `repo-overview`; desde aquí también reaccionan los eventos de foco.
    let app = window.app_handle();
    app.state::<Refresh>().ready.store(true, std::sync::atomic::Ordering::Release);
    refresh_all(app);
}

#[tauri::command]
#[specta::specta]
fn list_groups(db: tauri::State<Db>) -> Result<Vec<String>, String> {
    db.lock().unwrap().groups().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn add_group(db: tauri::State<Db>, name: String) -> Result<(), String> {
    db.lock().unwrap().add_group(&name).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn delete_group(db: tauri::State<Db>, name: String) -> Result<(), String> {
    db.lock().unwrap().delete_group(&name).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn remove_repo(db: tauri::State<Db>, path: String) -> Result<(), String> {
    db.lock().unwrap().remove_repo(&path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn branch(path: PathBuf) -> Result<String, String> {
    blocking(move || grove_git::current_branch(&path)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new().commands(collect_commands![changes, history, list_repos, add_repo, remove_repo, branch, branches, switch_branch, last_fetched, fetch, worktrees, file_diff, commit, last_message, undo_commit, discard, watch_repo, open_in, image_versions, commit_info, commit_diff, identity, create_branch, rename_branch, delete_branch, gh_login_start, gh_login_poll, gh_user, gh_logout, gh_avatars, pull, push, sync_state, repo_private, merge_abort, gh_repos, clone_repo, authors, stash_list, stash_push, stash_op, set_repo_meta, stash_files, stash_diff, worktree_add, stash_rename, pr_list, pr_detail, pr_create, pr_merge, pr_close, default_branch, pr_checkout, pr_checks, pr_comments, pr_comment, gh_publish, wt_remove, delete_remote_branch, resolve_conflict, merge_continue, ai_settings, set_ai_settings, ai_commit_message, list_groups, add_group, delete_group, repo_info, app_ready, repos_overview, merge_branch, revert_commit, cherry_pick, sync_repos, tags, create_tag, delete_tag, stale_branches, clean_branches, ghost_worktrees, forget_ghost_worktrees, reflog, branch_at, commits_since, reset_to, pull_rebase, set_pull_rebase, compare_branches, compare_diff, conflict_sides, file_action, ignore_file]);

    #[cfg(debug_assertions)]
    builder
        .export(specta_typescript::Typescript::default(), "../src/bindings.ts")
        .expect("export bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            // macOS: `.DS_Store` ignorado globalmente en Git, para que nunca se cuele en un commit.
            #[cfg(target_os = "macos")]
            std::thread::spawn(|| { let _ = grove_git::ignore_ds_store_globally(); });
            // Red de seguridad: si el frontend fallara antes de pedirlo, la ventana igual aparece.
            if let Some(w) = app.get_webview_window("main") {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    let _ = w.show();
                });
            }
            app.manage(Watcher::default());
            app.manage(Refresh::default());
            app.manage(Mutex::new(grove_store::Store::open(&dir.join("grove.db"))?));
            Ok(())
        })
        .on_window_event(|w, e| {
            if let tauri::WindowEvent::Focused(true) = e {
                refresh_all(w.app_handle());
            }
        })
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
