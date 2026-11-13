//! Mensajes de commit con cualquier API compatible con OpenAI Sin proveedor configurado = función apagada.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub struct Config {
    /// Vacía = sin cabecera Authorization (Ollama local).
    pub key: String,
    pub base_url: String,
    pub model: String,
    /// Tope de caracteres de diff que se envían (cuida la cuota gratuita).
    pub max_chars: usize,
}

impl Config {
    /// Lo que se guardó en «Configuración de IA»; `None` si la IA está apagada (sin URL de proveedor).
    pub fn new(base_url: String, model: String) -> Option<Self> {
        (!base_url.trim().is_empty()).then(|| Self { key: load_key().unwrap_or_default(), base_url, model, max_chars: 12_000 })
    }
}

// La clave va al llavero del sistema (Llavero en macOS), nunca a SQLite ni a archivos.
fn entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("grove", "ai")?)
}

pub fn save_key(key: &str) -> Result<()> {
    if key.is_empty() { return delete_key(); }
    Ok(entry()?.set_password(key)?)
}

pub fn load_key() -> Option<String> {
    entry().ok()?.get_password().ok()
}

pub fn delete_key() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

const SYSTEM: &str = "Eres un asistente que redacta mensajes de commit en español. \
Recibes la lista de archivos cambiados (+líneas -líneas) y fragmentos del diff; puede venir recortado. \
Responde SOLO un JSON {\"summary\": string, \"body\": string}. \
summary: una línea de máximo 70 caracteres, en presente, que diga qué cambió y por qué importa, sin punto final ni prefijos tipo feat:. \
body: vacío si el título basta; si no, máximo 3 viñetas cortas que empiecen con \"- \" separadas por saltos de línea. \
No inventes lo que no se vea en el diff; si algo no se ve, usa un título general. No menciones el recorte.";

/// `https://` a cualquier host; `http://` solo local (Ollama): la clave y el diff no viajan en claro por la red.
pub fn check_url(url: &str) -> Result<()> {
    let u: ureq::http::Uri = url.parse().context("URL del proveedor no válida")?;
    let local = matches!(u.host(), Some("localhost" | "127.0.0.1" | "::1" | "[::1]"));
    match u.scheme_str() {
        Some("https") => Ok(()),
        Some("http") if local => Ok(()),
        Some("http") => bail!("http:// solo se permite con localhost; usa https://"),
        _ => bail!("la URL del proveedor debe empezar por https://"),
    }
}

pub fn commit_message(cfg: &Config, digest: &str) -> Result<(String, String)> {
    check_url(&cfg.base_url)?;
    let mut req = json!({
        "model": cfg.model,
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": digest}],
        "temperature": 0.2,
        "max_tokens": 700,
        "response_format": {"type": "json_object"},
    });
    // Los modelos de razonamiento de Groq gastan cuota pensando: lo mínimo.
    if cfg.model.contains("gpt-oss") {
        req["reasoning_effort"] = json!("low");
    }
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let mut r = ureq::post(&url);
    if !cfg.key.is_empty() {
        r = r.header("Authorization", &format!("Bearer {}", cfg.key));
    }
    let mut resp = r
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(90))) // proveedor colgado: error, no spinner eterno
        .build()
        .send_json(&req)
        .context("no se pudo contactar a la IA")?;
    let status = resp.status().as_u16();
    let v: Value = resp.body_mut().read_json().unwrap_or(Value::Null);
    if status >= 400 {
        let m = v["error"]["message"].as_str().unwrap_or("sin detalle");
        bail!("la IA respondió {status}: {m}{}", if status == 429 { " (cuota agotada, espera un momento)" } else { "" });
    }
    parse(v["choices"][0]["message"]["content"].as_str().unwrap_or(""))
}

fn parse(text: &str) -> Result<(String, String)> {
    let t = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        let s = v["summary"].as_str().unwrap_or("").trim().trim_end_matches('.').to_string();
        if !s.is_empty() {
            return Ok((s, v["body"].as_str().unwrap_or("").trim().to_string()));
        }
    }
    // Modelo que no respetó el JSON: primera línea = título, resto = descripción.
    let mut l = t.lines();
    let s = l.next().unwrap_or("").trim().trim_end_matches('.').to_string();
    if s.is_empty() {
        bail!("la IA no devolvió un mensaje");
    }
    Ok((s, l.collect::<Vec<_>>().join("\n").trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::{check_url, parse};

    #[test]
    fn urls_permitidas() {
        assert!(check_url("https://api.groq.com/openai/v1").is_ok());
        assert!(check_url("http://localhost:11434/v1").is_ok());
        assert!(check_url("http://127.0.0.1/v1").is_ok());
        assert!(check_url("http://[::1]:11434/v1").is_ok());
        assert!(check_url("http://192.168.1.5:11434/v1").is_err());
        assert!(check_url("http://localhost.evil.com/v1").is_err());
        assert!(check_url("http://localhost:80@evil.com/v1").is_err());
        assert!(check_url("ftp://x").is_err());
    }

    #[test]
    fn json_y_texto_plano() {
        assert_eq!(parse("{\"summary\":\"Agrega login.\",\"body\":\"- a\"}").unwrap(), ("Agrega login".into(), "- a".into()));
        assert_eq!(parse("```json\n{\"summary\":\"X\",\"body\":\"\"}\n```").unwrap().0, "X");
        assert_eq!(parse("Título\n\n- uno").unwrap(), ("Título".into(), "- uno".into()));
        assert!(parse("").is_err());
    }
}
