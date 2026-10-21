//! 修复「每次重启固定错乱同一批图标」——清掉损坏的系统布局缓存
//!
//! Win10 1809+ shell 把桌面图标位置存在
//! `HKCU\Software\Microsoft\Windows\Shell\Bags\1\Desktop\IconLayouts`（REG_BINARY）。
//! 这个二进制值内部状态一旦被弄脏，Windows 会按错误方式还原并写回，
//! 表现得像系统固有 bug：手动拖动、工具恢复都无效。
//!
//! 做法：先备份成文件，再删值，重启后系统会重建一份干净的。

use crate::sys::*;
use crate::store;
use anyhow::{Context, Result};
use std::path::PathBuf;

const BAG_KEY: &str = r"Software\Microsoft\Windows\Shell\Bags\1\Desktop";
const VALUE: &str = "IconLayouts";

pub struct RepairInfo {
    pub backup: PathBuf,
    pub bytes: usize,
}

pub struct Key(isize);

impl Key {
    fn open(sam: u32) -> Result<Self> {
        let mut h: isize = 0;
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, to_wide(BAG_KEY).as_ptr(), 0, sam, &mut h) };
        if rc != ERROR_SUCCESS {
            anyhow::bail!("打开注册表失败（错误 {}）", rc);
        }
        Ok(Key(h))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

/// 读取 IconLayouts 原始字节；不存在则返回 None
pub fn read_icon_layouts() -> Result<Option<Vec<u8>>> {
    let key = match Key::open(KEY_READ) {
        Ok(k) => k,
        Err(_) => return Ok(None),
    };

    let name = to_wide(VALUE);
    let mut ty: u32 = 0;
    let mut cb: u32 = 0;
    let rc = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            std::ptr::null_mut(),
            &mut cb,
        )
    };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if rc != ERROR_SUCCESS {
        anyhow::bail!("读取 IconLayouts 大小失败（错误 {}）", rc);
    }

    let mut buf = vec![0u8; cb.max(1) as usize];
    let mut cb2 = buf.len() as u32;
    let rc = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut cb2,
        )
    };
    if rc != ERROR_SUCCESS {
        anyhow::bail!("读取 IconLayouts 失败（错误 {}）", rc);
    }
    buf.truncate(cb2 as usize);
    Ok(Some(buf))
}

pub fn backup_path() -> PathBuf {
    store::root_dir().join("IconLayouts.bak")
}

/// 备份 + 删除。返回 None 表示本来就没有这个值（已经是干净状态）
pub fn repair() -> Result<Option<RepairInfo>> {
    let Some(blob) = read_icon_layouts()? else {
        return Ok(None);
    };

    let backup = backup_path();
    let _ = std::fs::create_dir_all(store::root_dir());
    std::fs::write(&backup, &blob).with_context(|| format!("备份失败: {}", backup.display()))?;

    let key = Key::open(KEY_SET_VALUE).context("打开注册表写入权限失败")?;
    let rc = unsafe { RegDeleteValueW(key.0, to_wide(VALUE).as_ptr()) };
    drop(key);
    if rc != ERROR_SUCCESS {
        anyhow::bail!("删除 IconLayouts 失败（错误 {}），可能是权限问题", rc);
    }

    Ok(Some(RepairInfo {
        backup,
        bytes: blob.len(),
    }))
}

/// 仅做诊断：缓存是否存在、多大
pub fn describe_cache() -> String {
    match read_icon_layouts() {
        Ok(Some(b)) => format!("{} 字节", b.len()),
        Ok(None) => "不存在（干净）".to_string(),
        Err(e) => format!("读取失败: {}", e),
    }
}
