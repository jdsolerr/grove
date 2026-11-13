//! GitHub: login OAuth (device flow, sin secreto en el binario) y consulta de usuario.
//! El token vive en el llavero del sistema, nunca en disco plano.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const API: &str = "https://api.github.com";
const HOST: &str = "https://github.com";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u32,
    pub expires_in: u32,
}

#[derive(Debug, PartialEq)]
pub enum Poll {
    Pending,
    SlowDown,
    Done(String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct User {
    pub login: String,
    pub avatar_url: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into()
}

pub fn device_start(client_id: &str) -> Result<DeviceCode> {
    let r = agent()
        .post(format!("{HOST}/login/device/code"))
        .header("Accept", "application/json")
        .send_form([("client_id", client_id), ("scope", "repo read:user workflow")])
        .context("sin conexión con GitHub")?
        .body_mut()
        .read_json::<serde_json::Value>()?;
    if let Some(e) = r["error_description"].as_str().or(r["error"].as_str()) {
        bail!("{e}");
    }
    Ok(serde_json::from_value(r)?)
}

pub fn device_poll(client_id: &str, device_code: &str) -> Result<Poll> {
    let r = agent()
        .post(format!("{HOST}/login/oauth/access_token"))
        .header("Accept", "application/json")
        .send_form([
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])?
        .body_mut()
        .read_json::<serde_json::Value>()?;
    parse_poll(&r)
}

fn parse_poll(r: &serde_json::Value) -> Result<Poll> {
    if let Some(t) = r["access_token"].as_str() {
        return Ok(Poll::Done(t.into()));
    }
    match r["error"].as_str() {
        Some("authorization_pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown),
        Some("expired_token") => bail!("el código expiró, inicia sesión otra vez"),
        Some("access_denied") => bail!("autorización rechazada"),
        Some(e) => bail!("{e}"),
        None => bail!("respuesta inesperada de GitHub"),
    }
}

pub fn user(token: &str) -> Result<User> {
    let mut r = agent()
        .get(format!("{API}/user"))
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "grove")
        .call()?;
    if r.status() == 401 {
        bail!("token inválido o revocado");
    }
    Ok(r.body_mut().read_json()?)
}

pub enum Avatar {
    Url(String),
    /// El commit existe pero su autor no está vinculado a una cuenta de GitHub.
    NoAccount,
    /// GitHub no conoce ese sha (commit local sin publicar): probar con otro del mismo autor.
    Missing,
}

/// Foto del autor de un commit según GitHub (login real, sin enviar correos).
pub fn commit_avatar(token: &str, owner: &str, repo: &str, sha: &str) -> Result<Avatar> {
    if sha.is_empty() || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("sha no válido");
    }
    let mut r = agent()
        .get(format!("{API}/repos/{owner}/{repo}/commits/{sha}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "grove")
        .call()?;
    match r.status().as_u16() {
        200 => {}
        422 => return Ok(Avatar::Missing), // "No commit found for SHA": aún no está en GitHub
        404 | 403 => bail!("GitHub no deja leer {owner}/{repo} con esta sesión: tu cuenta no tiene acceso al repo, o su organización exige aprobar la OAuth App"),
        c => bail!("GitHub respondió {c} al pedir la foto del autor"),
    }
    let v: serde_json::Value = r.body_mut().read_json()?;
    Ok(v["author"]["avatar_url"].as_str().map_or(Avatar::NoAccount, |u| Avatar::Url(u.into())))
}

/// ¿Es privado el repo? Con token, el campo `private` de la API. Sin token, GitHub responde 404 a los
/// privados, y no se puede distinguir de "no existe": solo se afirma `Some(false)` si la API contesta 200.
pub fn repo_private(token: Option<&str>, owner: &str, repo: &str) -> Option<bool> {
    let mut req = agent().get(format!("{API}/repos/{owner}/{repo}")).header("User-Agent", "grove");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let mut r = req.call().ok()?;
    if r.status().as_u16() != 200 {
        return None;
    }
    let v: serde_json::Value = r.body_mut().read_json().ok()?;
    v["private"].as_bool()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RepoInfo {
    pub full_name: String,
    pub name: String,
    pub private: bool,
    pub clone_url: String,
}

/// Repos a los que tienes acceso (propios, colaborador y de organizaciones), recientes primero.
/// Limitación: tope de 300 repos (3 páginas); habría que filtrar en el servidor para cuentas con más.
pub fn list_repos(token: &str) -> Result<Vec<RepoInfo>> {
    let mut out = Vec::new();
    for page in 1..=3 {
        let mut r = agent()
            .get(format!("{API}/user/repos?per_page=100&sort=pushed&affiliation=owner,collaborator,organization_member&page={page}"))
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "grove")
            .call()?;
        if !r.status().is_success() {
            bail!("GitHub respondió {} al listar tus repositorios", r.status().as_u16());
        }
        let v: Vec<RepoInfo> = r.body_mut().read_json()?;
        let n = v.len();
        out.extend(v);
        if n < 100 {
            break;
        }
    }
    Ok(out)
}

// ---- Pull Requests ----

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Pr {
    pub number: u32,
    pub title: String,
    pub author: String,
    pub avatar_url: String,
    pub head: String,
    pub base: String,
    pub draft: bool,
    /// "open" | "closed" | "merged"
    pub state: String,
    pub updated_at: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrFile {
    pub path: String,
    /// "added" | "removed" | "modified" | "renamed"...
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    /// Hunks del diff; `None` en binarios o diffs enormes.
    pub patch: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrDetail {
    pub pr: Pr,
    pub body: String,
    /// `None` mientras GitHub aún lo calcula.
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
    pub commits: u32,
    pub head_sha: String,
    pub files: Vec<PrFile>,
}

/// Llamada JSON a la API. Los errores devuelven el `message` de GitHub (más el primer detalle), que es legible.
fn api(method: &str, token: &str, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
    let url = format!("{API}{path}");
    let a = agent();
    let auth = format!("Bearer {token}");
    macro_rules! h {
        ($r:expr) => {
            $r.header("Authorization", &auth).header("User-Agent", "grove").header("Accept", "application/vnd.github+json")
        };
    }
    let mut r = match (method, body) {
        ("GET", _) => h!(a.get(&url)).call(),
        ("POST", Some(b)) => h!(a.post(&url)).send_json(b),
        ("PUT", Some(b)) => h!(a.put(&url)).send_json(b),
        ("PATCH", Some(b)) => h!(a.patch(&url)).send_json(b),
        _ => bail!("llamada a la API no válida"),
    }
    .context("sin conexión con GitHub")?;
    let status = r.status();
    let v: serde_json::Value = r.body_mut().read_json().unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        let m = v["message"].as_str().unwrap_or("error de GitHub");
        let d = v["errors"][0]["message"].as_str().map(|d| format!(": {d}")).unwrap_or_default();
        bail!("GitHub ({}): {m}{d}", status.as_u16());
    }
    Ok(v)
}

fn parse_pr(v: &serde_json::Value) -> Pr {
    let s = |x: &serde_json::Value| x.as_str().unwrap_or_default().to_string();
    let merged = !v["merged_at"].is_null();
    Pr {
        number: v["number"].as_u64().unwrap_or(0) as u32,
        title: s(&v["title"]),
        author: s(&v["user"]["login"]),
        avatar_url: s(&v["user"]["avatar_url"]),
        head: s(&v["head"]["ref"]),
        base: s(&v["base"]["ref"]),
        draft: v["draft"].as_bool().unwrap_or(false),
        state: if merged { "merged".into() } else { s(&v["state"]) },
        updated_at: s(&v["updated_at"]),
        url: s(&v["html_url"]),
    }
}

/// `state`: "open" | "closed" | "all".
pub fn pr_list(token: &str, owner: &str, repo: &str, state: &str) -> Result<Vec<Pr>> {
    if !["open", "closed", "all"].contains(&state) {
        bail!("estado no válido");
    }
    let v = api("GET", token, &format!("/repos/{owner}/{repo}/pulls?state={state}&per_page=50&sort=updated&direction=desc"), None)?;
    Ok(v.as_array().map(|a| a.iter().map(parse_pr).collect()).unwrap_or_default())
}

pub fn pr_detail(token: &str, owner: &str, repo: &str, number: u32) -> Result<PrDetail> {
    let v = api("GET", token, &format!("/repos/{owner}/{repo}/pulls/{number}"), None)?;
    let f = api("GET", token, &format!("/repos/{owner}/{repo}/pulls/{number}/files?per_page=100"), None)?;
    let files = f
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| PrFile {
                    path: x["filename"].as_str().unwrap_or_default().into(),
                    status: x["status"].as_str().unwrap_or("modified").into(),
                    additions: x["additions"].as_u64().unwrap_or(0) as u32,
                    deletions: x["deletions"].as_u64().unwrap_or(0) as u32,
                    patch: x["patch"].as_str().map(Into::into),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(PrDetail {
        pr: parse_pr(&v),
        body: v["body"].as_str().unwrap_or_default().into(),
        mergeable: v["mergeable"].as_bool(),
        mergeable_state: v["mergeable_state"].as_str().unwrap_or("unknown").into(),
        commits: v["commits"].as_u64().unwrap_or(0) as u32,
        head_sha: v["head"]["sha"].as_str().unwrap_or_default().into(),
        files,
    })
}

pub struct NewPr<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub head: &'a str,
    pub base: &'a str,
    pub draft: bool,
}

pub fn pr_create(token: &str, owner: &str, repo: &str, n: &NewPr) -> Result<Pr> {
    let NewPr { title, body, head, base, draft } = *n;
    if title.trim().is_empty() {
        bail!("el título no puede estar vacío");
    }
    let v = api(
        "POST",
        token,
        &format!("/repos/{owner}/{repo}/pulls"),
        Some(serde_json::json!({ "title": title.trim(), "body": body, "head": head, "base": base, "draft": draft })),
    )?;
    Ok(parse_pr(&v))
}

/// `method`: "merge" | "squash" | "rebase".
pub fn pr_merge(token: &str, owner: &str, repo: &str, number: u32, method: &str) -> Result<()> {
    if !["merge", "squash", "rebase"].contains(&method) {
        bail!("método de fusión no válido");
    }
    api("PUT", token, &format!("/repos/{owner}/{repo}/pulls/{number}/merge"), Some(serde_json::json!({ "merge_method": method }))).map(drop)
}

pub fn pr_close(token: &str, owner: &str, repo: &str, number: u32) -> Result<()> {
    api("PATCH", token, &format!("/repos/{owner}/{repo}/pulls/{number}"), Some(serde_json::json!({ "state": "closed" }))).map(drop)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Check {
    pub name: String,
    /// "success" | "failure" | "pending" | "neutral"
    pub state: String,
    pub url: String,
}

fn check_state(status: &str, conclusion: Option<&str>) -> &'static str {
    if status != "completed" {
        return "pending";
    }
    match conclusion {
        Some("success") => "success",
        Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure") => "failure",
        _ => "neutral",
    }
}

/// Checks de CI de un commit (GitHub Actions y apps de checks).
pub fn pr_checks(token: &str, owner: &str, repo: &str, sha: &str) -> Result<Vec<Check>> {
    if !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("sha no válido");
    }
    let v = api("GET", token, &format!("/repos/{owner}/{repo}/commits/{sha}/check-runs?per_page=100"), None)?;
    Ok(v["check_runs"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Check {
                    name: c["name"].as_str().unwrap_or_default().into(),
                    state: check_state(c["status"].as_str().unwrap_or(""), c["conclusion"].as_str()).into(),
                    url: c["html_url"].as_str().unwrap_or_default().into(),
                })
                .collect()
        })
        .unwrap_or_default())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Comment {
    pub author: String,
    pub avatar_url: String,
    pub body: String,
    pub created_at: String,
}

pub fn pr_comments(token: &str, owner: &str, repo: &str, number: u32) -> Result<Vec<Comment>> {
    let v = api("GET", token, &format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100"), None)?;
    Ok(v.as_array().map(|a| a.iter().map(parse_comment).collect()).unwrap_or_default())
}

fn parse_comment(c: &serde_json::Value) -> Comment {
    let s = |x: &serde_json::Value| x.as_str().unwrap_or_default().to_string();
    Comment { author: s(&c["user"]["login"]), avatar_url: s(&c["user"]["avatar_url"]), body: s(&c["body"]), created_at: s(&c["created_at"]) }
}

pub fn pr_comment(token: &str, owner: &str, repo: &str, number: u32, body: &str) -> Result<Comment> {
    if body.trim().is_empty() {
        bail!("el comentario no puede estar vacío");
    }
    let v = api("POST", token, &format!("/repos/{owner}/{repo}/issues/{number}/comments"), Some(serde_json::json!({ "body": body })))?;
    Ok(parse_comment(&v))
}

/// Crea un repo en tu cuenta y devuelve (full_name, clone_url).
/// Limitación: solo cuenta personal, no organizaciones.
pub fn create_repo(token: &str, name: &str, description: &str, private: bool) -> Result<(String, String)> {
    let v = api("POST", token, "/user/repos", Some(serde_json::json!({ "name": name.trim(), "description": description, "private": private })))?;
    Ok((v["full_name"].as_str().unwrap_or_default().into(), v["clone_url"].as_str().unwrap_or_default().into()))
}

/// Rama por defecto del repo (base habitual de un PR).
pub fn default_branch(token: &str, owner: &str, repo: &str) -> Result<String> {
    let v = api("GET", token, &format!("/repos/{owner}/{repo}"), None)?;
    Ok(v["default_branch"].as_str().unwrap_or("main").into())
}

fn entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("grove", "github.com")?)
}

pub fn save_token(token: &str) -> Result<()> {
    Ok(entry()?.set_password(token)?)
}

pub fn load_token() -> Option<String> {
    entry().ok()?.get_password().ok()
}

pub fn logout() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pr_parsing() {
        let v = json!({"number":7,"title":"t","user":{"login":"u","avatar_url":"a"},"head":{"ref":"h"},"base":{"ref":"main"},
            "draft":true,"state":"closed","merged_at":"2026-01-01T00:00:00Z","updated_at":"x","html_url":"https://github.com/o/r/pull/7"});
        let p = parse_pr(&v);
        assert_eq!((p.number, p.author.as_str(), p.head.as_str(), p.state.as_str(), p.draft), (7, "u", "h", "merged", true));
        assert!(pr_list("t", "o", "r", "x; y").is_err());
        assert!(pr_merge("t", "o", "r", 1, "yolo").is_err());
        assert!(pr_create("t", "o", "r", &NewPr { title: "  ", body: "", head: "h", base: "b", draft: false }).is_err());
    }

    #[test]
    fn checks_and_comments() {
        assert_eq!(check_state("in_progress", None), "pending");
        assert_eq!(check_state("completed", Some("success")), "success");
        assert_eq!(check_state("completed", Some("timed_out")), "failure");
        assert_eq!(check_state("completed", Some("skipped")), "neutral");
        assert!(pr_checks("t", "o", "r", "x; rm").is_err());
        assert!(pr_comment("t", "o", "r", 1, " ").is_err());
        let c = parse_comment(&json!({"user":{"login":"u","avatar_url":"a"},"body":"hola","created_at":"d"}));
        assert_eq!((c.author.as_str(), c.body.as_str()), ("u", "hola"));
    }

    #[test]
    fn poll_states() {
        assert_eq!(parse_poll(&json!({"error":"authorization_pending"})).unwrap(), Poll::Pending);
        assert_eq!(parse_poll(&json!({"error":"slow_down"})).unwrap(), Poll::SlowDown);
        assert_eq!(parse_poll(&json!({"access_token":"t"})).unwrap(), Poll::Done("t".into()));
        assert!(parse_poll(&json!({"error":"expired_token"})).is_err());
        assert!(parse_poll(&json!({"error":"access_denied"})).is_err());
    }
}
