//! 通过 shell 自己的 IFolderView 设置图标位置
//!
//! 为什么不直接写 explorer 内存：
//!   1) shell 的文件夹视图内部状态没变，explorer 一刷新就把位置纠回去，
//!      所以以前要点好几次才收敛；
//!   2) Win10 1809+ shell 用注册表 `Bags\1\Desktop\IconLayouts` 保存位置，
//!      内存改写不会写进这个值，重启后被旧值覆盖 → 布局全乱。
//!   `IFolderView::SelectAndPositionItems` 是官方接口，shell 会自己更新
//!   内部状态并立即持久化。
//!
//! 链路：IShellWindows → FindWindowSW(SWC_DESKTOP) → IServiceProvider
//!       → QueryService(SID_STopLevelBrowser) → IShellBrowser
//!       → QueryActiveShellView → IShellView → QueryInterface(IFolderView)

use crate::sys::*;
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

pub struct ShellItem {
    pub name: String,
    pub pidl: *mut ITEMIDLIST,
    pub x: i32,
    pub y: i32,
}

pub struct ShellView {
    fv: *mut c_void,
    desk: *mut c_void,
    items: Vec<ShellItem>,
}

impl ShellView {
    /// 拿不到桌面视图时返回 None（例如非交互会话），调用方回退到内存方式
    pub fn try_open() -> Option<Self> {
        unsafe {
            let sw = {
                let mut p: *mut c_void = null_mut();
                let hr = CoCreateInstance(
                    &CLSID_SHELLWINDOWS,
                    std::ptr::null(),
                    CLSCTX_ALL,
                    &IID_ISHELLWINDOWS,
                    &mut p,
                );
                if hr != S_OK || p.is_null() {
                    return None;
                }
                p
            };

            let varloc = VARIANT::from_i32(0); // CSIDL_DESKTOP
            let varroot: VARIANT = std::mem::zeroed();
            let mut hwnd: i32 = 0;
            let mut disp: *mut c_void = null_mut();
            let hr = (vtable::<IShellWindowsVtbl>(sw).find_window_sw)(
                sw,
                &varloc,
                &varroot,
                SWC_DESKTOP,
                &mut hwnd,
                SWFO_NEEDDISPATCH,
                &mut disp,
            );
            release(sw);
            if hr != S_OK || disp.is_null() {
                return None;
            }

            let sp = {
                let mut p: *mut c_void = null_mut();
                let hr =
                    (vtable::<IUnknownVtbl>(disp).query_interface)(disp, &IID_ISERVICEPROVIDER, &mut p);
                release(disp);
                if hr != S_OK || p.is_null() {
                    return None;
                }
                p
            };

            let browser = {
                let mut p: *mut c_void = null_mut();
                let hr = (vtable::<IServiceProviderVtbl>(sp).query_service)(
                    sp,
                    &SID_STOPLEVELBROWSER,
                    &IID_ISHELLBROWSER,
                    &mut p,
                );
                release(sp);
                if hr != S_OK || p.is_null() {
                    return None;
                }
                p
            };

            let view = {
                let mut p: *mut c_void = null_mut();
                let hr = (vtable::<IShellBrowserVtbl>(browser).query_active_shell_view)(browser, &mut p);
                release(browser);
                if hr != S_OK || p.is_null() {
                    return None;
                }
                p
            };

            let fv = {
                let mut p: *mut c_void = null_mut();
                let hr = (vtable::<IUnknownVtbl>(view).query_interface)(view, &IID_IFOLDERVIEW, &mut p);
                release(view);
                if hr != S_OK || p.is_null() {
                    return None;
                }
                p
            };

            let mut desk: *mut c_void = null_mut();
            if SHGetDesktopFolder(&mut desk) != S_OK || desk.is_null() {
                release(fv);
                return None;
            }

            let mut sv = ShellView {
                fv,
                desk,
                items: Vec::new(),
            };
            if sv.reload().is_err() || sv.items.is_empty() {
                return None;
            }
            Some(sv)
        }
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn items(&self) -> &[ShellItem] {
        &self.items
    }

    /// 名字取自 shell 的显示名，与布局里保存的完全一致
    pub fn reload(&mut self) -> Result<()> {
        unsafe {
            self.free_items();
            let mut n: i32 = 0;
            let hr = (vtable::<IFolderViewVtbl>(self.fv).item_count)(self.fv, 0, &mut n);
            if hr != S_OK {
                bail!("枚举桌面条目失败（0x{:08X}）", hr);
            }

            let mut strret = STRRET::default();
            let fv = vtable::<IFolderViewVtbl>(self.fv);

            for i in 0..n {
                let mut pidl: *mut ITEMIDLIST = null_mut();
                if (fv.item)(self.fv, i, &mut pidl) != S_OK || pidl.is_null() {
                    continue;
                }
                let mut pt = POINT::default();
                if (fv.get_item_position)(self.fv, pidl, &mut pt) != S_OK {
                    CoTaskMemFree(pidl as *const c_void);
                    continue;
                }
                let name = self
                    .display_name(pidl, &mut strret)
                    .unwrap_or_else(|| format!("#{}", i));
                self.items.push(ShellItem {
                    name,
                    pidl,
                    x: pt.x,
                    y: pt.y,
                });
            }
            Ok(())
        }
    }

    unsafe fn display_name(&self, pidl: *const ITEMIDLIST, strret: &mut STRRET) -> Option<String> {
        *strret = STRRET::default();
        let hr = (vtable::<IShellFolderVtbl>(self.desk).get_display_name_of)(
            self.desk,
            pidl,
            SHGDN_INFOLDER,
            strret,
        );
        if hr != S_OK {
            return None;
        }
        let mut buf = [0u16; 300];
        let hr2 = StrRetToBufW(strret, pidl, buf.as_mut_ptr(), buf.len() as u32);
        // pOleStr 是 shell 分配的，取完要还回去
        if strret.u_type == 0 {
            let p = *(strret.data.as_ptr() as *const *const c_void);
            if !p.is_null() {
                CoTaskMemFree(p);
            }
        }
        if hr2 != S_OK {
            return None;
        }
        Some(from_wide(&buf))
    }

    /// 批量设置位置。返回 (移动数, 最终仍不一致的图标名, 轮数)。
    /// shell 的网格吸附是异步的，写完必须读回核对，不一致就再来一轮。
    pub fn apply(
        &mut self,
        want: &HashMap<String, (i32, i32)>,
        max_rounds: usize,
    ) -> Result<(usize, Vec<String>, usize)> {
        let mut moved = 0usize;
        let mut rounds = 0usize;

        for round in 0..max_rounds {
            self.reload()?;

            let mut pidls: Vec<*const ITEMIDLIST> = Vec::new();
            let mut pts: Vec<POINT> = Vec::new();
            for it in &self.items {
                let Some(&(x, y)) = want.get(&it.name.to_lowercase()) else {
                    continue;
                };
                if it.x == x && it.y == y {
                    continue;
                }
                pidls.push(it.pidl as *const ITEMIDLIST);
                pts.push(POINT::new(x, y));
            }
            if pidls.is_empty() {
                break;
            }

            rounds += 1;
            if round == 0 {
                moved = pidls.len();
            }
            let hr = unsafe {
                (vtable::<IFolderViewVtbl>(self.fv).select_and_position_items)(
                    self.fv,
                    pidls.len() as u32,
                    pidls.as_ptr(),
                    pts.as_ptr(),
                    SVSI_POSITIONITEM,
                )
            };
            if hr != S_OK {
                bail!("设置图标位置失败（0x{:08X}）", hr);
            }
            std::thread::sleep(std::time::Duration::from_millis(160));
        }

        self.reload()?;
        let mut failed = Vec::new();
        for it in &self.items {
            let Some(&(x, y)) = want.get(&it.name.to_lowercase()) else {
                continue;
            };
            if it.x != x || it.y != y {
                failed.push(it.name.clone());
            }
        }
        Ok((moved, failed, rounds.max(1)))
    }

    fn free_items(&mut self) {
        for it in self.items.drain(..) {
            if !it.pidl.is_null() {
                unsafe { CoTaskMemFree(it.pidl as *const c_void) };
            }
        }
    }
}

impl Drop for ShellView {
    fn drop(&mut self) {
        unsafe {
            self.free_items();
            if !self.desk.is_null() {
                release(self.desk);
                self.desk = null_mut();
            }
            if !self.fv.is_null() {
                release(self.fv);
                self.fv = null_mut();
            }
        }
    }
}
