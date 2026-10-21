//! 跨进程访问 explorer 里的桌面图标 ListView（SysListView32）
//!
//! 读名字 / 读坐标 / 写坐标都要把结构体放进 explorer 的地址空间再发消息，
//! 这是我们自己的进程看不到别人的内存导致的，老牌做法。

use crate::model::IconPos;
use crate::sys::*;
use anyhow::{bail, Result};
use std::ffi::c_void;

const REMOTE_SIZE: usize = 4096;
const TEXT_OFF: usize = 1024;
const SEND_TIMEOUT_MS: u32 = 3000;

pub struct Desktop {
    lv: HWND,
    pid: u32,
    proc_: HANDLE,
    remote: *mut c_void,
}

impl Desktop {
    pub fn open() -> Result<Self> {
        let lv = find_listview();
        if lv == 0 {
            bail!("找不到桌面图标列表（explorer 未运行或桌面图标已隐藏）");
        }

        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(lv, &mut pid) };

        let access =
            PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION;
        let proc_ = unsafe { OpenProcess(access, 0, pid) };
        if proc_ == 0 {
            let err = last_error();
            if err == 5 {
                bail!("没有权限访问 explorer 进程，请用普通用户身份运行（不要用管理员）");
            }
            bail!("无法打开 explorer 进程（错误 {}）", err);
        }

        let remote = unsafe {
            VirtualAllocEx(
                proc_,
                std::ptr::null(),
                REMOTE_SIZE,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if remote.is_null() {
            unsafe { CloseHandle(proc_) };
            bail!("在 explorer 中分配内存失败");
        }

        Ok(Desktop {
            lv,
            pid,
            proc_,
            remote,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn hwnd(&self) -> HWND {
        self.lv
    }

    fn send(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Result<LRESULT> {
        let mut result: usize = 0;
        let ok = unsafe {
            SendMessageTimeoutW(
                self.lv,
                msg,
                wparam,
                lparam,
                SMTO_ABORTIFHUNG,
                SEND_TIMEOUT_MS,
                &mut result,
            )
        };
        if ok == 0 {
            bail!("与桌面通讯超时（消息 0x{:X}）", msg);
        }
        Ok(result as LRESULT)
    }

    fn read(&self, addr: *mut c_void, size: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; size];
        let mut got: usize = 0;
        let ok = unsafe {
            ReadProcessMemory(
                self.proc_,
                addr,
                buf.as_mut_ptr() as *mut c_void,
                size,
                &mut got,
            )
        };
        if ok == 0 {
            bail!("读取 explorer 内存失败");
        }
        Ok(buf)
    }

    fn write(&self, addr: *mut c_void, data: &[u8]) -> Result<()> {
        let mut written: usize = 0;
        let ok = unsafe {
            WriteProcessMemory(
                self.proc_,
                addr,
                data.as_ptr() as *const c_void,
                data.len(),
                &mut written,
            )
        };
        if ok == 0 {
            bail!("写入 explorer 内存失败");
        }
        Ok(())
    }

    fn remote_ptr(&self, offset: usize) -> *mut c_void {
        unsafe { (self.remote as *mut u8).add(offset) as *mut c_void }
    }

    pub fn count(&self) -> Result<i32> {
        Ok(self.send(LVM_GETITEMCOUNT, 0, 0)? as i32)
    }

    pub fn auto_arrange(&self) -> bool {
        unsafe { GetWindowLongW(self.lv, GWL_STYLE) & LVS_AUTOARRANGE != 0 }
    }

    /// 只清样式位，绝对不要跟着发 LVM_ARRANGE —— 那会立刻把所有图标重排一遍，
    /// 把刚写好的位置冲掉（v0.0.1「要点好几次」的原因之一）
    pub fn disable_auto_arrange(&self) -> bool {
        let style = unsafe { GetWindowLongW(self.lv, GWL_STYLE) };
        if style & LVS_AUTOARRANGE == 0 {
            return false;
        }
        unsafe { SetWindowLongW(self.lv, GWL_STYLE, style & !LVS_AUTOARRANGE) };
        true
    }

    pub fn item_name(&self, index: i32) -> Result<String> {
        let text_addr = self.remote_ptr(TEXT_OFF);
        let item = LVITEMW {
            mask: LVIF_TEXT,
            i_item: 0,
            i_sub_item: 0,
            psz_text: text_addr as *mut u16,
            cch_text_max: 260,
            ..Default::default()
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(&item as *const LVITEMW as *const u8, std::mem::size_of::<LVITEMW>())
        };
        self.write(self.remote, bytes)?;
        self.send(LVM_GETITEMTEXTW, index as WPARAM, self.remote as LPARAM)?;

        let raw = self.read(text_addr, 520)?;
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(from_wide(&units))
    }

    pub fn item_pos(&self, index: i32) -> Result<(i32, i32)> {
        self.write(self.remote, &[0u8; 8])?;
        self.send(LVM_GETITEMPOSITION, index as WPARAM, self.remote as LPARAM)?;
        let raw = self.read(self.remote, 8)?;
        let x = i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let y = i32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        Ok((x, y))
    }

    /// 用 SETITEMPOSITION32 + 远程 POINT：支持负坐标（副屏在主屏左侧）和 > 65535 的坐标
    pub fn set_pos(&self, index: i32, x: i32, y: i32) -> Result<()> {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&x.to_le_bytes());
        buf[4..8].copy_from_slice(&y.to_le_bytes());
        self.write(self.remote, &buf)?;
        self.send(LVM_SETITEMPOSITION32, index as WPARAM, self.remote as LPARAM)?;
        Ok(())
    }

    /// 当前桌面所有图标的 (名字, x, y)
    pub fn snapshot(&self) -> Result<Vec<IconPos>> {
        let n = self.count()?;
        let mut out = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n {
            let (x, y) = self.item_pos(i)?;
            out.push(IconPos {
                name: self.item_name(i)?,
                x,
                y,
            });
        }
        Ok(out)
    }

    /// 名字 -> 序号 的映射（大小写不敏感）
    pub fn index_map(&self) -> Result<std::collections::HashMap<String, i32>> {
        let n = self.count()?;
        let mut map = std::collections::HashMap::new();
        for i in 0..n {
            map.insert(self.item_name(i)?.to_lowercase(), i);
        }
        Ok(map)
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            if !self.remote.is_null() {
                VirtualFreeEx(self.proc_, self.remote, 0, MEM_RELEASE);
                self.remote = std::ptr::null_mut();
            }
            if self.proc_ != 0 {
                CloseHandle(self.proc_);
                self.proc_ = 0;
            }
        }
    }
}

/// 定位桌面图标所在的 SysListView32（兼容 WorkerW 壁纸宿主）
fn find_listview() -> HWND {
    struct Ctx {
        found: HWND,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut Ctx);
        if ctx.found != 0 {
            return 0; // 已经找到，停止枚举
        }
        let def_view = unsafe {
            FindWindowExW(
                hwnd,
                0,
                to_wide("SHELLDLL_DefView").as_ptr(),
                std::ptr::null(),
            )
        };
        if def_view != 0 {
            let lv = unsafe {
                FindWindowExW(
                    def_view,
                    0,
                    to_wide("SysListView32").as_ptr(),
                    std::ptr::null(),
                )
            };
            if lv != 0 {
                ctx.found = lv;
                return 0;
            }
        }
        1
    }

    let mut ctx = Ctx { found: 0 };
    unsafe {
        // EnumWindows 的回调类型是 unsafe extern "system" fn，这里直接传
        let cb_ptr: extern "system" fn(HWND, LPARAM) -> BOOL = std::mem::transmute::<
            unsafe extern "system" fn(HWND, LPARAM) -> BOOL,
            extern "system" fn(HWND, LPARAM) -> BOOL,
        >(cb);
        EnumWindows(cb_ptr, &mut ctx as *mut Ctx as LPARAM);
    }
    ctx.found
}
