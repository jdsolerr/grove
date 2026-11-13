// Fija GROVE_GITHUB_CLIENT_ID al compilar (lo lee `option_env!` en lib.rs). Orden: variable de
// entorno ya definida (CI) y, si no, la línea del `.env` de la raíz del repo.
// Solo se lee esa clave: el resto del `.env` (p. ej. claves de IA) nunca debe quedar dentro del binario.
const KEY: &str = "GROVE_GITHUB_CLIENT_ID";

fn main() {
    let env_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    println!("cargo:rerun-if-changed={}", env_file.display());
    println!("cargo:rerun-if-env-changed={KEY}");

    if std::env::var_os(KEY).is_none() {
        // Parser mínimo: no admite `export` ni comentarios en la misma línea.
        let value = std::fs::read_to_string(&env_file).ok().and_then(|s| {
            s.lines()
                .filter_map(|l| l.trim().strip_prefix(KEY)?.trim_start().strip_prefix('='))
                .map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
                .find(|v| !v.is_empty())
        });
        if let Some(v) = value {
            println!("cargo:rustc-env={KEY}={v}");
        }
    }

    tauri_build::build()
}
