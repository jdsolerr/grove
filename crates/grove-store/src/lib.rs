//! Persistencia local en SQLite (WAL). Solo metadatos de Grove; nunca secretos (esos van a `keyring`).
use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub struct Store(Connection);

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS repos (
               id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE,
               alias TEXT, last_opened INTEGER);
             CREATE TABLE IF NOT EXISTS groups (name TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        // Migración: la columna de grupo es posterior a la tabla original (falla si ya existe: es lo esperado).
        let _ = db.execute("ALTER TABLE repos ADD COLUMN grp TEXT", []);
        Ok(Self(db))
    }

    pub fn repos(&self) -> Result<Vec<String>> {
        let mut q = self.0.prepare("SELECT path FROM repos ORDER BY last_opened DESC, id")?;
        let rows = q.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// (ruta, alias, grupo) de cada repo; vacíos = sin definir.
    pub fn meta(&self) -> Result<Vec<(String, String, String)>> {
        let mut q = self.0.prepare("SELECT path, COALESCE(alias,''), COALESCE(grp,'') FROM repos")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_meta(&self, path: &str, alias: &str, group: &str) -> Result<()> {
        let n = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        self.0.execute("UPDATE repos SET alias = ?2, grp = ?3 WHERE path = ?1", rusqlite::params![path, n(alias), n(group)])?;
        Ok(())
    }

    /// Grupos existentes: los creados a mano (aunque estén vacíos) y los que ya usa algún repo.
    pub fn groups(&self) -> Result<Vec<String>> {
        let mut q = self.0.prepare(
            "SELECT name FROM groups UNION SELECT grp FROM repos WHERE grp IS NOT NULL AND grp <> '' ORDER BY 1 COLLATE NOCASE",
        )?;
        let rows = q.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn add_group(&self, name: &str) -> Result<()> {
        let name = name.trim();
        if !name.is_empty() {
            self.0.execute("INSERT OR IGNORE INTO groups (name) VALUES (?1)", [name])?;
        }
        Ok(())
    }

    /// Borra el grupo; sus repos quedan sin grupo (no se tocan en disco).
    pub fn delete_group(&self, name: &str) -> Result<()> {
        self.0.execute("DELETE FROM groups WHERE name = ?1", [name])?;
        self.0.execute("UPDATE repos SET grp = NULL WHERE grp = ?1", [name])?;
        Ok(())
    }

    /// Añade (o marca como recién abierto) un repo.
    pub fn add_repo(&self, path: &str) -> Result<()> {
        self.0.execute(
            "INSERT INTO repos (path, last_opened) VALUES (?1, strftime('%s','now'))
             ON CONFLICT(path) DO UPDATE SET last_opened = excluded.last_opened",
            [path],
        )?;
        Ok(())
    }

    /// Ajuste guardado; vacío si no existe.
    pub fn get(&self, key: &str) -> Result<String> {
        let mut q = self.0.prepare("SELECT value FROM settings WHERE key = ?1")?;
        Ok(q.query_row([key], |r| r.get(0)).unwrap_or_default())
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.0.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
        Ok(())
    }

    pub fn remove_repo(&self, path: &str) -> Result<()> {
        self.0.execute("DELETE FROM repos WHERE path = ?1", [path])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_list_remove() {
        let d = std::env::temp_dir().join(format!("grove-store-{}.db", std::process::id()));
        let s = Store::open(&d).unwrap();
        s.add_repo("/a").unwrap();
        s.add_repo("/b").unwrap();
        s.add_repo("/a").unwrap(); // duplicado: no inserta
        assert_eq!(s.repos().unwrap().len(), 2);
        s.set_meta("/a", " Mi app ", "Ascend").unwrap();
        assert!(s.meta().unwrap().contains(&("/a".into(), "Mi app".into(), "Ascend".into())));
        s.set_meta("/a", "", "").unwrap();
        assert!(s.meta().unwrap().contains(&("/a".into(), String::new(), String::new())));
        s.remove_repo("/a").unwrap();
        assert_eq!(s.repos().unwrap(), vec!["/b"]);
        let _ = std::fs::remove_file(d);
    }

    #[test]
    fn grupos_vacios_y_borrado() {
        let d = std::env::temp_dir().join(format!("grove-grp-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&d);
        let st = Store::open(&d).unwrap();
        st.add_group(" Ascend ").unwrap();
        st.add_repo("/r").unwrap();
        st.set_meta("/r", "", "Otro").unwrap();
        assert_eq!(st.groups().unwrap(), ["Ascend", "Otro"]);
        st.delete_group("Otro").unwrap();
        assert_eq!(st.groups().unwrap(), ["Ascend"]);
        assert_eq!(st.meta().unwrap()[0].2, "");
        let _ = std::fs::remove_file(&d);
    }

    #[test]
    fn ajustes() {
        let d = std::env::temp_dir().join(format!("grove-set-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&d);
        let st = Store::open(&d).unwrap();
        assert_eq!(st.get("ai_model").unwrap(), "");
        st.set("ai_model", "a").unwrap();
        st.set("ai_model", "b").unwrap();
        assert_eq!(st.get("ai_model").unwrap(), "b");
        let _ = std::fs::remove_file(&d);
    }
}
