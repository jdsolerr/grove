//! Lectura de repos vía `gix`. Escrituras/red irán por el binario `git`.
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Cantidad de entradas sucias (modificadas/sin seguimiento) del worktree.
pub fn status_count(path: &Path) -> Result<usize> {
    let repo = gix::open(path)?;
    let n = repo
        .status(gix::progress::Discard)?
        .into_index_worktree_iter(Vec::new())?
        .filter_map(Result::ok)
        .count();
    Ok(n)
}

/// Resúmenes de los últimos `limit` commits desde HEAD.
pub fn log(path: &Path, limit: usize) -> Result<Vec<String>> {
    let repo = gix::open(path)?;
    let head = repo.head_id()?;
    let mut out = Vec::new();
    for info in head.ancestors().all()?.take(limit) {
        let commit = info?.object()?;
        out.push(commit.message()?.summary().to_string());
    }
    Ok(out)
}

/// Archivo cambiado: ruta relativa y tipo (`Added`, `Modified`, `Removed`, ...).
pub struct FileChange {
    pub path: String,
    pub kind: String,
}

pub fn status_files(path: &Path) -> Result<Vec<FileChange>> {
    let repo = gix::open(path)?;
    let mut items: Vec<FileChange> = repo
        .status(gix::progress::Discard)?
        .into_index_worktree_iter(Vec::new())?
        .filter_map(Result::ok)
        .filter_map(|i| {
            Some(FileChange {
                path: i.rela_path().to_string(),
                kind: format!("{:?}", i.summary()?),
            })
        })
        .collect();
    // gix compara índice↔worktree: lo que ya está en staging (`git add` hecho fuera de Grove)
    // y quedó igual al worktree no aparece. Se completa con índice↔HEAD.
    let seen: std::collections::HashSet<String> = items.iter().map(|c| c.path.clone()).collect();
    let staged = git(path, &["diff", "--cached", "--name-status", "--no-renames", "-z"])?;
    let mut f = staged.split('\0');
    while let (Some(st), Some(p)) = (f.next(), f.next()) {
        if st.is_empty() || seen.contains(p) {
            continue;
        }
        let kind = match st {
            "A" => "Added",
            "D" => "Removed",
            "T" => "TypeChange",
            _ => "Modified",
        };
        items.push(FileChange { path: p.into(), kind: kind.into() });
    }
    Ok(items)
}

pub struct Commit {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub email: String,
    pub time: i64,
}

pub fn log_entries(path: &Path, skip: usize, limit: usize) -> Result<Vec<Commit>> {
    let repo = gix::open(path)?;
    let mut out = Vec::new();
    for info in repo.head_id()?.ancestors().all()?.skip(skip).take(limit) {
        let c = info?.object()?;
        out.push(Commit {
            id: c.id.to_hex_with_len(7).to_string(),
            summary: c.message()?.summary().to_string(),
            author: c.author()?.name.to_string(),
            email: c.author()?.email.to_string(),
            time: c.author()?.seconds(),
        });
    }
    Ok(out)
}

#[derive(Default)]
pub struct LogFilter {
    pub author: Option<String>,
    /// `YYYY-MM-DD`, inclusivo
    pub since: Option<String>,
    /// `YYYY-MM-DD`, inclusivo
    pub until: Option<String>,
    /// Texto en el mensaje del commit (sin distinguir mayúsculas).
    pub search: Option<String>,
    /// Solo commits que tocan este archivo (sigue renombrados).
    pub file: Option<String>,
    /// Historial de esta rama (local o `origin/x`) en vez de la actual.
    pub rev: Option<String>,
    /// Con `rev`: solo los commits que la rama actual aún no tiene (candidatos a cherry-pick).
    pub only_new: bool,
}

impl LogFilter {
    pub fn is_empty(&self) -> bool {
        [&self.author, &self.since, &self.until, &self.search, &self.file, &self.rev].iter().all(|x| x.as_deref().is_none_or(str::is_empty))
    }
}

fn valid_date(d: &str) -> bool {
    d.len() == 10 && d.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })
}

/// Historial filtrado por autor y/o fechas. Usa `git log` (gix no filtra); sin filtros usar `log_entries`.
pub fn log_filtered(path: &Path, skip: usize, limit: usize, f: &LogFilter) -> Result<Vec<Commit>> {
    let (skip, limit) = (format!("--skip={skip}"), format!("-n{limit}"));
    let mut args = vec!["log", "--abbrev=7", "--format=%h%x00%s%x00%an%x00%ae%x00%at%x1e", &skip, &limit];
    let (a, s, u);
    if let Some(x) = f.author.as_deref().filter(|x| !x.is_empty()) {
        a = format!("--author={x}");
        args.extend(["--fixed-strings", "--regexp-ignore-case", &a]);
    }
    if let Some(x) = f.since.as_deref().filter(|x| !x.is_empty()) {
        if !valid_date(x) {
            bail!("fecha no válida: {x}");
        }
        s = format!("--since={x} 00:00:00");
        args.push(&s);
    }
    if let Some(x) = f.until.as_deref().filter(|x| !x.is_empty()) {
        if !valid_date(x) {
            bail!("fecha no válida: {x}");
        }
        u = format!("--until={x} 23:59:59");
        args.push(&u);
    }
    let q;
    if let Some(x) = f.search.as_deref().filter(|x| !x.is_empty()) {
        q = format!("--grep={x}");
        args.extend(["--fixed-strings", "--regexp-ignore-case", &q]);
    }
    // La revisión va antes de `--`; solo ramas que existen (el nombre viene de la UI).
    let range;
    if let Some(r) = f.rev.as_deref().filter(|x| !x.is_empty()) {
        if !branch_list(path)?.iter().any(|b| b.name == r) {
            bail!("la rama `{r}` no existe");
        }
        range = if f.only_new { format!("HEAD..{r}") } else { r.to_string() };
        args.extend(["--end-of-options", &range]);
    }
    if let Some(x) = f.file.as_deref().filter(|x| !x.is_empty()) {
        args.extend(["--follow", "--", x]);
    }
    let out = git(path, &args)?;
    Ok(out
        .split('\x1e')
        .filter_map(|r| {
            let mut c = r.trim_start_matches('\n').split('\0');
            Some(Commit {
                id: c.next().filter(|x| !x.is_empty())?.into(),
                summary: c.next()?.into(),
                author: c.next()?.into(),
                email: c.next()?.into(),
                time: c.next()?.trim().parse().ok()?,
            })
        })
        .collect())
}

/// Autores del historial (nombre, correo, commits), el más activo primero.
pub fn authors(path: &Path) -> Result<Vec<(String, String, u32)>> {
    Ok(git(path, &["shortlog", "-sne", "HEAD"])?
        .lines()
        .filter_map(|l| {
            let (n, rest) = l.trim().split_once('\t')?;
            let (name, email) = rest.rsplit_once(" <")?;
            Some((name.into(), email.trim_end_matches('>').into(), n.trim().parse().ok()?))
        })
        .collect())
}

pub struct StashEntry {
    pub index: u32,
    pub message: String,
    pub time: i64,
}

pub fn stash_list(path: &Path) -> Result<Vec<StashEntry>> {
    Ok(git(path, &["stash", "list", "--format=%ct%x00%gs"])?
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let (t, m) = l.split_once('\0')?;
            Some(StashEntry { index: i as u32, message: m.into(), time: t.parse().ok()? })
        })
        .collect())
}

/// Guarda los cambios (incluye archivos nuevos) y deja el árbol limpio.
pub fn stash_push(path: &Path, message: &str) -> Result<()> {
    let before = stash_list(path)?.len();
    let msg = message.trim();
    let mut args = vec!["stash", "push", "-u"];
    if !msg.is_empty() {
        args.extend(["-m", msg]);
    }
    git(path, &args)?;
    if stash_list(path)?.len() == before {
        bail!("no hay cambios que guardar");
    }
    Ok(())
}

/// Cambia el mensaje de un stash. Git no lo permite directamente: se guarda una copia con el mensaje nuevo
/// (queda la primera de la lista) y se elimina la original.
pub fn stash_rename(path: &Path, index: u32, message: &str) -> Result<()> {
    let msg = message.trim();
    if msg.is_empty() {
        bail!("el mensaje no puede estar vacío");
    }
    let r = format!("stash@{{{index}}}");
    // Misma instantánea con otro mensaje = otro commit: `stash store` ignora un commit idéntico al de arriba.
    let revs = git(path, &["rev-list", "--parents", "-n1", &r])?;
    let parents: Vec<&str> = revs.split_whitespace().skip(1).collect();
    let tree = format!("{r}^{{tree}}");
    let mut args = vec!["commit-tree", tree.as_str(), "-m", msg];
    for p in &parents {
        args.extend(["-p", p]);
    }
    let sha = git(path, &args)?.trim().to_string();
    git(path, &["stash", "store", "-m", msg, &sha])?;
    git(path, &["stash", "drop", &format!("stash@{{{}}}", index + 1)]).map(drop)
}

/// `pop` restaura y elimina; `apply` restaura y conserva; `drop` elimina.
pub fn stash_op(path: &Path, index: u32, op: &str) -> Result<()> {
    if !["pop", "apply", "drop"].contains(&op) {
        bail!("operación de stash no válida");
    }
    git(path, &["stash", op, &format!("stash@{{{index}}}")]).map(drop)
}

/// Archivos de un stash (incluye los nuevos sin seguimiento): (estado, ruta).
pub fn stash_files(path: &Path, index: u32) -> Result<Vec<FileChange>> {
    let r = format!("stash@{{{index}}}");
    let out = git(path, &["stash", "show", "--include-untracked", "--name-status", "-z", &r])?;
    let mut v = Vec::new();
    let mut it = out.split('\0');
    while let (Some(st), Some(p)) = (it.next(), it.next()) {
        let kind = match st.trim().chars().next() {
            Some('A') => "Added",
            Some('D') => "Removed",
            _ => "Modified",
        };
        v.push(FileChange { path: p.into(), kind: kind.into() });
    }
    Ok(v)
}

/// Diff de un archivo dentro de un stash: contra la base, o contra vacío si era un archivo nuevo.
pub fn stash_diff(path: &Path, index: u32, file: &str) -> Result<String> {
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let r = format!("stash@{{{index}}}");
    let tracked = git(path, &["diff", "--no-color", "--no-ext-diff", &format!("{r}^1"), &r, "--", file])?;
    if !tracked.is_empty() {
        return Ok(tracked);
    }
    // sin ^3 (stash sin archivos nuevos) el archivo simplemente no cambió
    git(path, &["diff", "--no-color", "--no-ext-diff", EMPTY_TREE, &format!("{r}^3"), "--", file]).or_else(|_| Ok(String::new()))
}

/// Crea un worktree en `dest`. `create`: rama nueva con ese nombre desde HEAD; si no, saca una rama existente
/// (una remota `origin/x` crea la local `x` siguiéndola).
pub fn worktree_add(path: &Path, dest: &str, branch: &str, create: bool) -> Result<()> {
    if !Path::new(dest).is_absolute() {
        bail!("la carpeta del worktree debe ser una ruta absoluta");
    }
    let branch = branch.trim();
    valid_branch(path, branch)?;
    if create {
        if branches(path)?.iter().any(|b| b == branch) {
            bail!("ya existe la rama `{branch}`");
        }
        return git(path, &["worktree", "add", "-b", branch, "--", dest]).map(drop);
    }
    match branch.strip_prefix("origin/") {
        Some(local) if !branches(path)?.iter().any(|b| b == local) => {
            git(path, &["worktree", "add", "--track", "-b", local, "--", dest, branch]).map(drop)
        }
        Some(local) => git(path, &["worktree", "add", "--", dest, local]).map(drop),
        None => git(path, &["worktree", "add", "--", dest, branch]).map(drop),
    }
}

/// Enlaza `origin` a `url` (solo https/ssh/git@, nunca una opción de git ni una ruta local).
pub fn add_origin(path: &Path, url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("ssh://") || url.starts_with("git@")) {
        bail!("URL de remoto no válida");
    }
    git(path, &["remote", "add", "origin", "--", url]).map(drop)
}

/// Quita un worktree enlazado. No permite el principal ni el que estás usando; sin `force` git se niega
/// si tiene cambios sin commitear.
pub fn worktree_remove(path: &Path, target: &str, force: bool) -> Result<()> {
    let list = worktrees(path)?;
    let canon = |p: &str| std::fs::canonicalize(p).unwrap_or_else(|_| p.into());
    let t = canon(target);
    if list.first().is_some_and(|m| canon(&m.path) == t) {
        bail!("no se puede quitar el worktree principal");
    }
    if canon(&path.to_string_lossy()) == t {
        bail!("no se puede quitar el worktree que tienes abierto: cambia a otro antes");
    }
    if !list.iter().any(|w| canon(&w.path) == t) {
        bail!("ese worktree no pertenece a este repositorio");
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", target]);
    git(path, &args).map(drop)
}

/// Borra `origin/<name>` en el remoto. No toca la rama local del mismo nombre.
pub fn delete_remote_branch(path: &Path, name: &str, token: Option<&str>) -> Result<()> {
    let name = name.strip_prefix("origin/").unwrap_or(name);
    valid_branch(path, name)?;
    if name == "HEAD" {
        bail!("no se puede borrar HEAD");
    }
    git_net(path, &["push", "origin", "--delete", name], token, &mut |_, _| {})
}

/// Resuelve un conflicto quedándote con tu versión (`ours`), la entrante (`theirs`), o marcando como resuelto
/// el archivo que ya editaste a mano (`manual`).
pub fn resolve_conflict(path: &Path, file: &str, side: &str) -> Result<()> {
    match side {
        "ours" | "theirs" => {
            git(path, &["checkout", &format!("--{}", resolved_side(path, side)?), "--", file])?;
        }
        "manual" => {}
        _ => bail!("opción de resolución no válida"),
    }
    git(path, &["add", "--", file]).map(drop)
}

/// Durante un rebase git invierte los lados: `--ours` es la rama base y `--theirs` tu commit. Aquí «mía» es
/// siempre lo tuyo, así que se traduce.
fn resolved_side(path: &Path, side: &str) -> Result<&'static str> {
    let rebasing = pending_op(gix::open(path)?.git_dir()) == "rebase";
    Ok(if (side == "ours") != rebasing { "ours" } else { "theirs" })
}

/// Contenido de un archivo en conflicto por cada lado (`mía`, `suya`), con los mismos lados que el resto de
/// la UI. `None` si ese lado no lo tiene (borrado), es binario o es demasiado grande para mostrarlo.
pub fn conflict_sides(path: &Path, file: &str) -> Result<(Option<String>, Option<String>)> {
    let rebasing = pending_op(gix::open(path)?.git_dir()) == "rebase";
    let side = |stage: u8| -> Option<String> {
        let o = git_raw(path, &["show", &format!(":{stage}:{file}")]).ok().filter(|o| o.status.success())?;
        (o.stdout.len() <= 256 * 1024 && !o.stdout.contains(&0)).then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    // Etapa 2 = ours de git, 3 = theirs; en un rebase están intercambiadas respecto a «mía/suya».
    let (a, b) = (side(2), side(3));
    Ok(if rebasing { (b, a) } else { (a, b) })
}

/// Termina la operación a medias (fusión, cherry-pick o revert) cuando ya no quedan conflictos.
/// `true` = se creó el commit; `false` = quedó vacío y el paso se omitió (`--skip`).
pub fn merge_continue(path: &Path) -> Result<bool> {
    let s = sync_state(path)?;
    if !s.merging {
        bail!("no hay ninguna operación en curso");
    }
    if !s.conflicts.is_empty() {
        bail!("aún quedan {} archivos en conflicto", s.conflicts.len());
    }
    // `core.editor=true`: se acepta el mensaje que git propone, sin abrir un editor.
    // Si al resolver no queda nada por commitear (te quedaste con lo que ya había), git rechaza
    // `--continue` por commit vacío: el paso se descarta con `--skip`.
    let empty = s.op != "merge" && git_raw(path, &["diff", "--cached", "--quiet"])?.status.success();
    let r = match s.op {
        "merge" => git(path, &["commit", "--no-edit"]),
        op if empty => git(path, &[op, "--skip"]),
        op => git(path, &["-c", "core.editor=true", op, "--continue"]),
    };
    // Un rebase reaplica varios commits: puede detenerse otra vez con conflictos en el siguiente. No es un error.
    if r.is_err() && s.op == "rebase" && sync_state(path).is_ok_and(|n| n.merging && !n.conflicts.is_empty()) {
        return Ok(true);
    }
    r?;
    Ok(!empty)
}

fn valid_rev(id: &str) -> Result<()> {
    if (4..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        bail!("commit no válido")
    }
}

/// Resultado de una operación que puede terminar en conflicto.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done,
    /// Quedó a medias con conflictos: resolverlos y concluir, o abortar.
    Conflicts,
}

fn outcome(path: &Path, r: Result<String>) -> Result<Outcome> {
    match r {
        Ok(_) => Ok(Outcome::Done),
        Err(_) if sync_state(path).is_ok_and(|s| s.merging && !s.conflicts.is_empty()) => Ok(Outcome::Conflicts),
        Err(e) => Err(e),
    }
}

/// Fusiona `branch` (local o `origin/x`) en la rama actual, sin abrir editor.
pub fn merge_branch(path: &Path, branch: &str) -> Result<Outcome> {
    if !branch_list(path)?.iter().any(|b| b.name == branch) {
        bail!("la rama `{branch}` no existe");
    }
    outcome(path, git(path, &["merge", "--no-edit", "--end-of-options", branch]))
}

/// Parámetro `-m 1` si `id` es un commit de fusión (git lo exige para revertir/cherry-pick).
fn mainline(path: &Path, id: &str) -> Result<Vec<&'static str>> {
    let parents = git(path, &["rev-list", "--parents", "-n1", id])?.split_whitespace().count() - 1;
    Ok(if parents > 1 { vec!["-m", "1"] } else { vec![] })
}

/// Crea un commit que deshace `id`.
pub fn revert_commit(path: &Path, id: &str) -> Result<Outcome> {
    valid_rev(id)?;
    let mut args = vec!["revert", "--no-edit"];
    args.extend(mainline(path, id)?);
    args.push(id);
    outcome(path, git(path, &args))
}

/// Aplica `id` sobre la rama actual.
pub fn cherry_pick(path: &Path, id: &str) -> Result<Outcome> {
    valid_rev(id)?;
    let mut args = vec!["cherry-pick"];
    args.extend(mainline(path, id)?);
    args.push(id);
    outcome(path, git(path, &args))
}

/// Rama actual (o `HEAD` si está desacoplado). También valida que `path` sea un repo.
pub fn current_branch(path: &Path) -> Result<String> {
    let repo = gix::open(path)?;
    Ok(repo.head_name()?.map_or("HEAD".into(), |n| n.shorten().to_string()))
}

use anyhow::{Context, bail};
use std::process::Command;

/// Ejecuta `git -C path ...` (las escrituras y la red van por el binario `git`).
/// Sin prompts interactivos: si falta credencial falla en vez de colgarse.
fn git_cmd(path: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_MESSAGES", "C") // mensajes de git en inglés: la UI reconoce algunos errores por su texto
        .env("GIT_OPTIONAL_LOCKS", "0") // lecturas no reescriben el índice (evita bucles con el watcher)
        // Conexión HTTP estancada (<1 B/s durante 60 s): git aborta en vez de colgar pull/push/clone para siempre.
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1")
        .env("GIT_HTTP_LOW_SPEED_TIME", "60");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: sin parpadeo de cmd en cada llamada
    }
    c
}

fn git_raw(path: &Path, args: &[&str]) -> Result<std::process::Output> {
    git_cmd(path, args).output().context("no se pudo ejecutar `git` (¿está instalado?)")
}

fn git(path: &Path, args: &[&str]) -> Result<String> {
    let out = git_raw(path, args)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
        bail!("{}", err.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// De `rels` (rutas relativas), las que NO están ignoradas por `.gitignore`.
/// Una sola invocación de `git check-ignore`; lo usa el watcher para no refrescar por ruido
/// de `target/`, `node_modules/`, etc.
pub fn not_ignored(path: &Path, rels: &[String]) -> Result<Vec<String>> {
    use std::io::Write;
    if rels.is_empty() {
        return Ok(vec![]);
    }
    let mut child = git_cmd(path, &["check-ignore", "-z", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("no se pudo ejecutar `git`")?;
    let mut stdin = child.stdin.take().unwrap();
    let input: Vec<u8> = rels.iter().flat_map(|r| r.bytes().chain([0])).collect();
    // Hilo aparte: escribir y leer en el mismo hilo se bloquea con entradas grandes.
    let w = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output()?;
    let _ = w.join();
    let ignored: std::collections::HashSet<&str> =
        std::str::from_utf8(&out.stdout)?.split('\0').filter(|s| !s.is_empty()).collect();
    Ok(rels.iter().filter(|r| !ignored.contains(r.as_str())).cloned().collect())
}

/// Rutas que el watcher debe conocer: raíz del worktree, git dir propio y git dir común.
/// En un worktree enlazado `.git` es un archivo y el git dir vive fuera de la raíz.
/// Todas canonicalizadas: los eventos del SO llegan resueltos (p. ej. `/private/var` en macOS).
pub struct WatchDirs {
    pub root: std::path::PathBuf,
    pub git_dir: std::path::PathBuf,
    pub common_dir: std::path::PathBuf,
}

pub fn watch_dirs(path: &Path) -> Result<WatchDirs> {
    let repo = gix::open(path)?;
    let c = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    Ok(WatchDirs {
        root: c(path),
        git_dir: c(repo.git_dir()),
        common_dir: c(repo.common_dir()),
    })
}

pub enum RepoEvent {
    /// HEAD, índice o refs: cambió rama/historial.
    Meta,
    /// Archivo del worktree (ruta relativa con `/`); falta filtrar lo ignorado.
    Work(String),
}

impl WatchDirs {
    pub fn classify(&self, ev: &Path) -> Option<RepoEvent> {
        if let Ok(g) = ev.strip_prefix(&self.git_dir) {
            let g = g.to_string_lossy();
            return (g == "HEAD" || g == "index" || g.starts_with("refs/")).then_some(RepoEvent::Meta);
        }
        if ev.starts_with(self.common_dir.join("refs")) {
            return Some(RepoEvent::Meta);
        }
        let rel = ev.strip_prefix(&self.root).ok()?.to_string_lossy().replace('\\', "/");
        // `.git` (carpeta o archivo de worktree enlazado) y su contenido no son archivos del usuario.
        (rel != ".git" && !rel.starts_with(".git/")).then_some(RepoEvent::Work(rel))
    }
}

/// Ramas locales, ordenadas por nombre.
pub fn branches(path: &Path) -> Result<Vec<String>> {
    let repo = gix::open(path)?;
    let mut v: Vec<String> = repo
        .references()?
        .local_branches()?
        .filter_map(Result::ok)
        .map(|r| r.name().shorten().to_string())
        .collect();
    v.sort();
    Ok(v)
}

pub struct BranchInfo {
    pub name: String,
    /// Fecha (epoch) del último commit de la rama.
    pub time: i64,
    pub remote: bool,
}

/// Ramas locales y remotas con la fecha de su último commit (un solo `for-each-ref`).
pub fn branch_list(path: &Path) -> Result<Vec<BranchInfo>> {
    let out = git(path, &["for-each-ref", "--format=%(refname)%00%(committerdate:unix)", "refs/heads", "refs/remotes"])?;
    let mut v = Vec::new();
    for l in out.lines() {
        let Some((r, t)) = l.split_once('\0') else { continue };
        let (name, remote) = match (r.strip_prefix("refs/heads/"), r.strip_prefix("refs/remotes/")) {
            (Some(n), _) => (n, false),
            (_, Some(n)) if !n.ends_with("/HEAD") => (n, true),
            _ => continue,
        };
        v.push(BranchInfo { name: name.into(), time: t.parse().unwrap_or(0), remote });
    }
    Ok(v)
}

/// Crea una rama desde el HEAD actual y cambia a ella (los cambios locales viajan con ella).
pub fn create_branch(path: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    // Valida con git mismo: rechaza vacíos, espacios, `..`, y nombres que empiecen con `-`.
    git(path, &["check-ref-format", "--branch", name]).map_err(|_| anyhow::anyhow!("`{name}` no es un nombre de rama válido"))?;
    if branches(path)?.iter().any(|b| b == name) {
        bail!("ya existe la rama `{name}`");
    }
    git(path, &["switch", "-c", name]).map(drop)
}

fn valid_branch(path: &Path, name: &str) -> Result<()> {
    git(path, &["check-ref-format", "--branch", name]).map(drop).map_err(|_| anyhow::anyhow!("`{name}` no es un nombre de rama válido"))
}

/// Renombra una rama local (la actual incluida).
pub fn rename_branch(path: &Path, old: &str, new: &str) -> Result<()> {
    let new = new.trim();
    valid_branch(path, new)?;
    if branches(path)?.iter().any(|b| b == new) {
        bail!("ya existe la rama `{new}`");
    }
    git(path, &["branch", "-m", old, new]).map(drop)
}

/// Borra una rama local. Sin `force` git se niega si no está fusionada (el error llega tal cual).
pub fn delete_branch(path: &Path, name: &str, force: bool) -> Result<()> {
    if current_branch(path)? == name {
        bail!("no se puede borrar la rama actual");
    }
    git(path, &["branch", if force { "-D" } else { "-d" }, "--", name]).map(drop)
}

/// `owner` de la URL de `origin` (GitHub, GitLab, ssh o https), si hay.
pub fn remote_owner(path: &Path) -> Option<String> {
    let repo = gix::open(path).ok()?;
    let url = repo.config_snapshot().string("remote.origin.url")?.to_string();
    parse_owner(&url)
}

/// (owner, repo) de `origin` si apunta a github.com.
pub fn remote_slug(path: &Path) -> Option<(String, String)> {
    let repo = gix::open(path).ok()?;
    let url = repo.config_snapshot().string("remote.origin.url")?.to_string();
    if !url.contains("github.com") {
        return None;
    }
    let tail = url.rsplit_once("github.com")?.1.trim_start_matches([':', '/']);
    let (o, r) = tail.split_once('/')?;
    let r = r.trim_end_matches('/').trim_end_matches(".git");
    // Viene de `.git/config` (repo ajeno): va a rutas de la API con tu token; `../`, `?` o `/` las desviarían a otros endpoints.
    let ok = |s: &str| !s.is_empty() && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    (ok(o) && ok(r)).then(|| (o.into(), r.into()))
}

/// Nombre del repo según `origin` (`https://host/owner/repo.git` → `repo`), en cualquier host.
pub fn remote_repo(path: &Path) -> Option<String> {
    let repo = gix::open(path).ok()?;
    let url = repo.config_snapshot().string("remote.origin.url")?.to_string();
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next()?;
    Some(last.trim_end_matches(".git").to_string()).filter(|n| !n.is_empty())
}

fn parse_owner(url: &str) -> Option<String> {
    // https://host/owner/repo.git · ssh://git@host/owner/repo · git@host:owner/repo.git
    let tail = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?.1,
        None => url.split_once(':')?.1,
    };
    let mut parts: Vec<&str> = tail.trim_matches('/').split('/').collect();
    parts.pop()?; // el nombre del repo
    parts.last().map(|o| o.to_string()).filter(|o| !o.is_empty())
}

#[derive(Clone, Copy, PartialEq)]
pub enum SwitchMode {
    /// Cambiar sin tocar nada; si hay cambios que chocan, no hace nada.
    Plain,
    /// Guardar los cambios en un stash y dejarlos en la rama actual.
    Stash,
    /// Guardar en stash, cambiar y reaplicarlos en la rama nueva.
    Carry,
}

/// `Ok(false)` = bloqueado por cambios locales que se sobrescribirían (no se tocó nada).
pub fn switch_branch(path: &Path, branch: &str, mode: SwitchMode) -> Result<bool> {
    // Solo ramas existentes: el nombre viene de la UI.
    let list = branch_list(path)?;
    let Some(info) = list.iter().find(|b| b.name == branch) else {
        bail!("la rama `{branch}` no existe");
    };
    // `origin/x` -> `x`: git la crea siguiendo al remoto (no queda HEAD desacoplado).
    let target = if info.remote { branch.split_once('/').map_or(branch, |(_, b)| b) } else { branch };
    let switch = || git(path, &["switch", "--end-of-options", target]);
    if mode == SwitchMode::Plain {
        return match switch() {
            Ok(_) => Ok(true),
            Err(e) if e.to_string().contains("would be overwritten") => Ok(false),
            Err(e) => Err(e),
        };
    }
    // `stash push` sin cambios sale con 0 y no crea nada: comparar refs/stash para no
    // hacer `pop` de un stash ajeno.
    let top = || git(path, &["rev-parse", "-q", "--verify", "refs/stash"]).ok();
    let before = top();
    let from = current_branch(path)?;
    git(path, &["stash", "push", "-u", "-m", &format!("grove: cambios en {from}")])?;
    let stashed = top() != before;
    if let Err(e) = switch() {
        if stashed {
            let _ = git(path, &["stash", "pop"]); // dejar el árbol como estaba
        }
        return Err(e);
    }
    if mode == SwitchMode::Carry && stashed {
        git(path, &["stash", "pop"]).map_err(|e| {
            anyhow::anyhow!("Se cambió de rama, pero al reaplicar tus cambios hubo conflictos (siguen en el stash): {e}")
        })?;
    }
    Ok(true)
}

/// Diff de un archivo contra HEAD (o contra vacío si es nuevo/sin seguimiento).
pub fn file_diff(path: &Path, file: &str) -> Result<String> {
    let mut d = file_diff_raw(path, file)?;
    if d.len() > MAX_DIFF {
        let mut n = MAX_DIFF;
        while !d.is_char_boundary(n) {
            n -= 1;
        }
        d.truncate(n);
        d.push_str("\n@@ diff recortado: archivo demasiado grande @@\n");
    }
    Ok(d)
}

const MAX_DIFF: usize = 5 * 1024 * 1024;

fn file_diff_raw(path: &Path, file: &str) -> Result<String> {
    let out = git(path, &["diff", "HEAD", "--no-color", "--no-ext-diff", "--", file])?;
    if !out.is_empty() {
        return Ok(out);
    }
    if git(path, &["ls-files", "--others", "--exclude-standard", "--", file])?.trim().is_empty() {
        return Ok(out);
    }
    // `--no-index` sale con 1 cuando hay diferencias: no es un error.
    let o = git_raw(path, &["diff", "--no-index", "--no-color", "--", "/dev/null", file])?;
    if o.status.code() == Some(1) {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        bail!("{}", String::from_utf8_lossy(&o.stderr).trim())
    }
}

/// Qué entra al commit de un archivo: todo (`lines: None`) o solo ciertas líneas `+`/`-`
/// del diff (índices sobre `file_diff().split('\n')`). Líneas solo para archivos ya versionados.
pub struct Pick {
    pub file: String,
    pub lines: Option<Vec<u32>>,
}

/// Commit de lo seleccionado. Deja el índice exactamente como la selección (lo que estuviera en
/// staging y no se marcó vuelve a ser cambio local) y pasa por `git commit`: hooks y firma intactos.
/// Con `amend` reescribe el último commit; ahí `picks` puede ir vacío (solo cambia el mensaje).
pub fn commit(path: &Path, picks: &[Pick], summary: &str, body: &str, amend: bool) -> Result<()> {
    let summary = summary.trim();
    if summary.is_empty() {
        bail!("el resumen del commit es obligatorio");
    }
    if picks.is_empty() && !amend {
        bail!("no hay archivos seleccionados");
    }
    // Índice := HEAD (o vacío si aún no hay commits).
    if git(path, &["rev-parse", "-q", "--verify", "HEAD"]).is_ok() {
        git(path, &["reset", "-q"])?;
    } else {
        git(path, &["read-tree", "--empty"])?;
    }
    let whole: Vec<&str> = picks.iter().filter(|p| p.lines.is_none()).map(|p| p.file.as_str()).collect();
    if !whole.is_empty() {
        let mut add = vec!["add", "-A", "--"];
        add.extend(whole);
        git(path, &add)?;
    }
    for p in picks.iter().filter(|p| p.lines.is_some()) {
        let sel = p.lines.as_deref().unwrap_or_default();
        let diff = git(path, &["diff", "--no-color", "--no-ext-diff", "--", &p.file])?;
        let patch = partial_patch(&diff, sel);
        if patch.is_empty() {
            bail!("la selección de líneas de `{}` ya no coincide con el archivo; vuelve a elegirlas", p.file);
        }
        git_stdin(path, &["apply", "--cached", "--recount", "-"], &patch)?;
    }
    let mut c = vec!["commit", "-m", summary];
    if !body.trim().is_empty() {
        c.extend(["-m", body.trim()]);
    }
    if amend {
        c.push("--amend");
    }
    git(path, &c).map(drop)
}

fn git_stdin(path: &Path, args: &[&str], input: &str) -> Result<()> {
    use std::io::Write;
    let mut child = git_cmd(path, args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("no se pudo ejecutar `git`")?;
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_owned();
    let w = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output()?;
    let _ = w.join();
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Parche con solo las líneas `+`/`-` elegidas (índices sobre las líneas de `diff`).
/// Un `-` no elegido pasa a contexto y un `+` no elegido se descarta; los hunks sin cambios
/// elegidos desaparecen. Los contadores del `@@` los recalcula `git apply --recount`.
/// Devuelve "" si no queda ningún cambio.
pub fn partial_patch(diff: &str, selected: &[u32]) -> String {
    let sel: std::collections::HashSet<u32> = selected.iter().copied().collect();
    let mut head = String::new();
    let mut hunks: Vec<(String, String, bool)> = Vec::new(); // (@@, cuerpo, tiene cambios)
    let mut dropped_last = false;
    for (i, l) in diff.split('\n').enumerate() {
        if l.starts_with("@@") {
            hunks.push((l.to_string(), String::new(), false));
            dropped_last = false;
        } else if let Some((_, body, changed)) = hunks.last_mut() {
            let keep = match l.chars().next() {
                Some('+') if sel.contains(&(i as u32)) => { *changed = true; Some(l.to_string()) }
                Some('+') => None,
                Some('-') if sel.contains(&(i as u32)) => { *changed = true; Some(l.to_string()) }
                Some('-') => Some(format!(" {}", &l[1..])),
                Some('\\') => (!dropped_last).then(|| l.to_string()), // "\ No newline…" sigue a su línea
                Some(' ') => Some(l.to_string()),
                _ => None, // línea vacía final del split
            };
            dropped_last = keep.is_none() && !l.is_empty();
            if let Some(k) = keep {
                body.push_str(&k);
                body.push('\n');
            }
        } else if !l.is_empty() {
            head.push_str(l);
            head.push('\n');
        }
    }
    if !hunks.iter().any(|h| h.2) {
        return String::new();
    }
    let mut out = head;
    for (h, body, changed) in hunks {
        if changed {
            out.push_str(&h);
            out.push('\n');
            out.push_str(&body);
        }
    }
    out
}

/// (resumen, cuerpo) del último commit, para prellenar el amend o el deshacer.
pub fn last_message(path: &Path) -> Result<(String, String)> {
    let m = git(path, &["log", "-1", "--format=%B"])?;
    let m = m.trim();
    Ok(match m.split_once("\n") {
        Some((a, b)) => (a.trim().into(), b.trim().into()),
        None => (m.into(), String::new()),
    })
}

/// `reset --soft HEAD~1`: los cambios quedan en staging. Devuelve el mensaje deshecho.
/// Reversible vía reflog. Falla si es el primer commit (no hay padre).
pub fn undo_commit(path: &Path) -> Result<(String, String)> {
    if !git(path, &["branch", "-r", "--contains", "HEAD"])?.trim().is_empty() {
        bail!("el último commit ya está publicado en un remoto; no se puede deshacer desde aquí");
    }
    let msg = last_message(path)?;
    git(path, &["reset", "--soft", "HEAD~1"]).context("no se puede deshacer el primer commit")?;
    Ok(msg)
}

/// Descarta cambios: tracked vuelve a HEAD (worktree e índice), sin seguimiento se borra.
/// DESTRUCTIVO e irrecuperable: la UI debe confirmar antes.
pub fn discard(path: &Path, files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let run = |pre: &[&str], f: &[String]| {
        let mut a: Vec<&str> = pre.to_vec();
        a.push("--");
        a.extend(f.iter().map(String::as_str));
        git(path, &a)
    };
    let untracked: Vec<String> = run(&["ls-files", "--others", "--exclude-standard", "-z"], files)?
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if !untracked.is_empty() {
        run(&["clean", "-f", "-q"], &untracked)?;
    }
    // Paths que git ya no conoce (p. ej. temporales borrados tras listar) se omiten: restore fallaría con "pathspec did not match".
    let known: Vec<String> = [
        run(&["ls-files", "-z"], files)?,
        run(&["ls-tree", "-r", "--name-only", "-z", "HEAD"], files)?,
    ]
    .iter()
    .flat_map(|o| o.split('\0').filter(|s| !s.is_empty()).map(String::from))
    .collect();
    let tracked: Vec<String> = files.iter().filter(|f| known.contains(f)).cloned().collect();
    if !tracked.is_empty() {
        run(&["restore", "--source=HEAD", "--staged", "--worktree"], &tracked)?;
    }
    Ok(())
}

/// `git fetch` y devuelve la hora (epoch) de la última actualización.
/// "Receiving objects:  45% (9/20)" -> ("Receiving objects", 45)
fn parse_progress(line: &str) -> Option<(&str, u32)> {
    let (label, num) = line.split_once('%')?.0.rsplit_once(':')?;
    Some((label.trim().trim_start_matches("remote:").trim(), num.trim().parse().ok()?))
}

/// Autentica contra github.com con `token` por entorno (no por argv, que otros procesos pueden ver).
fn auth(c: &mut Command, token: Option<&str>) {
    use base64::Engine;
    if let Some(t) = token {
        let b = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{t}"));
        c.env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader")
            .env("GIT_CONFIG_VALUE_0", format!("AUTHORIZATION: basic {b}"));
    }
}

/// Corre git con red y progreso. Con `token`, autentica contra github.com por cabecera HTTP pasada por
/// entorno (no por argv, que otros procesos pueden ver); sin token usa el helper de credenciales del sistema.
/// `on` recibe (etapa, porcentaje) mientras git informa por stderr.
fn git_net(path: &Path, args: &[&str], token: Option<&str>, on: &mut dyn FnMut(&str, u32)) -> Result<()> {
    use std::io::Read;
    // `--progress` va justo tras el subcomando: después de `--` git lo tomaría como argumento.
    let mut all = vec![args[0], "--progress"];
    all.extend(&args[1..]);
    let mut c = git_cmd(path, &all);
    auth(&mut c, token);
    let mut child = c
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("no se pudo ejecutar `git` (¿está instalado?)")?;
    let mut err = child.stderr.take().unwrap();
    let (mut buf, mut line, mut log) = ([0u8; 4096], Vec::<u8>::new(), String::new());
    let mut flush = |line: &mut Vec<u8>, on: &mut dyn FnMut(&str, u32)| {
        let l = String::from_utf8_lossy(line).trim().to_string();
        line.clear();
        if let Some((label, pct)) = parse_progress(&l) {
            on(label, pct);
        } else if !l.is_empty() {
            log.push_str(&l);
            log.push('\n');
        }
    };
    loop {
        let n = err.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            if b == b'\r' || b == b'\n' {
                flush(&mut line, on);
            } else {
                line.push(b);
            }
        }
    }
    flush(&mut line, on);
    if !child.wait()?.success() {
        bail!("{}", log.trim());
    }
    Ok(())
}

/// `fetch --prune` en segundo plano: sin progreso y con límite de tiempo; si el remoto se cuelga
/// (red caída a medias, servidor lento) se mata `git` en vez de bloquear al trabajador.
pub fn fetch_quiet(path: &Path, token: Option<&str>, timeout: std::time::Duration) -> Result<()> {
    let mut c = git_cmd(path, &["fetch", "--prune"]);
    auth(&mut c, token);
    let mut child = c
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("no se pudo ejecutar `git` (¿está instalado?)")?;
    let t = std::time::Instant::now();
    loop {
        if let Some(st) = child.try_wait()? {
            return if st.success() { Ok(()) } else { Err(anyhow::anyhow!("fetch falló ({st})")) };
        }
        if t.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("fetch: tiempo agotado");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// ¿Tiene remoto `origin`? (lectura de config, sin lanzar `git`).
pub fn has_origin(path: &Path) -> bool {
    gix::open(path).is_ok_and(|r| r.find_remote("origin").is_ok())
}

/// (ahead, behind) respecto al upstream; (0, 0) si no hay. Un solo `rev-list`, sin los extras de `sync_state`.
pub fn ahead_behind(path: &Path) -> (u32, u32) {
    let Ok(out) = git(path, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]) else { return (0, 0) };
    let mut n = out.split_whitespace().map(|x| x.parse().unwrap_or(0));
    let behind = n.next().unwrap_or(0);
    (n.next().unwrap_or(0), behind)
}

pub fn fetch(path: &Path, token: Option<&str>, on: &mut dyn FnMut(&str, u32)) -> Result<Option<i64>> {
    git_net(path, &["fetch", "--prune"], token, on)?;
    last_fetched(path)
}

/// Trae y fusiona la rama remota (como el "Pull origin" de GitHub Desktop: merge, sin abrir editor).
/// Con conflictos git sale con error y deja la fusión a medias: ver `sync_state().conflicts`.
pub fn pull(path: &Path, token: Option<&str>, rebase: bool, on: &mut dyn FnMut(&str, u32)) -> Result<()> {
    // Con rebase, `--autostash` guarda y devuelve tus cambios sin commitear en vez de negarse.
    let args: &[&str] = if rebase { &["pull", "--rebase", "--autostash"] } else { &["pull", "--no-rebase", "--no-edit"] };
    git_net(path, args, token, on)
}

/// Sube la rama actual; si no tiene upstream la publica en `origin` y la enlaza.
/// `force` usa `--force-with-lease`: no pisa commits del remoto que no hayas traído.
pub fn push(path: &Path, token: Option<&str>, force: bool, on: &mut dyn FnMut(&str, u32)) -> Result<()> {
    let mut args = vec!["push"];
    if force {
        args.push("--force-with-lease");
    }
    if !sync_state(path)?.upstream {
        args.extend(["-u", "origin", "HEAD"]);
    }
    git_net(path, &args, token, on)
}

/// `git clone url parent/name`. `parent` debe existir; falla si el destino ya existe.
pub fn clone(parent: &Path, url: &str, name: &str, token: Option<&str>, on: &mut dyn FnMut(&str, u32)) -> Result<std::path::PathBuf> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        bail!("nombre de carpeta no válido");
    }
    if url.starts_with('-') {
        bail!("URL no válida");
    }
    let dest = parent.join(name);
    if dest.exists() {
        bail!("`{}` ya existe", dest.display());
    }
    git_net(parent, &["clone", "--", url, name], token, on)?;
    Ok(dest)
}

/// Descarga el PR `number` (también de forks: `pull/N/head`) y cambia a la rama local `pr-N`.
pub fn checkout_pr(path: &Path, number: u32, token: Option<&str>, on: &mut dyn FnMut(&str, u32)) -> Result<String> {
    let remote_ref = format!("refs/remotes/origin/pr/{number}");
    let local = format!("pr-{number}");
    git_net(path, &["fetch", "origin", &format!("+pull/{number}/head:{remote_ref}")], token, on)?;
    if branches(path)?.contains(&local) {
        git(path, &["switch", "-q", &local])?;
        git(path, &["merge", "--ff-only", "-q", &remote_ref]).context("la rama local pr-N tiene commits propios; resuélvelo a mano")?;
    } else {
        git(path, &["switch", "-q", "-c", &local, &remote_ref])?;
    }
    Ok(local)
}

pub struct SyncState {
    pub has_origin: bool,
    pub upstream: bool,
    pub ahead: u32,
    pub behind: u32,
    /// Operación a medias con conflictos: fusión, cherry-pick o revert.
    pub merging: bool,
    /// Cuál: `merge`, `cherry-pick`, `revert`, `rebase` o vacío.
    pub op: &'static str,
    pub conflicts: Vec<String>,
}

/// Operación que dejó el repo a medias, según los archivos de estado de git.
fn pending_op(git_dir: &Path) -> &'static str {
    [("MERGE_HEAD", "merge"), ("CHERRY_PICK_HEAD", "cherry-pick"), ("REVERT_HEAD", "revert"), ("rebase-merge", "rebase"), ("rebase-apply", "rebase")]
        .into_iter()
        .find(|(f, _)| git_dir.join(f).exists())
        .map_or("", |(_, op)| op)
}

/// Commits por delante/detrás del upstream de la rama actual y estado de fusión.
pub fn sync_state(path: &Path) -> Result<SyncState> {
    let has_origin = git(path, &["remote"])?.lines().any(|r| r == "origin");
    let op = pending_op(gix::open(path)?.git_dir());
    let merging = !op.is_empty();
    let conflicts = if merging {
        git(path, &["diff", "--name-only", "--diff-filter=U"])?.lines().map(String::from).collect()
    } else {
        vec![]
    };
    // Falla si no hay upstream (o HEAD sin commits): se reporta como "sin upstream".
    let Ok(out) = git(path, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]) else {
        return Ok(SyncState { has_origin, upstream: false, ahead: 0, behind: 0, merging, op, conflicts });
    };
    let mut n = out.split_whitespace().map(|x| x.parse().unwrap_or(0));
    let behind = n.next().unwrap_or(0);
    let ahead = n.next().unwrap_or(0);
    Ok(SyncState { has_origin, upstream: true, ahead, behind, merging, op, conflicts })
}

/// Cancela la operación a medias (fusión, cherry-pick o revert) y vuelve al estado previo.
pub fn merge_abort(path: &Path) -> Result<()> {
    match pending_op(gix::open(path)?.git_dir()) {
        "" => bail!("no hay ninguna operación en curso"),
        op => git(path, &[op, "--abort"]).map(drop),
    }
}

/// Avanza la rama actual al upstream solo si es un fast-forward (nunca crea fusiones ni conflictos).
/// `Ok(true)` = avanzó; `Ok(false)` = nada que hacer o no es seguro (adelantada/divergida/sin upstream).
pub fn fast_forward(path: &Path) -> Result<bool> {
    let (ahead, behind) = ahead_behind(path);
    if behind == 0 || ahead > 0 {
        return Ok(false);
    }
    git(path, &["merge", "--ff-only", "@{upstream}"]).map(|_| true)
}

/// mtime de `FETCH_HEAD` (como "Last fetched" de GitHub Desktop).
pub fn last_fetched(path: &Path) -> Result<Option<i64>> {
    let repo = gix::open(path)?;
    let t = std::fs::metadata(repo.common_dir().join("FETCH_HEAD"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    Ok(t)
}

pub struct Worktree {
    pub path: String,
    pub branch: Option<String>,
}

struct WtRaw {
    wt: Worktree,
    /// git ve que su carpeta no existe.
    prunable: bool,
    locked: bool,
}

fn worktree_entries(path: &Path) -> Result<Vec<WtRaw>> {
    let out = git(path, &["worktree", "list", "--porcelain"])?;
    let mut v: Vec<WtRaw> = Vec::new();
    for l in out.lines() {
        if let Some(p) = l.strip_prefix("worktree ") {
            v.push(WtRaw { wt: Worktree { path: p.into(), branch: None }, prunable: false, locked: false });
        } else if let Some(w) = v.last_mut() {
            if let Some(b) = l.strip_prefix("branch refs/heads/") {
                w.wt.branch = Some(b.into());
            } else if l.starts_with("prunable") {
                w.prunable = true;
            } else if l.starts_with("locked") {
                w.locked = true;
            }
        }
    }
    Ok(v)
}

/// Worktrees utilizables: los que git marca como `prunable` (carpeta ausente) no se listan, porque
/// usarlos daría "no es un repositorio git".
pub fn worktrees(path: &Path) -> Result<Vec<Worktree>> {
    Ok(worktree_entries(path)?.into_iter().filter(|w| !w.prunable).map(|w| w.wt).collect())
}

/// ¿Su unidad no está disponible ahora (disco desconectado, volumen expulsado, recurso de red caído)?
/// Entonces la carpeta ausente es temporal y no debe olvidarse sola. Heurística por plataforma:
/// `D:\` o `\\servidor\recurso` (Windows), `/Volumes/x` (macOS), `/mnt/x`, `/media/u/x`, `/run/media/u/x` (Linux).
/// Un punto de montaje que existe pero está vacío cuenta como no montado.
fn volume_gone(p: &Path) -> bool {
    use std::path::Component::{Normal, Prefix, RootDir};
    let mut it = p.components();
    let mount: PathBuf = match it.next() {
        Some(Prefix(pre)) => PathBuf::from(format!("{}\\", pre.as_os_str().to_string_lossy())),
        Some(RootDir) => {
            let parts: Vec<_> = it.filter_map(|c| if let Normal(n) = c { n.to_str() } else { None }).collect();
            let depth = match parts.as_slice() {
                ["Volumes" | "mnt", _, ..] => 2,
                ["media", _, _, ..] => 3,
                ["run", "media", _, _, ..] => 4,
                _ => return false, // carpeta normal del disco del sistema: si falta, se borró
            };
            std::iter::once("/").chain(parts[..depth].iter().copied()).collect()
        }
        _ => return false,
    };
    // En Windows, `D:\` ausente no existe; en Unix, un punto de montaje vacío tampoco está montado.
    !mount.exists() || (!cfg!(windows) && mount.read_dir().is_ok_and(|mut d| d.next().is_none()))
}

/// Worktrees ausentes porque su unidad no está conectada: no se olvidan solos.
pub fn ghost_worktrees(path: &Path) -> Result<Vec<Worktree>> {
    Ok(worktree_entries(path)?.into_iter().filter(|w| w.prunable && volume_gone(Path::new(&w.wt.path))).map(|w| w.wt).collect())
}

fn prune_dead_with(path: &Path, gone: impl Fn(&Path) -> bool) -> Result<()> {
    // Los de unidades ausentes se bloquean mientras dura el prune (git no toca los bloqueados). Los que el
    // usuario ya bloqueó no se tocan ni se desbloquean.
    let keep: Vec<String> = worktree_entries(path)?.into_iter().filter(|w| w.prunable && !w.locked && gone(Path::new(&w.wt.path))).map(|w| w.wt.path).collect();
    let locked: Vec<&String> = keep.iter().filter(|p| git(path, &["worktree", "lock", "--reason", "grove: unidad no disponible", "--", p]).is_ok()).collect();
    let r = git(path, &["worktree", "prune"]).map(drop);
    for p in locked {
        let _ = git(path, &["worktree", "unlock", "--", p]);
    }
    r
}

/// Olvida los worktrees cuya carpeta se borró de verdad (su unidad está disponible).
/// Es solo metadatos y no toca ramas ni commits.
pub fn prune_dead_worktrees(path: &Path) -> Result<()> {
    prune_dead_with(path, volume_gone)
}

/// Olvida TODOS los worktrees ausentes, incluidos los de unidades desconectadas. Solo bajo petición expresa;
/// si el disco vuelve, `git worktree repair` los recupera.
pub fn forget_ghost_worktrees(path: &Path) -> Result<()> {
    git(path, &["worktree", "prune"]).map(drop)
}

/// El id viene de la UI: solo hex, para que nunca se lea como opción de git.
fn check_id(id: &str) -> Result<()> {
    if !(4..=40).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("id de commit inválido");
    }
    Ok(())
}

pub struct CommitInfo {
    pub id: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub summary: String,
    pub body: String,
    pub files: Vec<FileChange>,
    /// Líneas añadidas/borradas en total (binarios no cuentan).
    pub additions: u32,
    pub deletions: u32,
}

/// Detalle de un commit del historial: mensaje completo y archivos tocados.
pub fn commit_info(path: &Path, id: &str) -> Result<CommitInfo> {
    check_id(id)?;
    // Tres `git show` independientes: en paralelo el detalle tarda lo de uno solo.
    let (meta, names, stats) = std::thread::scope(|s| {
        let m = s.spawn(|| git(path, &["show", "-s", "--format=%H%x00%an%x00%ae%x00%at%x00%B", id]));
        let n = s.spawn(|| git(path, &["show", "--format=", "--name-status", "--no-renames", "-m", "--first-parent", "-z", id]));
        let t = s.spawn(|| git(path, &["show", "--format=", "--numstat", "-m", "--first-parent", id]));
        (m.join().unwrap(), n.join().unwrap(), t.join().unwrap())
    });
    let (meta, names, stats) = (meta?, names?, stats?);
    let mut f = meta.splitn(5, '\0');
    let (full, author, email, time, msg) = (f.next(), f.next(), f.next(), f.next(), f.next());
    let msg = msg.unwrap_or("").trim();
    let (summary, body) = msg.split_once('\n').map_or((msg, ""), |(a, b)| (a, b.trim()));
    let mut files = Vec::new();
    let mut it = names.split('\0');
    while let (Some(st), Some(p)) = (it.next(), it.next()) {
        let st = st.trim();
        if st.is_empty() {
            continue;
        }
        let kind = match &st[..1] {
            "A" => "Added",
            "D" => "Removed",
            "T" => "TypeChange",
            _ => "Modified",
        };
        files.push(FileChange { path: p.into(), kind: kind.into() });
    }
    let (mut additions, mut deletions) = (0u32, 0u32);
    for l in stats.lines() {
        let mut c = l.split('\t');
        additions += c.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
        deletions += c.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
    }
    Ok(CommitInfo {
        additions,
        deletions,
        id: full.unwrap_or(id).into(),
        author: author.unwrap_or("").into(),
        email: email.unwrap_or("").into(),
        time: time.and_then(|t| t.parse().ok()).unwrap_or(0),
        summary: summary.into(),
        body: body.into(),
        files,
    })
}

/// Diff de un archivo en un commit (contra su primer padre; el commit raíz, contra vacío).
pub fn commit_diff(path: &Path, id: &str, file: &str) -> Result<String> {
    check_id(id)?;
    let mut d = git(path, &["show", "--format=", "--no-color", "--no-ext-diff", "-m", "--first-parent", id, "--", file])?;
    if d.len() > MAX_DIFF {
        let mut n = MAX_DIFF;
        while !d.is_char_boundary(n) {
            n -= 1;
        }
        d.truncate(n);
        d.push_str("\n@@ diff recortado: archivo demasiado grande @@\n");
    }
    Ok(d)
}

/// Nombre y correo de git (`user.name`/`user.email`) del repo, para el avatar de la caja de commit.
pub fn identity(path: &Path) -> Result<(String, String)> {
    let get = |k: &str| git(path, &["config", "--get", k]).map(|s| s.trim().to_string()).unwrap_or_default();
    Ok((get("user.name"), get("user.email")))
}

/// Versión anterior (HEAD) y actual (disco) de una imagen como data URL, para compararlas.
/// `None` si no existe esa versión. Solo formatos que el webview muestra con `<img>`.
pub fn image_versions(path: &Path, file: &str) -> Result<(Option<String>, Option<String>)> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    const MAX: usize = 10 * 1024 * 1024;
    let ext = file.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => bail!("no es una imagen soportada"),
    };
    let url = |b: &[u8]| (b.len() <= MAX).then(|| format!("data:{mime};base64,{}", STANDARD.encode(b)));
    let old = git_cmd(path, &["show", &format!("HEAD:{file}")]).output().ok().filter(|o| o.status.success());
    // Ruta del usuario: nunca salir del repo (`..`, absolutas).
    let rel = Path::new(file);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        bail!("ruta inválida");
    }
    // Un symlink del repo (p. ej. `logo.png -> ~/.ssh/id_rsa`) no debe leer fuera de él.
    let root = std::fs::canonicalize(path)?;
    let new = std::fs::canonicalize(path.join(rel)).ok().filter(|p| p.starts_with(&root)).and_then(|p| std::fs::read(p).ok());
    Ok((old.and_then(|o| url(&o.stdout)), new.and_then(|b| url(&b))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("grove-git-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let g = |a: &[&str]| git(&d, a).unwrap();
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "t"]);
        g(&["config", "user.email", "t@t"]);
        g(&["config", "commit.gpgsign", "false"]);
        fs::write(d.join("a"), "1\n").unwrap();
        g(&["add", "a"]);
        g(&["commit", "-q", "-m", "x"]);
        g(&["switch", "-q", "-c", "dev"]);
        fs::write(d.join("a"), "2\n").unwrap();
        g(&["commit", "-qam", "y"]);
        g(&["switch", "-q", "main"]);
        d
    }

    fn pick(f: &str) -> Pick {
        Pick { file: f.into(), lines: None }
    }

    #[test]
    fn branches_and_worktrees() {
        let d = repo("br");
        assert_eq!(branches(&d).unwrap(), ["dev", "main"]);
        assert!(switch_branch(&d, "--oops", SwitchMode::Plain).is_err());
        assert_eq!(worktrees(&d).unwrap().len(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn switch_blocked_stash_and_carry() {
        let d = repo("sw");
        fs::write(d.join("a"), "3\n").unwrap(); // choca con dev
        assert!(!switch_branch(&d, "dev", SwitchMode::Plain).unwrap());
        assert_eq!(current_branch(&d).unwrap(), "main");

        // Stash: cambia de rama, árbol limpio, cambios guardados.
        assert!(switch_branch(&d, "dev", SwitchMode::Stash).unwrap());
        assert_eq!(current_branch(&d).unwrap(), "dev");
        assert_eq!(status_count(&d).unwrap(), 0);
        assert!(!git(&d, &["stash", "list"]).unwrap().is_empty());
        git(&d, &["stash", "drop", "-q"]).unwrap();

        // Carry sin conflicto: archivo nuevo viaja a la otra rama.
        fs::write(d.join("b"), "n\n").unwrap();
        assert!(switch_branch(&d, "main", SwitchMode::Carry).unwrap());
        assert!(d.join("b").exists());
        assert!(git(&d, &["stash", "list"]).unwrap().is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn fetch_pull_push_roundtrip() {
        let d = repo("np");
        let remote = std::env::temp_dir().join(format!("grove-git-np-remote-{}", std::process::id()));
        let _ = fs::remove_dir_all(&remote);
        git(&d, &["init", "-q", "--bare", remote.to_str().unwrap()]).unwrap();
        git(&d, &["remote", "add", "origin", remote.to_str().unwrap()]).unwrap();
        let s = sync_state(&d).unwrap();
        assert!(s.has_origin && !s.upstream);
        push(&d, None, false, &mut |_, _| {}).unwrap(); // publica la rama
        let s = sync_state(&d).unwrap();
        assert!(s.upstream && s.ahead == 0 && s.behind == 0);
        fs::write(d.join("z"), "1\n").unwrap();
        git(&d, &["add", "z"]).unwrap();
        git(&d, &["commit", "-q", "-m", "z"]).unwrap();
        assert_eq!(sync_state(&d).unwrap().ahead, 1);
        push(&d, None, false, &mut |_, _| {}).unwrap();
        // segundo clon sube un commit; el primero queda detrás tras fetch y pull lo alcanza
        let c2 = std::env::temp_dir().join(format!("grove-git-np-c2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&c2);
        git(&d, &["clone", "-q", remote.to_str().unwrap(), c2.to_str().unwrap()]).unwrap();
        git(&c2, &["config", "user.email", "t@t"]).unwrap();
        git(&c2, &["config", "user.name", "t"]).unwrap();
        git(&c2, &["config", "commit.gpgsign", "false"]).unwrap();
        git(&c2, &["switch", "-q", "main"]).unwrap();
        fs::write(c2.join("y"), "1\n").unwrap();
        git(&c2, &["add", "y"]).unwrap();
        git(&c2, &["commit", "-q", "-m", "y"]).unwrap();
        git(&c2, &["push", "-q"]).unwrap();
        fetch(&d, None, &mut |_, _| {}).unwrap();
        assert_eq!(sync_state(&d).unwrap().behind, 1);
        pull(&d, None, false, &mut |_, _| {}).unwrap();
        assert!(d.join("y").exists() && sync_state(&d).unwrap().behind == 0);
        for p in [&d, &remote, &c2] { let _ = fs::remove_dir_all(p); }
    }

    #[test]
    fn filtered_log_authors_and_stash() {
        let d = repo("fl");
        git(&d, &["-c", "user.name=Otra", "-c", "user.email=o@o", "commit", "-q", "--allow-empty", "-m", "de otra"]).unwrap();
        let all = log_filtered(&d, 0, 50, &LogFilter::default()).unwrap();
        assert!(all.len() >= 2);
        let f = LogFilter { author: Some("o@o".into()), ..Default::default() };
        let r = log_filtered(&d, 0, 50, &f).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].summary, "de otra");
        let far = LogFilter { until: Some("2000-01-01".into()), ..Default::default() };
        assert!(log_filtered(&d, 0, 50, &far).unwrap().is_empty());
        assert!(log_filtered(&d, 0, 50, &LogFilter { since: Some("x; rm".into()), ..Default::default() }).is_err());
        assert!(authors(&d).unwrap().iter().any(|a| a.1 == "o@o" && a.2 == 1));

        assert!(stash_push(&d, "nada").is_err()); // sin cambios
        fs::write(d.join("a"), "cambio\n").unwrap();
        fs::write(d.join("nuevo"), "n\n").unwrap();
        stash_push(&d, "mi stash").unwrap();
        assert!(!d.join("nuevo").exists());
        let l = stash_list(&d).unwrap();
        assert_eq!(l.len(), 1);
        assert!(l[0].message.contains("mi stash"));
        stash_op(&d, 0, "apply").unwrap();
        assert!(d.join("nuevo").exists() && stash_list(&d).unwrap().len() == 1);
        git(&d, &["checkout", "-q", "--", "."]).unwrap();
        let _ = fs::remove_file(d.join("nuevo"));
        stash_rename(&d, 0, "renombrado").unwrap();
        let l = stash_list(&d).unwrap();
        assert!(l.len() == 1 && l[0].message.contains("renombrado"));
        assert!(stash_rename(&d, 0, "  ").is_err());
        stash_op(&d, 0, "drop").unwrap();
        assert!(stash_list(&d).unwrap().is_empty());
        assert!(stash_op(&d, 0, "rm -rf").is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn stash_contents_and_worktree_add() {
        let d = repo("stwt2");
        fs::write(d.join("a"), "cambio\n").unwrap();
        fs::write(d.join("nuevo"), "n\n").unwrap();
        stash_push(&d, "x").unwrap();
        let f = stash_files(&d, 0).unwrap();
        assert!(f.iter().any(|c| c.path == "a" && c.kind == "Modified"));
        assert!(f.iter().any(|c| c.path == "nuevo" && c.kind == "Added"));
        assert!(stash_diff(&d, 0, "a").unwrap().contains("+cambio"));
        assert!(stash_diff(&d, 0, "nuevo").unwrap().contains("+n"));

        let wt = std::env::temp_dir().join(format!("grove-git-wt-{}", std::process::id()));
        let wt2 = std::env::temp_dir().join(format!("grove-git-wt2-{}", std::process::id()));
        for p in [&wt, &wt2] { let _ = fs::remove_dir_all(p); }
        worktree_add(&d, wt.to_str().unwrap(), "nueva", true).unwrap();
        assert!(worktrees(&d).unwrap().iter().any(|w| w.branch.as_deref() == Some("nueva")));
        assert!(worktree_add(&d, wt2.to_str().unwrap(), "nueva", true).is_err()); // ya existe
        assert!(worktree_add(&d, "relativa", "dev", false).is_err());
        worktree_add(&d, wt2.to_str().unwrap(), "dev", false).unwrap();
        assert!(worktree_remove(&d, d.to_str().unwrap(), false).is_err()); // el principal
        assert!(worktree_remove(&d, "/tmp", false).is_err()); // ajeno
        worktree_remove(&d, wt2.to_str().unwrap(), false).unwrap();
        assert!(!wt2.exists());
        for p in [&d, &wt, &wt2] { let _ = fs::remove_dir_all(p); }
    }

    #[test]
    fn add_origin_validates() {
        let d = repo("ao");
        assert!(add_origin(&d, "--upload-pack=x").is_err());
        assert!(add_origin(&d, "/etc/passwd").is_err());
        add_origin(&d, "https://github.com/o/r.git").unwrap();
        assert!(sync_state(&d).unwrap().has_origin);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("Receiving objects:  45% (9/20), 1.00 MiB"), Some(("Receiving objects", 45)));
        assert_eq!(parse_progress("remote: Compressing objects: 100% (3/3)"), Some(("Compressing objects", 100)));
        assert_eq!(parse_progress("From github.com:o/r"), None);
    }

    #[test]
    fn clone_validates_and_reports_progress() {
        let d = repo("cl");
        let parent = std::env::temp_dir().join(format!("grove-git-clp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).unwrap();
        let src = format!("file://{}", d.display());
        assert!(clone(&parent, "--upload-pack=x", "a", None, &mut |_, _| {}).is_err());
        assert!(clone(&parent, &src, "../x", None, &mut |_, _| {}).is_err());
        let dest = clone(&parent, &src, "copia", None, &mut |_, _| {}).unwrap();
        assert!(dest.join("a").exists());
        assert!(clone(&parent, &src, "copia", None, &mut |_, _| {}).is_err()); // ya existe
        for p in [&d, &parent] { let _ = fs::remove_dir_all(p); }
    }

    #[test]
    fn diverged_push_rejected_then_merge_conflict_abort() {
        let d = repo("cf");
        let remote = std::env::temp_dir().join(format!("grove-git-cf-r-{}", std::process::id()));
        let c2 = std::env::temp_dir().join(format!("grove-git-cf-c-{}", std::process::id()));
        for p in [&remote, &c2] { let _ = fs::remove_dir_all(p); }
        git(&d, &["init", "-q", "--bare", remote.to_str().unwrap()]).unwrap();
        git(&d, &["remote", "add", "origin", remote.to_str().unwrap()]).unwrap();
        push(&d, None, false, &mut |_, _| {}).unwrap();
        git(&d, &["clone", "-q", remote.to_str().unwrap(), c2.to_str().unwrap()]).unwrap();
        for r in [&d, &c2] {
            git(r, &["config", "user.email", "t@t"]).unwrap();
            git(r, &["config", "user.name", "t"]).unwrap();
            git(r, &["config", "commit.gpgsign", "false"]).unwrap();
        }
        git(&c2, &["switch", "-q", "main"]).unwrap();
        fs::write(c2.join("a"), "remoto\n").unwrap();
        git(&c2, &["commit", "-qam", "r"]).unwrap();
        git(&c2, &["push", "-q"]).unwrap();
        fs::write(d.join("a"), "local\n").unwrap();
        git(&d, &["commit", "-qam", "l"]).unwrap();
        let e = push(&d, None, false, &mut |_, _| {}).unwrap_err().to_string();
        assert!(e.contains("rejected"), "{e}");
        assert!(pull(&d, None, false, &mut |_, _| {}).is_err());
        let s = sync_state(&d).unwrap();
        assert!(s.merging && s.conflicts == ["a"]);
        assert!(merge_continue(&d).is_err()); // quedan conflictos
        assert!(resolve_conflict(&d, "a", "otro").is_err());
        resolve_conflict(&d, "a", "theirs").unwrap();
        assert_eq!(fs::read_to_string(d.join("a")).unwrap(), "remoto\n");
        merge_continue(&d).unwrap();
        assert!(!sync_state(&d).unwrap().merging && sync_state(&d).unwrap().ahead == 2);
        // otra fusión a medias para probar abortar
        git(&c2, &["commit", "-q", "--allow-empty", "-m", "otro"]).unwrap();
        fs::write(c2.join("a"), "remoto2\n").unwrap();
        git(&c2, &["commit", "-qam", "r2"]).unwrap();
        git(&c2, &["push", "-q"]).unwrap();
        fs::write(d.join("a"), "local2\n").unwrap();
        git(&d, &["commit", "-qam", "l2"]).unwrap();
        assert!(pull(&d, None, false, &mut |_, _| {}).is_err());
        assert!(sync_state(&d).unwrap().merging);
        merge_abort(&d).unwrap();
        assert!(!sync_state(&d).unwrap().merging);
        // borrar rama remota
        git(&d, &["push", "-q", "origin", "HEAD:refs/heads/tmp"]).unwrap();
        delete_remote_branch(&d, "origin/tmp", None).unwrap();
        assert!(git(&d, &["ls-remote", "--heads", "origin", "tmp"]).unwrap().is_empty());
        assert!(delete_remote_branch(&d, "a b", None).is_err());
        for p in [&d, &remote, &c2] { let _ = fs::remove_dir_all(p); }
    }

    #[test]
    fn slug_from_github_urls() {
        let d = repo("sl");
        for (u, want) in [
            ("https://github.com/o/r.git", Some(("o", "r"))),
            ("git@github.com:o/r.git", Some(("o", "r"))),
            ("ssh://git@github.com/o/r", Some(("o", "r"))),
            ("https://gitlab.com/o/r.git", None),
        ] {
            git(&d, &["config", "remote.origin.url", u]).unwrap();
            assert_eq!(remote_slug(&d), want.map(|(a, b)| (a.to_string(), b.to_string())), "{u}");
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rename_and_delete_branch() {
        let d = repo("rd");
        rename_branch(&d, "dev", "feature/x").unwrap();
        assert!(branches(&d).unwrap().contains(&"feature/x".to_string()));
        assert!(rename_branch(&d, "feature/x", "main").is_err());
        assert!(rename_branch(&d, "feature/x", "a b").is_err());
        git(&d, &["switch", "-q", "main"]).unwrap();
        let cur = current_branch(&d).unwrap();
        assert!(delete_branch(&d, &cur, true).is_err());
        assert!(delete_branch(&d, "feature/x", false).is_err()); // sin fusionar: git se niega
        delete_branch(&d, "feature/x", true).unwrap();
        assert!(!branches(&d).unwrap().contains(&"feature/x".to_string()));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn stash_mode_without_changes_does_not_pop_foreign_stash() {
        let d = repo("ns");
        fs::write(d.join("b"), "n\n").unwrap();
        git(&d, &["stash", "push", "-u", "-m", "ajeno"]).unwrap();
        assert!(switch_branch(&d, "dev", SwitchMode::Carry).unwrap());
        assert!(!git(&d, &["stash", "list"]).unwrap().is_empty()); // sigue ahí
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn diff_and_partial_commit() {
        let d = repo("cm");
        fs::write(d.join("a"), "9\n").unwrap();
        fs::write(d.join("n"), "nuevo\n").unwrap();
        assert!(file_diff(&d, "a").unwrap().contains("+9"));
        assert!(file_diff(&d, "n").unwrap().contains("+nuevo"));
        assert!(commit(&d, &[pick("n")], "  ", "", false).is_err());
        commit(&d, &[pick("n")], "solo n", "cuerpo", false).unwrap();
        let st = status_files(&d).unwrap();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].path, "a"); // a quedó fuera del commit
        assert_eq!(log(&d, 1).unwrap(), ["solo n"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn staged_visible_amend_undo_discard() {
        let d = repo("st");
        // `git add` externo: gix lo vería igual al índice; debe aparecer igual.
        fs::write(d.join("s"), "staged\n").unwrap();
        git(&d, &["add", "s"]).unwrap();
        let st = status_files(&d).unwrap();
        assert!(st.iter().any(|c| c.path == "s" && c.kind == "Added"));

        // amend solo de mensaje
        commit(&d, &[], "nuevo msg", "", true).unwrap();
        assert_eq!(last_message(&d).unwrap().0, "nuevo msg");

        // undo: vuelve a staging y devuelve el mensaje
        fs::write(d.join("t"), "t\n").unwrap();
        commit(&d, &[pick("t")], "con t", "desc", false).unwrap();
        assert_eq!(undo_commit(&d).unwrap(), ("con t".into(), "desc".into()));
        assert!(status_files(&d).unwrap().iter().any(|c| c.path == "t"));

        // discard: tracked vuelve a HEAD, untracked se borra
        fs::write(d.join("a"), "sucio\n").unwrap();
        fs::write(d.join("u"), "u\n").unwrap();
        discard(&d, &["a".into(), "u".into()]).unwrap();
        assert_eq!(fs::read_to_string(d.join("a")).unwrap(), "1\n");
        assert!(!d.join("u").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn not_ignored_filters() {
        let d = repo("ig");
        fs::write(d.join(".gitignore"), "target/\n").unwrap();
        let r = not_ignored(&d, &["target/x.o".into(), "src/a.rs".into()]).unwrap();
        assert_eq!(r, ["src/a.rs"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn undo_refused_when_published() {
        let d = repo("up");
        let remote = std::env::temp_dir().join(format!("grove-git-remote-{}", std::process::id()));
        let _ = fs::remove_dir_all(&remote);
        git(&d, &["init", "-q", "--bare", remote.to_str().unwrap()]).unwrap();
        git(&d, &["remote", "add", "origin", remote.to_str().unwrap()]).unwrap();
        fs::write(d.join("n"), "n\n").unwrap();
        commit(&d, &[pick("n")], "local", "", false).unwrap();
        // aún no publicado: se puede deshacer y rehacer
        undo_commit(&d).unwrap();
        commit(&d, &[pick("n")], "local", "", false).unwrap();
        git(&d, &["push", "-q", "origin", "main"]).unwrap();
        let e = undo_commit(&d).unwrap_err().to_string();
        assert!(e.contains("publicado"), "{e}");
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&remote);
    }

    #[test]
    fn classify_normal_and_linked_worktree() {
        let d = repo("wt");
        let w = WatchDirs { root: d.clone(), git_dir: d.join(".git"), common_dir: d.join(".git") };
        assert!(matches!(w.classify(&d.join(".git/HEAD")), Some(RepoEvent::Meta)));
        assert!(matches!(w.classify(&d.join(".git/refs/heads/x")), Some(RepoEvent::Meta)));
        assert!(w.classify(&d.join(".git/objects/ab/cd")).is_none());
        assert!(matches!(w.classify(&d.join("src/a.rs")), Some(RepoEvent::Work(r)) if r == "src/a.rs"));

        // worktree enlazado real: .git es archivo y git_dir queda fuera de la raíz
        let lw = std::env::temp_dir().join(format!("grove-git-lw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&lw);
        git(&d, &["worktree", "add", "-q", lw.to_str().unwrap(), "dev"]).unwrap();
        let k = watch_dirs(&lw).unwrap();
        assert!(!k.git_dir.starts_with(&k.root));
        assert!(matches!(k.classify(&k.git_dir.join("HEAD")), Some(RepoEvent::Meta)));
        assert!(matches!(k.classify(&k.common_dir.join("refs/heads/dev")), Some(RepoEvent::Meta)));
        assert!(k.classify(&k.root.join(".git")).is_none());
        assert!(matches!(k.classify(&k.root.join("a")), Some(RepoEvent::Work(_))));
        // symlinks: la raíz canonicalizada coincide con rutas resueltas
        assert_eq!(k.root, std::fs::canonicalize(&lw).unwrap());
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&lw);
    }

    #[test]
    fn line_level_commit() {
        let d = repo("ln");
        let orig: String = (1..=12).map(|i| format!("l{i}\n")).collect();
        fs::write(d.join("f"), &orig).unwrap();
        git(&d, &["add", "f"]).unwrap();
        git(&d, &["commit", "-qm", "f"]).unwrap();
        // dos cambios lejanos (dos hunks): l2→X, y se borra l11; se añade Z tras l7
        let new = orig.replace("l2\n", "X\n").replace("l11\n", "").replace("l7\n", "l7\nZ\n");
        fs::write(d.join("f"), &new).unwrap();
        let diff = file_diff(&d, "f").unwrap();
        let idx = |needle: &str| diff.split('\n').position(|l| l == needle).unwrap() as u32;
        // elegir solo X (y su `-l2`); dejar fuera Z y el borrado de l11
        let sel = vec![idx("-l2"), idx("+X")];
        commit(&d, &[Pick { file: "f".into(), lines: Some(sel) }], "parcial", "", false).unwrap();
        let committed = git(&d, &["show", "HEAD:f"]).unwrap();
        assert!(committed.contains("X\n") && !committed.contains("l2\n"));
        assert!(committed.contains("l11\n") && !committed.contains('Z'));
        // lo no elegido sigue como cambio local, el archivo del disco no se tocó
        assert_eq!(fs::read_to_string(d.join("f")).unwrap(), new);
        let rest = file_diff(&d, "f").unwrap();
        assert!(rest.contains("+Z") && rest.contains("-l11") && !rest.contains("+X"));
        // selección imposible: error claro, no commit vacío
        assert!(commit(&d, &[Pick { file: "f".into(), lines: Some(vec![9999]) }], "x", "", false).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn first_commit_on_unborn_head() {
        let d = std::env::temp_dir().join(format!("grove-git-ub-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        for a in [&["init", "-q", "-b", "main"][..], &["config", "user.name", "t"], &["config", "user.email", "t@t"], &["config", "commit.gpgsign", "false"]] {
            git(&d, a).unwrap();
        }
        fs::write(d.join("a"), "1\n").unwrap();
        fs::write(d.join("b"), "2\n").unwrap();
        git(&d, &["add", "b"]).unwrap(); // en staging pero sin marcar
        commit(&d, &[pick("a")], "primero", "", false).unwrap();
        assert_eq!(git(&d, &["show", "--name-only", "--format=", "HEAD"]).unwrap().trim(), "a");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn image_versions_ok_and_rejects_escape() {
        let d = repo("im");
        fs::write(d.join("i.png"), [137, 80, 78, 71]).unwrap();
        let (old, new) = image_versions(&d, "i.png").unwrap();
        assert!(old.is_none() && new.unwrap().starts_with("data:image/png;base64,"));
        assert!(image_versions(&d, "../secreto.png").is_err());
        assert!(image_versions(&d, "a").is_err()); // no es imagen
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn history_details_and_paging() {
        let d = repo("hs");
        fs::write(d.join("n"), "x\n").unwrap();
        commit(&d, &[pick("n")], "tercero", "cuerpo largo", false).unwrap();
        let all = log_entries(&d, 0, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(log_entries(&d, 1, 10).unwrap()[0].summary, "x"); // paginado
        let info = commit_info(&d, &all[0].id).unwrap();
        assert_eq!((info.summary.as_str(), info.body.as_str()), ("tercero", "cuerpo largo"));
        assert_eq!(info.files.len(), 1);
        assert_eq!((info.additions, info.deletions), (1, 0));
        assert_eq!((info.files[0].path.as_str(), info.files[0].kind.as_str()), ("n", "Added"));
        assert!(commit_diff(&d, &all[0].id, "n").unwrap().contains("+x"));
        // commit raíz: todo es "Added"
        let root = commit_info(&d, &all[1].id).unwrap();
        assert_eq!(root.files[0].kind, "Added");
        assert!(commit_diff(&d, "--output=/tmp/x", "n").is_err());
        assert_eq!(identity(&d).unwrap().0, "t");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn owner_parsing() {
        assert_eq!(parse_owner("https://github.com/Maikol/web.git").as_deref(), Some("Maikol"));
        assert_eq!(parse_owner("git@github.com:octocat/hello.git").as_deref(), Some("octocat"));
        assert_eq!(parse_owner("ssh://git@gitlab.com/grp/sub/repo").as_deref(), Some("sub"));
        assert_eq!(parse_owner("/ruta/local/repo"), None);
        assert_eq!(parse_owner("repo"), None);
    }

    #[test]
    fn branch_list_create_and_remote_switch() {
        let d = repo("bl");
        let l = branch_list(&d).unwrap();
        assert!(l.iter().any(|b| b.name == "dev" && !b.remote && b.time > 0));
        create_branch(&d, "feature/x").unwrap();
        assert_eq!(current_branch(&d).unwrap(), "feature/x");
        assert!(create_branch(&d, "feature/x").is_err()); // duplicada
        for bad in ["", "a b", "-x", "a..b"] {
            assert!(create_branch(&d, bad).is_err(), "{bad}");
        }
        // remota sin copia local: se crea siguiéndola
        let remote = std::env::temp_dir().join(format!("grove-git-rb-{}", std::process::id()));
        let _ = fs::remove_dir_all(&remote);
        git(&d, &["init", "-q", "--bare", remote.to_str().unwrap()]).unwrap();
        git(&d, &["remote", "add", "origin", remote.to_str().unwrap()]).unwrap();
        git(&d, &["push", "-q", "origin", "dev"]).unwrap();
        git(&d, &["branch", "-D", "dev"]).unwrap();
        git(&d, &["fetch", "-q"]).unwrap();
        assert!(branch_list(&d).unwrap().iter().any(|b| b.name == "origin/dev" && b.remote));
        assert!(switch_branch(&d, "origin/dev", SwitchMode::Plain).unwrap());
        assert_eq!(current_branch(&d).unwrap(), "dev");
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&remote);
    }
}

/// Resumen de los cambios de `files` para pedirle a una IA un mensaje de commit, sin pasar de `budget` caracteres.
/// Siempre lista todos los archivos con +/-; el diff solo entra de los más pequeños primero y recortado
/// (lockfiles, binarios y minificados van solo en la lista). Lo que no cabe queda como "y otros N archivos".
pub fn ai_digest(path: &Path, files: &[String], budget: usize) -> Result<String> {
    const PER_FILE: usize = 1500;
    const MAX_LISTED: usize = 40;
    struct E { file: String, add: usize, del: usize, diff: Option<String> }
    let mut es = Vec::new();
    for f in files {
        let d = file_diff(path, f).unwrap_or_default();
        let bin = d.contains("Binary files") || d.contains("GIT binary patch");
        // Secretos habituales: el diff saldría hacia un proveedor externo; solo van en la lista.
        let name = f.rsplit('/').next().unwrap_or(f).to_ascii_lowercase();
        let secret = name.starts_with(".env")
            || name.starts_with("id_rsa")
            || name.starts_with("id_ed25519")
            || [".npmrc", ".netrc", ".pypirc", "credentials"].contains(&name.as_str())
            || [".pem", ".key", ".p12", ".pfx", ".jks", ".keystore"].iter().any(|e| name.ends_with(e));
        let noisy = bin
            || secret
            || f.ends_with(".lock")
            || f.ends_with("-lock.json")
            || f.ends_with("lock.yaml")
            || f.ends_with(".min.js")
            || f.ends_with(".map");
        let add = d.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
        let del = d.lines().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();
        es.push(E { file: f.clone(), add, del, diff: (!noisy).then_some(d) });
    }
    es.sort_by_key(|e| e.add + e.del);
    let mut out = String::new();
    let mut hidden = 0usize;
    for e in es.iter().take(MAX_LISTED) {
        out.push_str(&format!("{} (+{} -{})\n", e.file, e.add, e.del));
    }
    if es.len() > MAX_LISTED {
        let (a, d) = es[MAX_LISTED..].iter().fold((0, 0), |s, e| (s.0 + e.add, s.1 + e.del));
        out.push_str(&format!("y otros {} archivos (+{a} -{d})\n", es.len() - MAX_LISTED));
    }
    out.push('\n');
    for e in &es {
        let Some(d) = &e.diff else { continue };
        // Sin encabezados `diff --git`/`index`: solo hunks, que es lo que informa.
        let body: String = d.lines().skip_while(|l| !l.starts_with("@@")).collect::<Vec<_>>().join("\n");
        let mut n = body.len().min(PER_FILE);
        while !body.is_char_boundary(n) {
            n -= 1;
        }
        let chunk = format!("### {}\n{}{}\n", e.file, &body[..n], if n < body.len() { "\n…" } else { "" });
        if out.len() + chunk.len() > budget {
            hidden += 1;
            continue;
        }
        out.push_str(&chunk);
    }
    if hidden > 0 {
        out.push_str(&format!("\n(diff omitido de otros {hidden} archivos por tamaño)\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod ai_digest_tests {
    use super::*;

    #[test]
    fn digest_respeta_presupuesto_y_lista_todo() {
        let d = std::env::temp_dir().join(format!("grove-aidg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        git(&d, &["init", "-q"]).unwrap();
        let mut names = Vec::new();
        for i in 0..60 {
            let n = format!("f{i}.txt");
            std::fs::write(d.join(&n), "linea\n".repeat(50)).unwrap();
            names.push(n);
        }
        std::fs::write(d.join("a.lock"), "x\n".repeat(500)).unwrap();
        names.push("a.lock".into());
        std::fs::write(d.join(".env"), "API_KEY=supersecreto\n").unwrap();
        names.insert(0, ".env".into());
        let out = ai_digest(&d, &names, 4000).unwrap();
        assert!(!out.contains("supersecreto"));
        assert!(out.len() < 4000 + 400, "len {}", out.len());
        assert!(out.contains("y otros 22 archivos"), "{out}");
        assert!(!out.contains("### a.lock"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// Datos de un vistazo para el diálogo «Información» de un repo.
pub struct RepoInfo {
    pub branch: String,
    pub remote_url: Option<String>,
    pub commits: u32,
    /// "abc1234 resumen" del último commit; vacío si aún no hay.
    pub last_commit: String,
    pub last_commit_time: Option<i64>,
    pub last_author: String,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub changes: u32,
    pub stashes: u32,
    pub branches: u32,
}

pub fn repo_info(path: &Path) -> Result<RepoInfo> {
    let num = |args: &[&str]| git(path, args).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    let last = git(path, &["log", "-1", "--format=%h %s%n%ct%n%an"]).unwrap_or_default();
    let mut l = last.lines();
    let last_commit = l.next().unwrap_or("").to_string();
    let time = l.next().and_then(|t| t.trim().parse().ok());
    let last_author = l.next().unwrap_or("").to_string();
    Ok(RepoInfo {
        branch: current_branch(path)?,
        remote_url: git(path, &["remote", "get-url", "origin"]).ok().map(|u| hide_credentials(u.trim())).filter(|u| !u.is_empty()),
        commits: num(&["rev-list", "--count", "HEAD"]),
        last_commit,
        last_commit_time: time,
        last_author,
        owner: remote_owner(path),
        repo: remote_repo(path),
        changes: status_count(path).unwrap_or(0) as u32,
        stashes: stash_list(path).map(|s| s.len() as u32).unwrap_or(0),
        branches: git(path, &["for-each-ref", "--format=x", "refs/heads"]).map(|s| s.lines().count() as u32).unwrap_or(0),
    })
}

/// `https://usuario:token@host/..` → `https://host/..` (nunca mostrar credenciales en pantalla).
fn hide_credentials(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return url.into() };
    let end = rest.find('/').unwrap_or(rest.len());
    match rest[..end].rfind('@') {
        Some(i) => format!("{scheme}://{}", &rest[i + 1..]),
        None => url.into(),
    }
}

#[cfg(test)]
mod repo_info_tests {
    use super::hide_credentials;

    #[test]
    fn oculta_credenciales() {
        assert_eq!(hide_credentials("https://u:tok@github.com/a/b.git"), "https://github.com/a/b.git");
        assert_eq!(hide_credentials("git@github.com:a/b.git"), "git@github.com:a/b.git");
        assert_eq!(hide_credentials("https://github.com/a/b"), "https://github.com/a/b");
        assert_eq!(hide_credentials("https://u:tok@host.com"), "https://host.com");
    }
}

#[cfg(test)]
mod remote_repo_tests {
    #[test]
    fn nombre_del_remoto() {
        let d = std::env::temp_dir().join(format!("grove-rr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        super::git(&d, &["init", "-q"]).unwrap();
        assert_eq!(super::remote_repo(&d), None);
        super::git(&d, &["remote", "add", "origin", "git@github.com:Acme/mi-repo.git"]).unwrap();
        assert_eq!(super::remote_repo(&d).as_deref(), Some("mi-repo"));
        super::git(&d, &["remote", "set-url", "origin", "https://github.com/Acme/otro/"]).unwrap();
        assert_eq!(super::remote_repo(&d).as_deref(), Some("otro"));
        assert_eq!(super::remote_slug(&d), Some(("Acme".into(), "otro".into())));
        for bad in ["https://github.com/x/../../user/following/y?", "https://github.com/x/y/collaborators/z", "git@github.com:../y.git"] {
            super::git(&d, &["remote", "set-url", "origin", bad]).unwrap();
            assert_eq!(super::remote_slug(&d), None, "{bad}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// Añade `/<archivo>` a `.gitignore` (lo crea si falta; no duplica). No des-versiona archivos ya rastreados.
pub fn ignore_file(path: &Path, file: &str) -> Result<()> {
    if file.is_empty() || file.contains(['\n', '\r']) || file.split(['/', '\\']).any(|c| c == "..") {
        bail!("ruta de archivo no válida");
    }
    // Un `#` o `!` inicial tendría significado especial en .gitignore.
    let esc = if file.starts_with(['#', '!']) { format!("\\{file}") } else { file.to_string() };
    append_ignore_line(&path.join(".gitignore"), &format!("/{esc}"))
}

/// Añade una línea a un archivo de ignore (lo crea si falta; no duplica).
fn append_ignore_line(p: &Path, line: &str) -> Result<()> {
    let cur = std::fs::read_to_string(p).unwrap_or_default();
    if cur.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    let mut out = cur;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, out)?;
    Ok(())
}

/// Pone `.DS_Store` en el ignore global de Git del usuario (`core.excludesFile`, o `~/.config/git/ignore`):
/// nunca aparece como cambio ni se sube, en ningún repo. No toca los `.gitignore` de los proyectos.
pub fn ignore_ds_store_globally() -> Result<()> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).context("sin HOME")?;
    let configured = Command::new("git").args(["config", "--global", "--get", "core.excludesFile"]).output().ok().filter(|o| o.status.success());
    let file = match configured.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()) {
        Some(f) => match f.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => std::path::PathBuf::from(f),
        },
        None => std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| home.join(".config")).join("git/ignore"),
    };
    append_ignore_line(&file, ".DS_Store")
}

#[cfg(test)]
mod ignore_tests {
    #[test]
    fn ignora_sin_duplicar() {
        let d = std::env::temp_dir().join(format!("grove-ign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".gitignore"), "target").unwrap(); // sin salto final
        super::ignore_file(&d, ".DS_Store").unwrap();
        super::ignore_file(&d, ".DS_Store").unwrap();
        super::ignore_file(&d, "#nota").unwrap();
        assert_eq!(std::fs::read_to_string(d.join(".gitignore")).unwrap(), "target\n/.DS_Store\n/\\#nota\n");
        assert!(super::ignore_file(&d, "../x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod fetch_quiet_tests {
    use super::*;
    use std::time::Duration;

    fn sh(dir: &Path, args: &[&str]) {
        assert!(git_cmd(dir, args).status().unwrap().success(), "{args:?}");
    }

    #[test]
    fn fetch_quiet_y_ahead_behind() {
        let t = std::env::temp_dir().join(format!("grove-fq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let (up, a, b) = (t.join("up"), t.join("a"), t.join("b"));
        std::fs::create_dir(&up).unwrap();
        sh(&up, &["init", "-q", "-b", "main"]);
        sh(&up, &["config", "receive.denyCurrentBranch", "ignore"]); // push de prueba a un repo no bare
        sh(&up, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "1"]);
        sh(&t, &["clone", "-q", up.to_str().unwrap(), "a"]);
        sh(&t, &["clone", "-q", up.to_str().unwrap(), "b"]);
        assert!(has_origin(&a));
        sh(&b, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "2"]);
        sh(&b, &["push", "-q"]);
        assert_eq!(ahead_behind(&a), (0, 0)); // sin fetch no se ve lo nuevo
        fetch_quiet(&a, None, Duration::from_secs(30)).unwrap();
        assert_eq!(ahead_behind(&a), (0, 1));
        assert!(fetch_quiet(&t.join("nope"), None, Duration::from_secs(5)).is_err());
        let _ = std::fs::remove_dir_all(&t);
    }
}

#[cfg(test)]
mod ops_tests {
    use super::*;

    fn sh(d: &Path, a: &[&str]) {
        let mut v = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
        v.extend(a);
        assert!(git_cmd(d, &v).status().unwrap().success(), "{a:?}");
    }
    fn write(d: &Path, f: &str, c: &str) {
        std::fs::write(d.join(f), c).unwrap();
    }

    #[test]
    fn merge_revert_cherry_pick() {
        let d = std::env::temp_dir().join(format!("grove-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        sh(&d, &["init", "-q", "-b", "main"]);
        write(&d, "a", "1\n");
        sh(&d, &["add", "."]);
        sh(&d, &["commit", "-qm", "base"]);
        sh(&d, &["switch", "-qc", "x"]);
        write(&d, "a", "x\n");
        sh(&d, &["commit", "-qam", "en x"]);
        sh(&d, &["switch", "-q", "main"]);
        write(&d, "a", "m\n");
        sh(&d, &["commit", "-qam", "en main"]);

        // merge con conflicto -> queda a medias -> se resuelve y concluye
        assert_eq!(merge_branch(&d, "x").unwrap(), Outcome::Conflicts);
        let s = sync_state(&d).unwrap();
        assert_eq!((s.op, s.conflicts.as_slice()), ("merge", &["a".to_string()][..]));
        resolve_conflict(&d, "a", "theirs").unwrap();
        assert!(merge_continue(&d).unwrap());
        assert!(!sync_state(&d).unwrap().merging);
        assert!(merge_branch(&d, "nope").is_err());

        // revert del merge (usa -m 1) y cherry-pick con conflicto + abortar
        let head = git(&d, &["rev-parse", "--short", "HEAD"]).unwrap();
        assert_eq!(revert_commit(&d, head.trim()).unwrap(), Outcome::Done);
        assert!(revert_commit(&d, "--oops").is_err());
        sh(&d, &["switch", "-qc", "y", "HEAD~2"]);
        write(&d, "a", "y\n");
        sh(&d, &["commit", "-qam", "en y"]);
        let x = git(&d, &["rev-parse", "--short", "x"]).unwrap();
        assert_eq!(cherry_pick(&d, x.trim()).unwrap(), Outcome::Conflicts);
        assert_eq!(sync_state(&d).unwrap().op, "cherry-pick");
        merge_abort(&d).unwrap();
        assert!(!sync_state(&d).unwrap().merging);
        // Conflicto resuelto quedándote con lo que ya había: queda vacío y se concluye igual (skip).
        assert_eq!(cherry_pick(&d, x.trim()).unwrap(), Outcome::Conflicts);
        resolve_conflict(&d, "a", "ours").unwrap();
        assert!(!merge_continue(&d).unwrap()); // vacío: omitido
        assert!(!sync_state(&d).unwrap().merging);
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// Tag y el commit al que apunta (el commit real también en tags anotados).
pub struct Tag {
    pub name: String,
    pub id: String,
}

pub fn tags(path: &Path) -> Result<Vec<Tag>> {
    let out = git(path, &["for-each-ref", "--format=%(refname:short)%00%(objectname:short=7)%00%(*objectname:short=7)", "refs/tags"])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\0');
            let (name, obj, target) = (f.next()?, f.next()?, f.next().unwrap_or(""));
            Some(Tag { name: name.into(), id: if target.is_empty() { obj } else { target }.into() })
        })
        .collect())
}

/// Crea un tag sobre `id`; con `message` es anotado. Con `push` lo sube a `origin`.
pub fn create_tag(path: &Path, id: &str, name: &str, message: &str, push: bool, token: Option<&str>) -> Result<()> {
    valid_rev(id)?;
    let name = name.trim();
    if name.starts_with('-') || git(path, &["check-ref-format", &format!("refs/tags/{name}")]).is_err() {
        bail!("`{name}` no es un nombre de tag válido");
    }
    if tags(path)?.iter().any(|t| t.name == name) {
        bail!("ya existe el tag `{name}`");
    }
    let msg = message.trim();
    if msg.is_empty() {
        git(path, &["tag", name, id])?;
    } else {
        git(path, &["tag", "-a", "-m", msg, name, id])?;
    }
    if push {
        git_net(path, &["push", "origin", &format!("refs/tags/{name}")], token, &mut |_, _| {})?;
    }
    Ok(())
}

/// Borra un tag local; con `remote` también en `origin`.
pub fn delete_tag(path: &Path, name: &str, remote: bool, token: Option<&str>) -> Result<()> {
    if name.starts_with('-') {
        bail!("nombre de tag no válido");
    }
    git(path, &["tag", "-d", name])?;
    if remote {
        git_net(path, &["push", "origin", "--delete", &format!("refs/tags/{name}")], token, &mut |_, _| {})?;
    }
    Ok(())
}

#[cfg(test)]
mod tags_log_tests {
    use super::*;

    #[test]
    fn tags_y_filtros() {
        let d = std::env::temp_dir().join(format!("grove-tags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let sh = |a: &[&str]| {
            let mut v = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
            v.extend(a);
            assert!(git_cmd(&d, &v).status().unwrap().success(), "{a:?}");
        };
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(d.join("a"), "1").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "Primero ARREGLO"]);
        std::fs::write(d.join("b"), "contenido distinto y más largo\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "Segundo"]);
        let ids = log_entries(&d, 0, 10).unwrap();
        create_tag(&d, &ids[1].id, "v1", "", false, None).unwrap();
        create_tag(&d, &ids[0].id, "v2", "versión 2", false, None).unwrap();
        let mut t = tags(&d).unwrap();
        t.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!((t[0].name.as_str(), t[0].id.as_str()), ("v1", ids[1].id.as_str()));
        assert_eq!(t[1].id, ids[0].id); // anotado: apunta al commit, no al objeto tag
        assert!(create_tag(&d, &ids[0].id, "v1", "", false, None).is_err());
        assert!(create_tag(&d, &ids[0].id, "-x", "", false, None).is_err());
        assert!(create_tag(&d, &ids[0].id, "mal nombre", "", false, None).is_err());
        let f = |search: Option<&str>, file: Option<&str>| {
            log_filtered(&d, 0, 10, &LogFilter { search: search.map(Into::into), file: file.map(Into::into), ..Default::default() }).unwrap().len()
        };
        assert_eq!(f(Some("arreglo"), None), 1);
        assert_eq!(f(None, Some("b")), 1);
        assert_eq!(f(None, None), 2);
        // historial de otra rama: completo y solo lo que falta en la actual
        sh(&["switch", "-qc", "x", "HEAD~1"]);
        std::fs::write(d.join("c"), "otra cosa totalmente distinta\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "extra en x"]);
        sh(&["switch", "-q", "main"]);
        let r = |only_new| log_filtered(&d, 0, 10, &LogFilter { rev: Some("x".into()), only_new, ..Default::default() }).unwrap();
        assert_eq!((r(false).len(), r(true).len()), (2, 1));
        assert_eq!(r(true)[0].summary, "extra en x");
        assert!(log_filtered(&d, 0, 10, &LogFilter { rev: Some("--all".into()), ..Default::default() }).is_err());
        delete_tag(&d, "v1", false, None).unwrap();
        assert_eq!(tags(&d).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod worktree_tests {
    use super::*;

    #[test]
    fn worktrees_sin_carpeta_no_se_listan() {
        let d = std::env::temp_dir().join(format!("grove-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let main = d.join("main");
        std::fs::create_dir_all(&main).unwrap();
        let sh = |dir: &Path, a: &[&str]| {
            let mut v = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
            v.extend(a);
            assert!(git_cmd(dir, &v).status().unwrap().success(), "{a:?}");
        };
        sh(&main, &["init", "-q", "-b", "main"]);
        sh(&main, &["commit", "-q", "--allow-empty", "-m", "1"]);
        let wt = d.join("wt");
        sh(&main, &["worktree", "add", "-q", "-b", "x", wt.to_str().unwrap()]);
        assert_eq!(worktrees(&main).unwrap().len(), 2);
        std::fs::remove_dir_all(&wt).unwrap();
        let l = worktrees(&main).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].branch.as_deref(), Some("main"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// Rama local cuya rama remota ya no existe.
pub struct Stale {
    pub name: String,
    /// Sus commits ya están en otra rama: borrarla no pierde nada.
    pub merged: bool,
}

/// ¿Algún otro ref (local o remoto) contiene el commit de la rama `name`?
fn contained_elsewhere(path: &Path, name: &str) -> Result<bool> {
    let own = format!("refs/heads/{name}");
    let out = git(path, &["for-each-ref", "--contains", &own, "--format=%(refname)", "refs/heads", "refs/remotes"])?;
    Ok(out.lines().any(|r| r != own && !r.ends_with("/HEAD")))
}

/// Ramas locales con upstream eliminado (`[gone]`), sin contar la actual ni las abiertas en un worktree.
/// Antes olvida los worktrees cuya carpeta se borró de verdad (no los de una unidad desconectada), porque
/// una rama "en uso" por uno de ellos no se podría borrar.
pub fn stale_branches(path: &Path) -> Result<Vec<Stale>> {
    let _ = prune_dead_worktrees(path);
    let cur = current_branch(path).unwrap_or_default();
    // Incluye los de unidades ausentes: git tampoco dejaría borrar esas ramas.
    let in_use: Vec<String> = worktree_entries(path)?.into_iter().filter_map(|w| w.wt.branch).collect();
    let out = git(path, &["for-each-ref", "--format=%(refname:short)%00%(upstream:track)", "refs/heads"])?;
    let mut v = Vec::new();
    for l in out.lines() {
        let Some((name, track)) = l.split_once('\0') else { continue };
        if track == "[gone]" && name != cur && !in_use.iter().any(|b| b == name) {
            v.push(Stale { name: name.into(), merged: contained_elsewhere(path, name)? });
        }
    }
    Ok(v)
}

/// Borra ramas obsoletas. Vuelve a comprobar cada una (la UI no puede colar otras) y usa `-D` porque
/// `-d` se niega a borrar ramas cuyo upstream ya no existe. Devuelve cuántas borró.
pub fn clean_branches(path: &Path, names: &[String]) -> Result<u32> {
    let stale = stale_branches(path)?;
    let (mut n, mut failed) = (0, Vec::new());
    for name in names {
        if !stale.iter().any(|s| &s.name == name) {
            failed.push(format!("{name} (ya no es obsoleta)"));
        } else if git(path, &["branch", "-D", "--", name]).is_ok() {
            n += 1;
        } else {
            failed.push(name.clone());
        }
    }
    if !failed.is_empty() {
        bail!("no se pudo borrar: {}", failed.join(", "));
    }
    Ok(n)
}

#[cfg(test)]
mod stale_tests {
    use super::*;

    #[test]
    fn ramas_obsoletas() {
        let t = std::env::temp_dir().join(format!("grove-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        let sh = |d: &Path, a: &[&str]| {
            let mut v = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
            v.extend(a);
            assert!(git_cmd(d, &v).status().unwrap().success(), "{a:?}");
        };
        let (up, a) = (t.join("up.git"), t.join("a"));
        std::fs::create_dir(&up).unwrap();
        sh(&up, &["init", "-q", "--bare", "-b", "main"]);
        sh(&t, &["clone", "-q", up.to_str().unwrap(), "a"]);
        sh(&a, &["commit", "-q", "--allow-empty", "-m", "base"]);
        sh(&a, &["push", "-q", "-u", "origin", "main"]);
        // m: igual que main (fusionada). u: con un commit propio que solo existe ahí.
        sh(&a, &["branch", "m"]);
        sh(&a, &["switch", "-qc", "u"]);
        sh(&a, &["commit", "-q", "--allow-empty", "-m", "solo en u"]);
        sh(&a, &["push", "-q", "-u", "origin", "m", "u"]);
        sh(&a, &["switch", "-q", "main"]);
        assert!(stale_branches(&a).unwrap().is_empty());
        sh(&up, &["branch", "-D", "m", "u"]);
        sh(&a, &["fetch", "-q", "--prune"]);
        let mut s = stale_branches(&a).unwrap();
        s.sort_by(|x, y| x.name.cmp(&y.name));
        assert_eq!(s.iter().map(|x| (x.name.as_str(), x.merged)).collect::<Vec<_>>(), [("m", true), ("u", false)]);
        // la actual y lo que no es obsoleto no se borran
        assert!(clean_branches(&a, &["main".into()]).is_err());
        assert_eq!(clean_branches(&a, &["m".into(), "u".into()]).unwrap(), 2);
        assert!(branches(&a).unwrap().iter().all(|b| b == "main" || b.starts_with("origin/")));
        let _ = std::fs::remove_dir_all(&t);
    }
}

#[cfg(test)]
mod ghost_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn volumenes_ausentes() {
        assert!(volume_gone(Path::new("/Volumes/__grove_no_existe__/x/y")));
        assert!(volume_gone(Path::new("/mnt/__grove_no_existe__/proyecto")));
        assert!(volume_gone(Path::new("/media/u/__grove_no_existe__/p")));
        assert!(!volume_gone(Path::new("/tmp/borrada/a")));
        assert!(!volume_gone(Path::new("/Users/yo/proyecto")));
        assert!(!volume_gone(Path::new("/Volumes"))); // sin nombre de volumen: no es un punto de montaje
    }

    #[test]
    fn prune_respeta_unidades_ausentes() {
        let d = std::env::temp_dir().join(format!("grove-ghost-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let main = d.join("main");
        std::fs::create_dir_all(&main).unwrap();
        let sh = |a: &[&str]| {
            let mut v = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
            v.extend(a);
            assert!(git_cmd(&main, &v).status().unwrap().success(), "{a:?}");
        };
        sh(&["init", "-q", "-b", "main"]);
        sh(&["commit", "-q", "--allow-empty", "-m", "1"]);
        let (dead, away) = (d.join("wt_muerto"), d.join("wt_disco"));
        sh(&["worktree", "add", "-q", "-b", "a", dead.to_str().unwrap()]);
        sh(&["worktree", "add", "-q", "-b", "b", away.to_str().unwrap()]);
        std::fs::remove_dir_all(&dead).unwrap();
        std::fs::remove_dir_all(&away).unwrap();
        // simula que `wt_disco` está en una unidad desconectada
        prune_dead_with(&main, |p| p.ends_with("wt_disco")).unwrap();
        let raw = worktree_entries(&main).unwrap();
        assert_eq!(raw.len(), 2, "queda el principal y el de la unidad ausente");
        let kept = raw.iter().find(|w| w.wt.path.ends_with("wt_disco")).unwrap();
        assert!(kept.prunable && !kept.locked, "queda ausente pero sin bloqueo residual");
        // su rama no se ofrece como obsoleta aunque su remota no exista
        assert!(worktree_entries(&main).unwrap().iter().any(|w| w.wt.branch.as_deref() == Some("b")));
        // bajo petición expresa sí se olvida
        forget_ghost_worktrees(&main).unwrap();
        assert_eq!(worktree_entries(&main).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// Entrada del reflog de HEAD: a dónde apuntó HEAD y por qué (commit, checkout, reset, merge...).
pub struct ReflogEntry {
    pub id: String,
    /// Asunto del commit al que apuntaba.
    pub summary: String,
    /// Acción que movió HEAD (`commit: ...`, `checkout: moving from a to b`, `reset: ...`).
    pub action: String,
    pub time: i64,
}

/// Últimos `limit` movimientos de HEAD, el más reciente primero. Es la red de seguridad para recuperar
/// commits o ramas tras un reset, un borrado o una operación abortada.
pub fn reflog(path: &Path, limit: usize) -> Result<Vec<ReflogEntry>> {
    let n = format!("-n{limit}");
    let out = git(path, &["reflog", "show", "--abbrev=7", "--format=%h%x00%s%x00%gs%x00%ct%x1e", &n, "HEAD"]).unwrap_or_default();
    Ok(out
        .split('\x1e')
        .filter_map(|r| {
            let mut c = r.trim_start_matches('\n').split('\0');
            Some(ReflogEntry {
                id: c.next().filter(|x| !x.is_empty())?.into(),
                summary: c.next()?.into(),
                action: c.next()?.into(),
                time: c.next()?.trim().parse().ok()?,
            })
        })
        .collect())
}

/// Crea la rama `name` en `rev` (un commit). Con `switch` cambia a ella.
pub fn branch_at(path: &Path, name: &str, rev: &str, switch: bool) -> Result<()> {
    valid_rev(rev)?;
    let name = name.trim();
    valid_branch(path, name)?;
    if branches(path)?.iter().any(|b| b == name) {
        bail!("ya existe la rama `{name}`");
    }
    if switch {
        git(path, &["switch", "-c", name, rev]).map(drop)
    } else {
        git(path, &["branch", name, rev]).map(drop)
    }
}

/// Commits que se perderían de la rama al volver a `rev` (los que están después).
pub fn commits_since(path: &Path, rev: &str) -> Result<u32> {
    valid_rev(rev)?;
    Ok(git(path, &["rev-list", "--count", &format!("{rev}..HEAD")])?.trim().parse().unwrap_or(0))
}

/// Mueve la rama actual a `rev`. `soft`/`mixed` conservan los cambios (los commits quitados vuelven como
/// cambios sin commitear); `hard` los descarta junto con los cambios locales.
pub fn reset_to(path: &Path, rev: &str, mode: &str) -> Result<()> {
    valid_rev(rev)?;
    let flag = match mode {
        "soft" => "--soft",
        "mixed" => "--mixed",
        "hard" => "--hard",
        _ => bail!("modo de reset no válido"),
    };
    git(path, &["reset", flag, rev]).map(drop)
}

#[cfg(test)]
mod reset_tests {
    use super::*;

    #[test]
    fn reflog_rama_desde_commit_y_reset() {
        let d = std::env::temp_dir().join(format!("grove-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let sh = |a: &[&str]| {
            let mut v = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
            v.extend(a);
            assert!(git_cmd(&d, &v).status().unwrap().success(), "{a:?}");
        };
        sh(&["init", "-q", "-b", "main"]);
        for (f, m) in [("a", "uno"), ("b", "dos"), ("c", "tres")] {
            std::fs::write(d.join(f), format!("{f} {m} contenido distinto\n")).unwrap();
            sh(&["add", "."]);
            sh(&["commit", "-qm", m]);
        }
        let log = log_entries(&d, 0, 10).unwrap();
        let (tres, uno) = (log[0].id.clone(), log[2].id.clone());
        assert_eq!(commits_since(&d, &uno).unwrap(), 2);

        // rama desde un commit, sin cambiar y cambiando
        branch_at(&d, "vieja", &uno, false).unwrap();
        assert_eq!(current_branch(&d).unwrap(), "main");
        assert!(branch_at(&d, "vieja", &uno, false).is_err());
        assert!(branch_at(&d, "mal nombre", &uno, false).is_err());
        assert!(branch_at(&d, "x", "--oops", false).is_err());
        branch_at(&d, "otra", &uno, true).unwrap();
        assert_eq!(current_branch(&d).unwrap(), "otra");
        sh(&["switch", "-q", "main"]);

        // reset mixed: la rama retrocede y los cambios de los commits quitados quedan sin commitear
        reset_to(&d, &uno, "mixed").unwrap();
        assert_eq!(log_entries(&d, 0, 10).unwrap().len(), 1);
        assert_eq!(status_files(&d).unwrap().len(), 2);
        // el reflog permite volver: HEAD apuntó a `tres`
        let rl = reflog(&d, 20).unwrap();
        assert!(rl.iter().any(|e| e.id == tres), "el reflog conserva el commit anterior");
        assert!(rl[0].action.starts_with("reset"));
        // recuperar creando una rama en el commit perdido
        branch_at(&d, "recuperada", &tres, false).unwrap();
        assert_eq!(commits_since(&d, &uno).unwrap(), 0);
        // volver a `tres` desde el reflog y luego reset hard: quita los commits y deja el árbol limpio
        reset_to(&d, &tres, "hard").unwrap();
        assert_eq!(log_entries(&d, 0, 10).unwrap().len(), 3);
        std::fs::write(d.join("c"), "cambio local sin commitear\n").unwrap();
        reset_to(&d, &uno, "hard").unwrap();
        assert!(!d.join("b").exists());
        assert!(status_files(&d).unwrap().is_empty());
        assert!(reset_to(&d, &uno, "raro").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// Archivo distinto entre dos ramas, con líneas añadidas/borradas (0 si es binario).
pub struct CmpFile {
    pub path: String,
    pub kind: String,
    pub additions: u32,
    pub deletions: u32,
}

pub struct Comparison {
    /// Commits que `other` tiene y `base` no.
    pub ahead: u32,
    /// Commits que `base` tiene y `other` no.
    pub behind: u32,
    pub files: Vec<CmpFile>,
}

fn existing_branch(path: &Path, name: &str) -> Result<()> {
    if branch_list(path)?.iter().any(|b| b.name == name) {
        Ok(())
    } else {
        bail!("la rama `{name}` no existe")
    }
}

/// Qué cambia `other` respecto a `base` (diff desde su ancestro común, como un Pull Request) y cuántos
/// commits difieren en cada dirección.
pub fn compare(path: &Path, base: &str, other: &str) -> Result<Comparison> {
    existing_branch(path, base)?;
    existing_branch(path, other)?;
    let range = format!("{base}...{other}");
    let n = git(path, &["rev-list", "--left-right", "--count", &range])?;
    let mut c = n.split_whitespace().map(|x| x.parse().unwrap_or(0));
    let (behind, ahead) = (c.next().unwrap_or(0), c.next().unwrap_or(0));
    let status = git(path, &["-c", "core.quotepath=false", "diff", "--no-renames", "--name-status", "-z", "--end-of-options", &range])?;
    let stats = git(path, &["-c", "core.quotepath=false", "diff", "--no-renames", "--numstat", "-z", "--end-of-options", &range])?;
    let nums: std::collections::HashMap<&str, (u32, u32)> = stats
        .split('\0')
        .filter_map(|r| {
            let mut f = r.splitn(3, '\t');
            let (a, d, p) = (f.next()?, f.next()?, f.next()?);
            Some((p.trim_start_matches('\n'), (a.parse().unwrap_or(0), d.parse().unwrap_or(0))))
        })
        .collect();
    let mut files = Vec::new();
    let mut f = status.split('\0');
    while let (Some(st), Some(p)) = (f.next(), f.next()) {
        if st.is_empty() {
            continue;
        }
        let kind = match st {
            "A" => "Added",
            "D" => "Removed",
            "T" => "TypeChange",
            _ => "Modified",
        };
        let (additions, deletions) = nums.get(p).copied().unwrap_or((0, 0));
        files.push(CmpFile { path: p.into(), kind: kind.into(), additions, deletions });
    }
    Ok(Comparison { ahead, behind, files })
}

/// Diff de un archivo entre dos ramas (desde su ancestro común).
pub fn compare_diff(path: &Path, base: &str, other: &str, file: &str) -> Result<String> {
    existing_branch(path, base)?;
    existing_branch(path, other)?;
    let range = format!("{base}...{other}");
    let mut d = git(path, &["diff", "--no-color", "--no-ext-diff", "--end-of-options", &range, "--", file])?;
    if d.len() > MAX_DIFF {
        let mut n = MAX_DIFF;
        while !d.is_char_boundary(n) {
            n -= 1;
        }
        d.truncate(n);
        d.push_str("\n@@ diff recortado: archivo demasiado grande @@\n");
    }
    Ok(d)
}

#[cfg(test)]
mod compare_rebase_tests {
    use super::*;

    fn setup(tag: &str) -> (PathBuf, impl Fn(&[&str])) {
        let d = std::env::temp_dir().join(format!("grove-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let d2 = d.clone();
        let sh = move |a: &[&str]| {
            let mut v = vec!["-c", "user.name=t", "-c", "user.email=t@t"];
            v.extend(a);
            assert!(git_cmd(&d2, &v).status().unwrap().success(), "{a:?}");
        };
        (d, sh)
    }

    #[test]
    fn comparar_ramas() {
        let (d, sh) = setup("cmp");
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(d.join("a"), "1\n2\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "base"]);
        sh(&["switch", "-qc", "x"]);
        std::fs::write(d.join("a"), "1\n2\n3\n").unwrap();
        std::fs::write(d.join("n"), "nuevo\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "en x"]);
        sh(&["switch", "-q", "main"]);
        std::fs::write(d.join("m"), "solo main\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "en main"]);
        let c = compare(&d, "main", "x").unwrap();
        assert_eq!((c.ahead, c.behind), (1, 1));
        let mut f: Vec<_> = c.files.iter().map(|f| (f.path.as_str(), f.kind.as_str(), f.additions, f.deletions)).collect();
        f.sort();
        assert_eq!(f, [("a", "Modified", 1, 0), ("n", "Added", 1, 0)], "diff desde el ancestro: no incluye lo solo-main");
        assert!(compare_diff(&d, "main", "x", "a").unwrap().contains("+3"));
        assert!(compare(&d, "main", "--all").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rebase_con_conflicto_lados_y_continuar() {
        let (d, sh) = setup("rb");
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(d.join("a"), "base\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-qm", "base"]);
        sh(&["switch", "-qc", "x"]);
        std::fs::write(d.join("a"), "de x\n").unwrap();
        sh(&["commit", "-qam", "x1"]);
        sh(&["switch", "-q", "main"]);
        std::fs::write(d.join("a"), "de main\n").unwrap();
        sh(&["commit", "-qam", "m1"]);
        sh(&["switch", "-q", "x"]);
        // rebase de x sobre main: conflicto. «Mía» debe ser lo de x (mi commit), aunque git lo llame theirs.
        assert!(git(&d, &["rebase", "main"]).is_err());
        assert_eq!(sync_state(&d).unwrap().op, "rebase");
        let (mia, suya) = conflict_sides(&d, "a").unwrap();
        assert_eq!((mia.as_deref(), suya.as_deref()), (Some("de x\n"), Some("de main\n")));
        resolve_conflict(&d, "a", "ours").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "de x\n");
        assert!(merge_continue(&d).unwrap());
        assert!(!sync_state(&d).unwrap().merging);
        assert_eq!(log_entries(&d, 0, 10).unwrap().len(), 3, "base + m1 + x1 reaplicado encima");
        let _ = std::fs::remove_dir_all(&d);
    }
}
