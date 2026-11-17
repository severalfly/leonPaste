use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Text,
    Link,
    Code,
    Color,
    Path,
    Image,
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub kind: EntryKind,
    pub text: String,
    pub preview: String,
    pub favorite: bool,
    pub created_at: u64,
    pub source: String,
    /// 原图 DIB 文件名（data/images/<id>.dib），用于写回剪贴板
    pub image: Option<String>,
    /// 缩略图 data URL，仅在 list_history 返回时填充，不持久化
    pub thumb: Option<String>,
    pub image_width: u32,
    pub image_height: u32,
}

impl Entry {
    pub fn blank() -> Self {
        Self {
            id: String::new(),
            kind: EntryKind::Text,
            text: String::new(),
            preview: String::new(),
            favorite: false,
            created_at: 0,
            source: String::new(),
            image: None,
            thumb: None,
            image_width: 0,
            image_height: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub max_items: usize,
    pub retention_days: u32,
    pub hotkey: String,
    pub auto_paste: bool,
    pub autostart: bool,
    pub theme: String,
    pub ignore_regex: String,
    pub paused: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_items: 500,
            retention_days: 30,
            hotkey: "Ctrl+Shift+V".into(),
            auto_paste: true,
            autostart: false,
            theme: "dark".into(),
            ignore_regex: String::new(),
            paused: false,
        }
    }
}

pub fn classify(text: &str) -> EntryKind {
    let t = text.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        return EntryKind::Link;
    }
    if t.starts_with('#')
        && (t.len() == 4 || t.len() == 7 || t.len() == 9)
        && t[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        return EntryKind::Color;
    }
    if (t.starts_with("C://") || t.starts_with('/') || t.starts_with("\\\\"))
        && !t.contains('\n')
    {
        return EntryKind::Path;
    }
    if t.contains('\n')
        && (t.contains("fn ") || t.contains("function ") || t.contains("=> ")
            || t.contains("SELECT ") || t.contains("import ") || t.contains("class "))
    {
        return EntryKind::Code;
    }
    EntryKind::Text
}

pub fn preview(text: &str) -> String {
    text.replace(['\r', '\n', '\t'], " ")
        .trim()
        .chars()
        .take(200)
        .collect()
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
