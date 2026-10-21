//! GUI 用的素材：真实壁纸 + 系统真实图标（HICON → RGBA）
//!
//! 图标查找顺序与 C# 版 Preview.cs 一致：
//!   1) 系统虚拟项（此电脑 / 回收站 / 网络 / 控制面板 …）走 shell 命名空间路径
//!   2) 桌面目录里同名文件 / 文件夹
//!   3) 「短名 → 完整路径」索引（快捷方式真实文件名带 .lnk、隐藏扩展名）

use crate::sys::*;
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

pub struct IconProvider {
    bare: Option<HashMap<String, PathBuf>>,
    cache: HashMap<String, Option<Rgba>>,
}

impl Default for IconProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl IconProvider {
    pub fn new() -> Self {
        IconProvider {
            bare: None,
            cache: HashMap::new(),
        }
    }

    /// 桌面内容变了就调一下，重新扫描文件名
    pub fn invalidate(&mut self) {
        self.bare = None;
        self.cache.clear();
    }

    pub fn get(&mut self, name: &str) -> Option<Rgba> {
        if name.is_empty() {
            return None;
        }
        if let Some(hit) = self.cache.get(name) {
            return hit.clone();
        }

        let mut img = None;

        // 1. 系统虚拟项
        if let Some(shell_path) = virtual_shell_path(name) {
            img = load_shell(shell_path);
        }

        // 2. 桌面上的实体文件 / 文件夹
        if img.is_none() {
            for dir in desktop_dirs() {
                let p = dir.join(name);
                if p.exists() {
                    img = load_path(&p);
                    break;
                }
            }
        }

        // 3. 短名匹配（快捷方式、隐藏扩展名）
        if img.is_none() {
            if let Some(p) = self.bare_index().get(&name.to_lowercase()).cloned() {
                img = load_path(&p);
            }
        }

        self.cache.insert(name.to_string(), img.clone());
        img
    }

    fn bare_index(&mut self) -> &HashMap<String, PathBuf> {
        if self.bare.is_none() {
            let mut map = HashMap::new();
            for dir in desktop_dirs() {
                let Ok(rd) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for e in rd.flatten() {
                    let path = e.path();
                    if let Some(stem) = path.file_stem() {
                        map.entry(stem.to_string_lossy().to_lowercase())
                            .or_insert_with(|| path.clone());
                    }
                    if let Some(fname) = path.file_name() {
                        map.entry(fname.to_string_lossy().to_lowercase())
                            .or_insert_with(|| path.clone());
                    }
                }
            }
            self.bare = Some(map);
        }
        self.bare.as_ref().unwrap()
    }
}

pub fn desktop_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("USERPROFILE") {
        v.push(Path::new(&p).join("Desktop"));
    }
    if let Some(p) = std::env::var_os("PUBLIC") {
        v.push(Path::new(&p).join("Desktop"));
    }
    v.retain(|p| p.is_dir());
    v
}

/// 桌面上的系统虚拟项（shell 命名空间路径）
fn virtual_shell_path(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    Some(match n.as_str() {
        "此电脑" | "this pc" | "计算机" => "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
        "回收站" | "recycle bin" => "::{645FF040-5081-101B-9F08-00AA002F954E}",
        "网络" | "network" => "::{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}",
        "控制面板" | "control panel" => "::{5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0}",
        "用户文件夹" | "administrator" | "administrator 的文件夹" => {
            "::{59031A47-3F72-44A7-89C5-5595FE6B30EE}"
        }
        _ => return None,
    })
}

fn load_path(path: &Path) -> Option<Rgba> {
    let wide = to_wide(&path.to_string_lossy());
    unsafe {
        let mut info = SHFILEINFO::default();
        let ok = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFO>() as u32,
            SHGFI_ICON | SHGFI_SYSICONINDEX | SHGFI_LARGEICON,
        );
        if ok == 0 {
            return None;
        }
        finish(&info)
    }
}

fn load_shell(shell_path: &str) -> Option<Rgba> {
    unsafe {
        let wide = to_wide(shell_path);
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        let mut attrs: u32 = 0;
        if SHParseDisplayName(wide.as_ptr(), std::ptr::null(), &mut pidl, 0, &mut attrs) != S_OK
            || pidl.is_null()
        {
            return None;
        }

        let mut info = SHFILEINFO::default();
        let ok = SHGetFileInfoW(
            pidl as *const u16,
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFO>() as u32,
            SHGFI_ICON | SHGFI_SYSICONINDEX | SHGFI_LARGEICON | SHGFI_PIDL,
        );
        ILFree(pidl);
        if ok == 0 {
            return None;
        }
        finish(&info)
    }
}

/// 优先从系统「超大图标」列表取 48px（缩略图里更清楚），失败退回 32px
unsafe fn finish(info: &SHFILEINFO) -> Option<Rgba> {
    if info.i_icon >= 0 {
        if let Some(img) = icon_from_image_list(info.i_icon) {
            return Some(img);
        }
    }
    if info.h_icon == 0 {
        return None;
    }
    let img = hicon_to_rgba(info.h_icon);
    DestroyIcon(info.h_icon);
    img
}

unsafe fn icon_from_image_list(index: i32) -> Option<Rgba> {
    let mut list: *mut c_void = std::ptr::null_mut();
    if SHGetImageList(SHIL_EXTRALARGE, &IID_IIMAGELIST, &mut list) != S_OK || list.is_null() {
        return None;
    }
    let vt = vtable::<IImageListVtbl>(list);
    let mut hicon: HICON = 0;
    let hr = (vt.get_icon)(list, index, ILD_TRANSPARENT, &mut hicon);
    release(list);
    if hr != S_OK || hicon == 0 {
        return None;
    }
    let img = hicon_to_rgba(hicon);
    DestroyIcon(hicon);
    img
}

pub unsafe fn hicon_to_rgba(hicon: HICON) -> Option<Rgba> {
    let mut ii = ICONINFO::default();
    if GetIconInfo(hicon, &mut ii) == 0 {
        return None;
    }
    let mut out = None;
    if ii.hbm_color != 0 {
        out = bitmap_to_rgba(ii.hbm_color, ii.hbm_mask);
    }
    if ii.hbm_color != 0 {
        DeleteObject(ii.hbm_color);
    }
    if ii.hbm_mask != 0 {
        DeleteObject(ii.hbm_mask);
    }
    out
}

unsafe fn bitmap_to_rgba(hbm: HBITMAP, hbm_mask: HBITMAP) -> Option<Rgba> {
    let mut bm = BITMAP::default();
    if GetObjectW(
        hbm,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bm as *mut BITMAP as *mut c_void,
    ) == 0
    {
        return None;
    }
    let (w, h) = (bm.bm_width, bm.bm_height);
    if w <= 0 || h <= 0 || w > 256 || h > 256 {
        return None;
    }

    let hdc = CreateCompatibleDC(0);
    if hdc == 0 {
        return None;
    }

    let mut info = BITMAPINFO::default();
    info.header.bi_size = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.header.bi_width = w;
    info.header.bi_height = -h; // 负高度 = 自上而下
    info.header.bi_planes = 1;
    info.header.bi_bit_count = 32;
    info.header.bi_compression = 0; // BI_RGB

    let mut buf = vec![0u8; (w * h * 4) as usize];
    let lines = GetDIBits(
        hdc,
        hbm,
        0,
        h as u32,
        buf.as_mut_ptr() as *mut c_void,
        &mut info,
        0,
    );
    if lines == 0 {
        DeleteDC(hdc);
        return None;
    }

    // BGRA -> RGBA
    let mut has_alpha = false;
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
        if px[3] != 0 {
            has_alpha = true;
        }
    }

    if !has_alpha {
        // 老式图标没有 alpha 通道，用 mask 位图补（bit=1 表示透明）
        match read_mask(hdc, hbm_mask, w, h) {
            Some(opaque) => {
                for (i, px) in buf.chunks_exact_mut(4).enumerate() {
                    px[3] = if opaque[i] { 255 } else { 0 };
                }
            }
            None => {
                for px in buf.chunks_exact_mut(4) {
                    px[3] = 255;
                }
            }
        }
    }

    DeleteDC(hdc);
    Some(Rgba {
        w: w as u32,
        h: h as u32,
        data: buf,
    })
}

/// 读 1bpp mask 位图，返回每个像素是否不透明
unsafe fn read_mask(hdc: HDC, hbm_mask: HBITMAP, w: i32, h: i32) -> Option<Vec<bool>> {
    if hbm_mask == 0 {
        return None;
    }
    let stride = ((w + 31) / 32 * 4) as usize;
    let mut info = BITMAPINFO::default();
    info.header.bi_size = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.header.bi_width = w;
    info.header.bi_height = -h;
    info.header.bi_planes = 1;
    info.header.bi_bit_count = 1;
    info.header.bi_compression = 0;
    info.colors[0] = 0x0000_0000;
    info.colors[1] = 0x00FF_FFFF;

    let mut buf = vec![0u8; stride * h as usize];
    let lines = GetDIBits(
        hdc,
        hbm_mask,
        0,
        h as u32,
        buf.as_mut_ptr() as *mut c_void,
        &mut info,
        0,
    );
    if lines == 0 {
        return None;
    }

    let mut out = vec![true; (w * h) as usize];
    for row in 0..h as usize {
        for col in 0..w as usize {
            let byte = buf[row * stride + col / 8];
            let bit = (byte >> (7 - (col % 8))) & 1;
            out[row * w as usize + col] = bit == 0;
        }
    }
    Some(out)
}

/// 当前桌面壁纸路径
pub fn wallpaper_path() -> Option<PathBuf> {
    let mut buf = [0u16; 520];
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            buf.as_mut_ptr() as *mut c_void,
            0,
        )
    };
    if ok == 0 {
        return None;
    }
    let s = from_wide(&buf);
    if s.trim().is_empty() {
        return None;
    }
    let p = PathBuf::from(s);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// 读壁纸并缩到 max_w 宽（预览用，不需要原图分辨率）
pub fn wallpaper_rgba(max_w: u32) -> Option<Rgba> {
    let path = wallpaper_path()?;
    let img = image::open(&path).ok()?;
    let img = if img.width() > max_w {
        img.resize(max_w, u32::MAX, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let img = img.to_rgba8();
    Some(Rgba {
        w: img.width(),
        h: img.height(),
        data: img.into_raw(),
    })
}

/// 找不到图标时的兜底色（按名字散列到固定色相）
pub fn color_of(name: &str) -> (u8, u8, u8) {
    let mut h: i64 = 0;
    for c in name.chars() {
        h = (h * 31 + c as i64) & 0x7FFF_FFFF;
    }
    let hue = (h % 360) as f32;
    hsv_to_rgb(hue, 0.55, 0.62)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
