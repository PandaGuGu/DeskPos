//! 布局文件存储：`%APPDATA%\DesktopIconPos\layouts\*.json`

use crate::model::{Layout, LayoutEntry};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// 布局目录（不存在则创建）
pub fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let path = base.join("DesktopIconPos").join("layouts");
    let _ = fs::create_dir_all(&path);
    path
}

/// 数据目录（布局目录的上一级）
pub fn root_dir() -> PathBuf {
    dir().parent().map(|p| p.to_path_buf()).unwrap_or_else(dir)
}

/// 文件名安全化：Windows 不允许的字符换成下划线
pub fn safe_name(name: &str) -> String {
    const BAD: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let cleaned: String = name
        .chars()
        .map(|c| if BAD.contains(&c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().to_string();
    if trimmed.is_empty() {
        "layout".to_string()
    } else {
        trimmed
    }
}

pub fn path_of(name: &str) -> PathBuf {
    dir().join(format!("{}.json", safe_name(name)))
}

pub fn exists(name: &str) -> bool {
    path_of(name).is_file()
}

pub fn save(layout: &Layout) -> Result<PathBuf> {
    let path = path_of(&layout.name);
    let text = serde_json::to_string_pretty(layout).context("序列化布局失败")?;
    fs::write(&path, text).with_context(|| format!("写入失败: {}", path.display()))?;
    Ok(path)
}

pub fn load(name: &str) -> Result<Layout> {
    let path = path_of(name);
    if !path.is_file() {
        anyhow::bail!("布局不存在: {}", name);
    }
    let text = fs::read_to_string(&path).with_context(|| format!("读取失败: {}", path.display()))?;
    let layout: Layout =
        serde_json::from_str(&text).with_context(|| format!("布局文件格式不对: {}", path.display()))?;
    Ok(layout)
}

pub fn load_file(path: &std::path::Path) -> Result<Layout> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

/// 列出所有布局，按创建时间倒序
pub fn list() -> Vec<LayoutEntry> {
    let d = dir();
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&d) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_json = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if !is_json {
            continue;
        }
        let Ok(data) = load_file(&path) else {
            continue; // 跳过损坏的文件
        };
        let file_name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let stamp = if data.created.is_empty() {
            mtime_text(&path)
        } else {
            data.created.clone()
        };
        out.push(LayoutEntry {
            file_name,
            path,
            stamp,
            data,
        });
    }
    out.sort_by(|a, b| b.stamp.cmp(&a.stamp));
    out
}

fn mtime_text(path: &std::path::Path) -> String {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d %H:%M:%S").to_string()
        })
        .unwrap_or_else(|_| "?".to_string())
}

/// 最近一次保存的布局名
pub fn latest() -> Option<String> {
    list().into_iter().next().map(|e| e.data.name)
}

pub fn delete(name: &str) -> Result<()> {
    let path = path_of(name);
    if !path.is_file() {
        anyhow::bail!("布局不存在: {}", name);
    }
    fs::remove_file(&path).with_context(|| format!("删除失败: {}", path.display()))?;
    Ok(())
}

pub fn delete_entry(entry: &LayoutEntry) -> Result<()> {
    if entry.path.is_file() {
        fs::remove_file(&entry.path)
            .with_context(|| format!("删除失败: {}", entry.path.display()))?;
    }
    Ok(())
}
