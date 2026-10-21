//! 手写 Windows FFI 绑定（user32 / kernel32 / shell32 / shlwapi / gdi32 / advapi32 / ole32）
//!
//! 为什么不用 windows-rs：这里只用到十来个 API 和 5 个 COM 接口，
//! 手写 vtable 可以精确对齐 C# 版里已经验证过的调用顺序，并且编译零依赖、产物更小。

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type HWND = isize;
pub type HANDLE = isize;
pub type HICON = isize;
pub type HBITMAP = isize;
pub type HDC = isize;
pub type HGDIOBJ = isize;
pub type HRESULT = i32;
pub type BOOL = i32;
pub type LPARAM = isize;
pub type WPARAM = usize;
pub type LRESULT = isize;

pub const TRUE: BOOL = 1;
pub const S_OK: HRESULT = 0;

// ---------------------------------------------------------------- ListView 消息
pub const LVM_FIRST: u32 = 0x1000;
pub const LVM_GETITEMCOUNT: u32 = LVM_FIRST + 4;
pub const LVM_GETITEMPOSITION: u32 = LVM_FIRST + 16;
pub const LVM_SETITEMPOSITION32: u32 = LVM_FIRST + 49;
pub const LVM_GETITEMTEXTW: u32 = LVM_FIRST + 115;

pub const LVIF_TEXT: u32 = 0x0001;
pub const LVS_AUTOARRANGE: i32 = 0x0100;
pub const GWL_STYLE: i32 = -16;
pub const SMTO_ABORTIFHUNG: u32 = 0x0002;
pub const SMTO_NORMAL: u32 = 0x0000;

// ---------------------------------------------------------------- 进程内存
pub const PROCESS_VM_OPERATION: u32 = 0x0008;
pub const PROCESS_VM_READ: u32 = 0x0010;
pub const PROCESS_VM_WRITE: u32 = 0x0020;
pub const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_RELEASE: u32 = 0x8000;
pub const PAGE_READWRITE: u32 = 0x04;

// ---------------------------------------------------------------- shell
pub const SHGFI_ICON: u32 = 0x0000_0100;
pub const SHGFI_LARGEICON: u32 = 0x0000_0000;
pub const SHGFI_PIDL: u32 = 0x0000_0008;
pub const SHGFI_SYSICONINDEX: u32 = 0x0000_4000;
pub const SHGDN_INFOLDER: u32 = 0x0001;
pub const SVSI_POSITIONITEM: u32 = 0x0080;
pub const SWC_DESKTOP: i32 = 8;
pub const SWFO_NEEDDISPATCH: i32 = 1;
pub const SHIL_LARGE: i32 = 0;
pub const SHIL_EXTRALARGE: i32 = 2;
pub const ILD_TRANSPARENT: u32 = 0x0000_0001;
pub const SPI_GETDESKWALLPAPER: u32 = 0x0073;
pub const SPI_GETWORKAREA: u32 = 0x0030;

// ---------------------------------------------------------------- COM
pub const CLSCTX_ALL: u32 = 0x17;
pub const COINIT_APARTMENTTHREADED: u32 = 0x2;

// ---------------------------------------------------------------- 注册表
pub const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
pub const KEY_READ: u32 = 0x2_0019;
pub const KEY_SET_VALUE: u32 = 0x0002;
pub const REG_BINARY: u32 = 3;
pub const ERROR_SUCCESS: i32 = 0;
pub const ERROR_FILE_NOT_FOUND: i32 = 2;

// ---------------------------------------------------------------- 结构体
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

impl POINT {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 64 位下的 LVITEMW 布局（与 C# / ctypes 版一致）
#[repr(C)]
pub struct LVITEMW {
    pub mask: u32,
    pub i_item: i32,
    pub i_sub_item: i32,
    pub state: u32,
    pub state_mask: u32,
    pub psz_text: *mut u16,
    pub cch_text_max: i32,
    pub i_image: i32,
    pub l_param: isize,
    pub i_indent: i32,
    pub i_group_id: i32,
    pub c_columns: u32,
    pub pu_columns: *mut u32,
    pub pi_col_fmt: *mut i32,
    pub i_group: i32,
}

impl Default for LVITEMW {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
pub struct ITEMIDLIST {
    pub data: u8,
}

/// STRRET：uType 之后是联合体（x64 下按 8 字节对齐，整体 272 字节）
#[repr(C)]
pub struct STRRET {
    pub u_type: u32,
    pub _pad: u32,
    pub data: [u8; 264],
}

impl Default for STRRET {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
pub struct SHFILEINFO {
    pub h_icon: HICON,
    pub i_icon: i32,
    pub dw_attributes: u32,
    pub sz_display_name: [u16; 260],
    pub sz_type_name: [u16; 80],
}

impl Default for SHFILEINFO {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const CLSID_SHELLWINDOWS: GUID = GUID {
    data1: 0x9BA0_5972,
    data2: 0xF6A8,
    data3: 0x11CF,
    data4: [0xA4, 0x42, 0x00, 0xA0, 0xC9, 0x0A, 0x8F, 0x39],
};
pub const IID_ISHELLWINDOWS: GUID = GUID {
    data1: 0x85CB_6900,
    data2: 0x4D95,
    data3: 0x11CF,
    data4: [0x96, 0x0C, 0x00, 0x80, 0xC7, 0xF4, 0xEE, 0x85],
};
pub const IID_ISERVICEPROVIDER: GUID = GUID {
    data1: 0x6D51_40C1,
    data2: 0x7436,
    data3: 0x11CE,
    data4: [0x80, 0x34, 0x00, 0xAA, 0x00, 0x60, 0x09, 0xFA],
};
pub const SID_STOPLEVELBROWSER: GUID = GUID {
    data1: 0x4C96_BE40,
    data2: 0x915C,
    data3: 0x11CF,
    data4: [0x99, 0xD3, 0x00, 0xAA, 0x00, 0x4A, 0xE8, 0x37],
};
pub const IID_ISHELLBROWSER: GUID = GUID {
    data1: 0x0002_14E2,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
pub const IID_IFOLDERVIEW: GUID = GUID {
    data1: 0xCDE7_25B0,
    data2: 0xCCC9,
    data3: 0x4519,
    data4: [0x91, 0x7E, 0x32, 0x5D, 0x72, 0xFA, 0xB4, 0xCE],
};
pub const IID_ISHELLFOLDER: GUID = GUID {
    data1: 0x0002_14E6,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
pub const IID_IIMAGELIST: GUID = GUID {
    data1: 0x46EB_5926,
    data2: 0x582E,
    data3: 0x4017,
    data4: [0x9F, 0xDF, 0xE8, 0x99, 0x8D, 0xAA, 0x09, 0x50],
};

/// VARIANT（x64 下 24 字节，只用 VT_I4 承载 CSIDL）
#[repr(C)]
pub struct VARIANT {
    pub vt: u16,
    pub r1: u16,
    pub r2: u16,
    pub r3: u16,
    pub data: [u8; 16],
}

impl VARIANT {
    pub fn from_i32(v: i32) -> Self {
        let mut var: VARIANT = unsafe { std::mem::zeroed() };
        var.vt = 3; // VT_I4
        var.data[0..4].copy_from_slice(&v.to_le_bytes());
        var
    }
}

#[repr(C)]
pub struct ICONINFO {
    pub f_icon: BOOL,
    pub x_hotspot: u32,
    pub y_hotspot: u32,
    pub hbm_mask: HBITMAP,
    pub hbm_color: HBITMAP,
}

impl Default for ICONINFO {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
pub struct BITMAP {
    pub bm_type: i32,
    pub bm_width: i32,
    pub bm_height: i32,
    pub bm_width_bytes: i32,
    pub bm_planes: u16,
    pub bm_bits_pixel: u16,
    pub bm_bits: *mut c_void,
}

impl Default for BITMAP {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct BITMAPINFOHEADER {
    pub bi_size: u32,
    pub bi_width: i32,
    pub bi_height: i32,
    pub bi_planes: u16,
    pub bi_bit_count: u16,
    pub bi_compression: u32,
    pub bi_size_image: u32,
    pub bi_x_pels_per_meter: i32,
    pub bi_y_pels_per_meter: i32,
    pub bi_clr_used: u32,
    pub bi_clr_important: u32,
}

#[repr(C)]
pub struct BITMAPINFO {
    pub header: BITMAPINFOHEADER,
    pub colors: [u32; 256],
}

impl Default for BITMAPINFO {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// ---------------------------------------------------------------- 导入
#[link(name = "user32")]
extern "system" {
    pub fn FindWindowExW(parent: HWND, child_after: HWND, class: *const u16, title: *const u16) -> HWND;
    pub fn EnumWindows(cb: extern "system" fn(HWND, LPARAM) -> BOOL, lparam: LPARAM) -> BOOL;
    pub fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
    pub fn SendMessageTimeoutW(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> LRESULT;
    pub fn GetWindowLongW(hwnd: HWND, index: i32) -> i32;
    pub fn SetWindowLongW(hwnd: HWND, index: i32, value: i32) -> i32;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn SetProcessDpiAwarenessContext(ctx: isize) -> BOOL;
    pub fn DestroyIcon(icon: HICON) -> BOOL;
    pub fn GetIconInfo(icon: HICON, info: *mut ICONINFO) -> BOOL;
    pub fn SystemParametersInfoW(action: u32, param: u32, data: *mut c_void, win_ini: u32) -> BOOL;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn OpenProcess(access: u32, inherit: BOOL, pid: u32) -> HANDLE;
    pub fn VirtualAllocEx(proc_: HANDLE, addr: *const c_void, size: usize, alloc_type: u32, protect: u32) -> *mut c_void;
    pub fn VirtualFreeEx(proc_: HANDLE, addr: *mut c_void, size: usize, free_type: u32) -> BOOL;
    pub fn ReadProcessMemory(proc_: HANDLE, addr: *const c_void, buf: *mut c_void, size: usize, read: *mut usize) -> BOOL;
    pub fn WriteProcessMemory(proc_: HANDLE, addr: *mut c_void, buf: *const c_void, size: usize, written: *mut usize) -> BOOL;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn GetLastError() -> u32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn SHGetFileInfoW(path: *const u16, attrs: u32, info: *mut SHFILEINFO, size: u32, flags: u32) -> usize;
    pub fn SHParseDisplayName(name: *const u16, bind_ctx: *const c_void, pidl: *mut *mut ITEMIDLIST, attrs_in: u32, attrs_out: *mut u32) -> HRESULT;
    pub fn ILFree(pidl: *mut ITEMIDLIST);
    pub fn SHGetDesktopFolder(folder: *mut *mut c_void) -> HRESULT;
    pub fn SHGetImageList(which: i32, riid: *const GUID, out: *mut *mut c_void) -> HRESULT;
}

#[link(name = "shlwapi")]
extern "system" {
    pub fn StrRetToBufW(strret: *mut STRRET, pidl: *const ITEMIDLIST, buf: *mut u16, cch: u32) -> HRESULT;
}

#[link(name = "ole32")]
extern "system" {
    pub fn CoInitializeEx(reserved: *const c_void, coinit: u32) -> HRESULT;
    pub fn CoCreateInstance(rclsid: *const GUID, outer: *const c_void, clsctx: u32, riid: *const GUID, out: *mut *mut c_void) -> HRESULT;
    pub fn CoTaskMemFree(p: *const c_void);
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
    pub fn DeleteDC(hdc: HDC) -> BOOL;
    pub fn GetDIBits(hdc: HDC, bmp: HBITMAP, start: u32, lines: u32, bits: *mut c_void, info: *mut BITMAPINFO, usage: u32) -> i32;
    pub fn DeleteObject(obj: HGDIOBJ) -> BOOL;
    pub fn GetObjectW(obj: HGDIOBJ, size: i32, out: *mut c_void) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegOpenKeyExW(key: isize, sub: *const u16, options: u32, sam: u32, out: *mut isize) -> i32;
    pub fn RegQueryValueExW(key: isize, name: *const u16, reserved: *mut u32, ty: *mut u32, data: *mut u8, cb: *mut u32) -> i32;
    pub fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
    pub fn RegCloseKey(key: isize) -> i32;
}

// ---------------------------------------------------------------- COM vtable
#[repr(C)]
pub struct IUnknownVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
pub struct IShellWindowsVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_type_info_count: usize,
    pub get_type_info: usize,
    pub get_ids_of_names: usize,
    pub invoke: usize,
    pub get_count: usize,
    pub item: usize,
    pub new_enum: usize,
    pub register: usize,
    pub register_pending: usize,
    pub revoke: usize,
    pub on_navigate: usize,
    pub on_activated: usize,
    pub find_window_sw: unsafe extern "system" fn(
        *mut c_void,
        *const VARIANT,
        *const VARIANT,
        i32,
        *mut i32,
        i32,
        *mut *mut c_void,
    ) -> HRESULT,
    pub on_created: usize,
    pub process_attach_detach: usize,
}

#[repr(C)]
pub struct IServiceProviderVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub query_service:
        unsafe extern "system" fn(*mut c_void, *const GUID, *const GUID, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct IShellBrowserVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_window: usize,
    pub context_sensitive_help: usize,
    pub insert_menus_sb: usize,
    pub set_menu_sb: usize,
    pub remove_menus_sb: usize,
    pub set_status_text_sb: usize,
    pub enable_modeless_sb: usize,
    pub translate_accelerator_sb: usize,
    pub browse_object: usize,
    pub get_view_state_stream: usize,
    pub get_control_window: usize,
    pub send_control_msg: usize,
    pub query_active_shell_view: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct IFolderViewVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_current_view_mode: usize,
    pub set_current_view_mode: usize,
    pub get_folder: usize,
    pub item: unsafe extern "system" fn(*mut c_void, i32, *mut *mut ITEMIDLIST) -> HRESULT,
    pub item_count: unsafe extern "system" fn(*mut c_void, u32, *mut i32) -> HRESULT,
    pub items: usize,
    pub get_selection_marked_item: usize,
    pub get_focused_item: usize,
    pub get_item_position: unsafe extern "system" fn(*mut c_void, *const ITEMIDLIST, *mut POINT) -> HRESULT,
    pub get_spacing: usize,
    pub get_default_spacing: usize,
    pub get_auto_arrange: usize,
    pub select_item: usize,
    pub select_and_position_items:
        unsafe extern "system" fn(*mut c_void, u32, *const *const ITEMIDLIST, *const POINT, u32) -> HRESULT,
}

#[repr(C)]
pub struct IShellFolderVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub parse_display_name: usize,
    pub enum_objects: usize,
    pub bind_to_object: usize,
    pub bind_to_storage: usize,
    pub compare_ids: usize,
    pub create_view_object: usize,
    pub get_attributes_of: usize,
    pub get_ui_object_of: usize,
    pub get_display_name_of: unsafe extern "system" fn(*mut c_void, *const ITEMIDLIST, u32, *mut STRRET) -> HRESULT,
    pub set_name_of: usize,
}

#[repr(C)]
pub struct IImageListVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub add: usize,
    pub replace_icon: usize,
    pub set_overlay_image: usize,
    pub replace: usize,
    pub add_masked: usize,
    pub draw: usize,
    pub remove: usize,
    pub get_icon: unsafe extern "system" fn(*mut c_void, i32, u32, *mut HICON) -> HRESULT,
}

// ---------------------------------------------------------------- 辅助
/// 取接口的 vtable 引用
pub unsafe fn vtable<T>(ptr: *mut c_void) -> &'static T {
    &**(ptr as *const *const T)
}

pub unsafe fn release(ptr: *mut c_void) -> u32 {
    if ptr.is_null() {
        return 0;
    }
    (vtable::<IUnknownVtbl>(ptr).release)(ptr)
}

pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// DPI 感知：让所有坐标都是物理像素，避免 125% / 150% 缩放下整体偏移
pub fn set_dpi_aware() {
    unsafe {
        // PER_MONITOR_AWARE_V2 = -4
        if SetProcessDpiAwarenessContext(-4) != 0 {
            return;
        }
        // 老系统兜底：shcore!SetProcessDpiAwareness(2) / user32!SetProcessDPIAware()
        #[link(name = "shcore")]
        extern "system" {
            fn SetProcessDpiAwareness(value: u32) -> HRESULT;
        }
        if SetProcessDpiAwareness(2) == S_OK {
            return;
        }
        #[link(name = "user32")]
        extern "system" {
            fn SetProcessDPIAware() -> BOOL;
        }
        SetProcessDPIAware();
    }
}

pub fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(0), GetSystemMetrics(1)) }
}

/// 桌面工作区（不含任务栏），物理像素
pub fn work_area() -> (i32, i32, i32, i32) {
    let mut r = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut r as *mut RECT as *mut c_void,
            0,
        )
    };
    if ok == 0 || r.right <= r.left || r.bottom <= r.top {
        let (w, h) = screen_size();
        return (0, 0, w, h);
    }
    (r.left, r.top, r.right - r.left, r.bottom - r.top)
}

/// 系统 DPI 缩放（125% -> 1.25）
pub fn dpi_scale() -> f32 {
    #[link(name = "user32")]
    extern "system" {
        fn GetDpiForSystem() -> u32;
    }
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

pub fn last_error() -> u32 {
    unsafe { GetLastError() }
}

/// 初始化 COM（STA）。返回 false 表示已经初始化过（可以继续用）
pub fn com_init() -> bool {
    unsafe {
        let hr = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED);
        hr == S_OK || hr == 1 // S_FALSE 表示已初始化
    }
}
