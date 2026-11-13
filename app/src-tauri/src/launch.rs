//! Abrir un repo en el editor, la terminal o el explorador del sistema.
use std::path::Path;
use std::process::Command;

#[derive(serde::Deserialize, specta::Type, Clone, Copy)]
pub enum Target {
    Editor,
    Terminal,
    Folder,
}

pub fn open(target: Target, path: &Path) -> Result<(), String> {
    // La ruta viene de la UI: solo repos reales.
    grove_git::current_branch(path).map_err(|e| e.to_string())?;
    match target {
        Target::Folder => folder(path),
        Target::Terminal => terminal(path),
        Target::Editor => editor(path),
    }
}

fn spawn(mut c: Command) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // sin consola intermedia
    }
    c.spawn().map(drop).map_err(|e| e.to_string())
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| ["", ".exe", ".cmd", ".bat"].iter().any(|e| d.join(format!("{name}{e}")).is_file()))
    })
}

fn folder(path: &Path) -> Result<(), String> {
    let (prog, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("open", &[])
    } else if cfg!(windows) {
        ("explorer", &[])
    } else {
        ("xdg-open", &[])
    };
    let mut c = Command::new(prog);
    c.args(args).arg(path);
    spawn(c)
}

fn terminal(path: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.args(["-a", "Terminal"]).arg(path);
        return spawn(c);
    }
    if cfg!(windows) {
        let c = if on_path("wt") {
            let mut c = Command::new("wt");
            c.arg("-d").arg(path);
            c
        } else {
            let mut c = Command::new("cmd");
            c.args(["/C", "start", "cmd.exe"]).current_dir(path);
            c
        };
        return spawn(c);
    }
    for t in ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"] {
        if on_path(t) {
            let mut c = Command::new(t);
            c.current_dir(path);
            return spawn(c);
        }
    }
    Err("No se encontró ninguna terminal instalada".into())
}

fn editor(path: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        // Desde Finder el PATH es mínimo (no hay `code`): se usa el nombre de la app.
        for app in ["Visual Studio Code", "Cursor", "Zed", "Sublime Text", "VSCodium"] {
            if Path::new(&format!("/Applications/{app}.app")).exists() {
                let mut c = Command::new("open");
                c.args(["-a", app]).arg(path);
                return spawn(c);
            }
        }
    } else {
        for e in ["code", "cursor", "zed", "subl", "codium"] {
            if on_path(e) {
                // En Windows son `.cmd`: pasan por cmd.
                let mut c = if cfg!(windows) {
                    let mut c = Command::new("cmd");
                    c.args(["/C", e]);
                    c
                } else {
                    Command::new(e)
                };
                c.arg(path);
                return spawn(c);
            }
        }
    }
    Err("No se encontró un editor (VS Code, Cursor, Zed o Sublime Text)".into())
}

#[derive(serde::Deserialize, specta::Type, Clone, Copy)]
pub enum FileAction {
    Reveal,
    Editor,
    Default,
}

/// Acciones sobre un archivo del repo (menú contextual de Cambios).
pub fn file_action(action: FileAction, repo: &Path, file: &str) -> Result<(), String> {
    grove_git::current_branch(repo).map_err(|e| e.to_string())?;
    // La ruta viene de la UI: relativa y sin `..`.
    if !Path::new(file).components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err("ruta de archivo no válida".into());
    }
    let full = repo.join(file);
    match action {
        FileAction::Editor => editor(&full),
        FileAction::Reveal => {
            let mut c;
            if cfg!(target_os = "macos") {
                c = Command::new("open");
                c.arg("-R").arg(&full);
            } else if cfg!(windows) {
                c = Command::new("explorer");
                c.arg(format!("/select,{}", full.display()));
            } else {
                c = Command::new("xdg-open");
                c.arg(full.parent().unwrap_or(repo));
            }
            spawn(c)
        }
        FileAction::Default => {
            let mut c;
            if cfg!(target_os = "macos") {
                c = Command::new("open");
                c.arg(&full);
            } else if cfg!(windows) {
                c = Command::new("cmd");
                c.args(["/C", "start", ""]).arg(&full);
            } else {
                c = Command::new("xdg-open");
                c.arg(&full);
            }
            spawn(c)
        }
    }
}
