use crate::types::{now, Entry, EntryKind, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::HashMap, fs, path::PathBuf};
use tauri::Manager;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS entries (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  preview TEXT NOT NULL,
  favorite INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  source TEXT NOT NULL,
  image BLOB,
  thumb BLOB,
  image_width INTEGER NOT NULL DEFAULT 0,
  image_height INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_entries_created ON entries(created_at);
CREATE TABLE IF NOT EXISTS settings (
  k INTEGER PRIMARY KEY CHECK (k = 1),
  value TEXT NOT NULL
);
";

type R<T> = Result<T, String>;
fn E<T>(r: rusqlite::Result<T>) -> R<T> {
    r.map_err(|e| e.to_string())
}

pub fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.dat").exists() {
                return dir.join("data");
            }
        }
    }
    app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn db_path(app: &tauri::AppHandle) -> PathBuf {
    data_dir(app).join("leonpaste.db")
}

fn open(app: &tauri::AppHandle) -> R<Connection> {
    fs::create_dir_all(data_dir(app)).map_err(|e| e.to_string())?;
    let c = E(Connection::open(db_path(app)))?;
    E(c.execute_batch(SCHEMA))?;
    Ok(c)
}

fn kind_str(k: &EntryKind) -> &'static str {
    match k {
        EntryKind::Text => "text",
        EntryKind::Link => "link",
        EntryKind::Code => "code",
        EntryKind::Color => "color",
        EntryKind::Path => "path",
        EntryKind::Image => "image",
        EntryKind::Files => "files",
    }
}
fn kind_parse(s: &str) -> EntryKind {
    match s {
        "link" => EntryKind::Link,
        "code" => EntryKind::Code,
        "color" => EntryKind::Color,
        "path" => EntryKind::Path,
        "image" => EntryKind::Image,
        "files" => EntryKind::Files,
        _ => EntryKind::Text,
    }
}

const SELECT_COLS: &str = "SELECT id, kind, text, preview, favorite, created_at, source, \
     (image IS NOT NULL) AS has_image, image_width, image_height FROM entries";

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    let has_img: i64 = row.get(7)?;
    Ok(Entry {
        id: row.get(0)?,
        kind: kind_parse(&row.get::<_, String>(1)?),
        text: row.get(2)?,
        preview: row.get(3)?,
        favorite: row.get::<_, i64>(4)? != 0,
        created_at: row.get::<_, i64>(5)? as u64,
        source: row.get(6)?,
        image: if has_img != 0 { Some("db".into()) } else { None },
        thumb: None,
        image_width: row.get::<_, i64>(8)? as u32,
        image_height: row.get::<_, i64>(9)? as u32,
    })
}

/// 内存缓存只放元数据；图片 BLOB 按需从数据库读取。
pub fn load_entries(app: &tauri::AppHandle) -> R<Vec<Entry>> {
    let c = open(app)?;
    let mut st = E(c.prepare(&format!("{} ORDER BY created_at", SELECT_COLS)))?;
    let rows = E(st.query_map([], row_to_entry))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(E(r)?);
    }
    Ok(out)
}

pub fn load_settings(app: &tauri::AppHandle) -> R<Settings> {
    let c = open(app)?;
    let v: Option<String> = E(
        c.query_row("SELECT value FROM settings WHERE k = 1", [], |r| r.get(0))
            .optional(),
    )?;
    Ok(v.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

pub fn save_settings(app: &tauri::AppHandle, s: &Settings) -> R<()> {
    let v = serde_json::to_string(s).map_err(|e| e.to_string())?;
    let c = open(app)?;
    E(c.execute(
        "INSERT INTO settings(k, value) VALUES(1, ?1) \
         ON CONFLICT(k) DO UPDATE SET value = excluded.value",
        params![v],
    ))?;
    Ok(())
}

/// 单条插入（含图片 BLOB），不再全量重写整个历史。
pub fn insert(
    app: &tauri::AppHandle,
    e: &Entry,
    image: Option<&[u8]>,
    thumb: Option<&[u8]>,
) -> R<()> {
    let c = open(app)?;
    E(c.execute(
        "INSERT OR REPLACE INTO entries \
         (id, kind, text, preview, favorite, created_at, source, image, thumb, image_width, image_height) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            e.id,
            kind_str(&e.kind),
            e.text,
            e.preview,
            e.favorite as i64,
            e.created_at as i64,
            e.source,
            image,
            thumb,
            e.image_width as i64,
            e.image_height as i64
        ],
    ))?;
    Ok(())
}

pub fn set_favorite(app: &tauri::AppHandle, id: &str, value: bool) -> R<()> {
    let c = open(app)?;
    E(c.execute(
        "UPDATE entries SET favorite = ?1 WHERE id = ?2",
        params![value as i64, id],
    ))?;
    Ok(())
}

pub fn delete(app: &tauri::AppHandle, id: &str) -> R<()> {
    let c = open(app)?;
    E(c.execute("DELETE FROM entries WHERE id = ?1", params![id]))?;
    Ok(())
}

pub fn clear_non_favorites(app: &tauri::AppHandle) -> R<()> {
    let c = open(app)?;
    E(c.execute("DELETE FROM entries WHERE favorite = 0", []))?;
    Ok(())
}

/// 过期 + 超限清理：图片 BLOB 与条目在同一行，删行即删图，不留孤儿文件。
pub fn prune(app: &tauri::AppHandle, s: &Settings) -> R<Vec<String>> {
    let mut c = open(app)?;
    let mut ids: Vec<String> = Vec::new();
    if s.retention_days > 0 {
        let cutoff = now().saturating_sub(s.retention_days as u64 * 86_400) as i64;
        let mut st = E(c.prepare(
            "SELECT id FROM entries WHERE favorite = 0 AND created_at < ?1",
        ))?;
        let rows = E(st.query_map(params![cutoff], |r| r.get::<_, String>(0)))?;
        for r in rows {
            ids.push(E(r)?);
        }
    }
    let total: i64 = E(c.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)))?;
    if total > s.max_items as i64 {
        let need = (total - s.max_items as i64) as usize;
        let mut st = E(c.prepare(
            "SELECT id FROM entries WHERE favorite = 0 ORDER BY created_at ASC LIMIT ?1",
        ))?;
        let rows = E(st.query_map(params![need as i64], |r| r.get::<_, String>(0)))?;
        for r in rows {
            let id = E(r)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if !ids.is_empty() {
        let tx = E(c.transaction())?;
        for id in &ids {
            E(tx.execute("DELETE FROM entries WHERE id = ?1", params![id]))?;
        }
        E(tx.commit())?;
    }
    Ok(ids)
}

pub fn load_image(app: &tauri::AppHandle, id: &str) -> R<Vec<u8>> {
    let c = open(app)?;
    E(c.query_row(
        "SELECT image FROM entries WHERE id = ?1",
        params![id],
        |r| r.get::<_, Vec<u8>>(0),
    ))
}

pub fn load_thumbs(app: &tauri::AppHandle, ids: &[String]) -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    if let Ok(c) = open(app) {
        if let Ok(mut st) = c.prepare("SELECT thumb FROM entries WHERE id = ?1") {
            for id in ids {
                if let Ok(b) = st.query_row(params![id.as_str()], |r| r.get::<_, Vec<u8>>(0)) {
                    out.insert(id.clone(), b);
                }
            }
        }
    }
    out
}

/// 首次启动把旧 history.json / settings.json / images/ 一次性导入 SQLite，
/// 成功后删除旧的 JSON 与图片目录（包括长期堆积的孤儿文件）。
fn migrate_legacy(app: &tauri::AppHandle, c: &mut Connection) -> R<usize> {
    let dir = data_dir(app);
    let hist = dir.join("history.json");
    let setp = dir.join("settings.json");
    let images = dir.join("images");
    if !hist.exists() && !setp.exists() {
        return Ok(0);
    }

    let cfg: Settings = fs::read_to_string(&setp)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let v = serde_json::to_string(&cfg).map_err(|e| e.to_string())?;
    E(c.execute(
        "INSERT OR REPLACE INTO settings(k, value) VALUES(1, ?1)",
        params![v],
    ))?;

    let old: Vec<Entry> = fs::read_to_string(&hist)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let n = old.len();
    let tx = E(c.transaction())?;
    for e in &old {
        let (img, th): (Option<Vec<u8>>, Option<Vec<u8>>) = if let Some(name) = &e.image {
            let stem = name.trim_end_matches(".dib");
            (
                fs::read(images.join(format!("{}.dib", stem))).ok(),
                fs::read(images.join(format!("{}.bmp", stem))).ok(),
            )
        } else {
            (None, None)
        };
        E(tx.execute(
            "INSERT OR IGNORE INTO entries \
             (id, kind, text, preview, favorite, created_at, source, image, thumb, image_width, image_height) \
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                e.id,
                kind_str(&e.kind),
                e.text,
                e.preview,
                e.favorite as i64,
                e.created_at as i64,
                e.source,
                img,
                th,
                e.image_width as i64,
                e.image_height as i64
            ],
        ))?;
    }
    E(tx.commit())?;

    // DB 已成为唯一数据源，旧的 JSON 与散落图片（含孤儿）全部回收
    let _ = fs::remove_dir_all(&images);
    let _ = fs::remove_file(&hist);
    let _ = fs::remove_file(&setp);
    let _ = fs::remove_file(dir.join("history.json.tmp"));
    let _ = fs::remove_file(dir.join("settings.json.tmp"));
    Ok(n)
}

pub fn load(app: &tauri::AppHandle) -> (Vec<Entry>, Settings) {
    let _ = fs::create_dir_all(data_dir(app));
    let mut c = match open(app) {
        Ok(c) => c,
        Err(e) => {
            crate::log_line(app, &format!("db-open-failed: {}", e));
            return (Vec::new(), Settings::default());
        }
    };
    let count: i64 = c
        .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))
        .unwrap_or(0);
    if count == 0 {
        match migrate_legacy(app, &mut c) {
            Ok(n) if n > 0 => {
                crate::log_line(app, &format!("db-migrated {} entries from legacy json", n))
            }
            Ok(_) => crate::log_line(app, "db-created (no legacy data)"),
            Err(e) => crate::log_line(app, &format!("db-migration-failed: {}", e)),
        }
    }
    let entries = load_entries(app).unwrap_or_default();
    let mut settings = load_settings(app).unwrap_or_default();
    if settings.hotkey == "Ctrl+Shift+V" {
        settings.hotkey = "Alt+Shift+V".into();
    }
    (entries, settings)
}
