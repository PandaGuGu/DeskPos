#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
deskpos.py -- 记住 / 恢复 Windows 桌面图标位置

设计目标：零常驻、零依赖、按需运行。
  * 纯标准库 ctypes，不需要 pip install 任何东西
  * 平时不运行，不占任何内存；执行完立刻退出
  * 布局存成人类可读的 JSON

用法:
  python deskpos.py save  [名称]        保存当前桌面布局（默认名 auto-时间戳）
  python deskpos.py restore [名称]      恢复布局（留空 = 最近一次）
  python deskpos.py list                列出所有已保存布局
  python deskpos.py show  [名称]        查看布局内容
  python deskpos.py diff  [名称]        预览：恢复后哪些图标会动（不改动桌面）
  python deskpos.py delete 名称         删除布局

注意事项:
  * 桌面右键 -> 查看 -> 必须关闭「自动排列图标」，否则恢复会被立刻重排
    （脚本检测到会自动帮你关掉，并给出提示）
  * 图标按显示名称匹配，重命名文件后该图标的位置会失配
  * Windows 10 1809+ 之后 explorer 自己也记一份位置，
    内存改写的位置在 explorer 重启后可能丢失，重新 restore 一次即可
"""

import ctypes
import ctypes.wintypes as wintypes
import json
import os
import sys
import time

# ---------------------------------------------------------------- Win32 常量
LVM_FIRST = 0x1000
LVM_GETITEMCOUNT = LVM_FIRST + 4
LVM_GETITEMPOSITION = LVM_FIRST + 16
LVM_SETITEMPOSITION32 = LVM_FIRST + 49
LVM_GETITEMTEXTW = LVM_FIRST + 115
LVM_ARRANGE = LVM_FIRST + 22
LVA_DEFAULT = 0x0000

LVIF_TEXT = 0x0001
LVS_AUTOARRANGE = 0x0100
GWL_STYLE = -16
SMTO_ABORTIFHUNG = 0x0002
SEND_TIMEOUT = 2000

PROCESS_VM_OPERATION = 0x0008
PROCESS_VM_READ = 0x0010
PROCESS_VM_WRITE = 0x0020
PROCESS_QUERY_INFORMATION = 0x0400
MEM_COMMIT = 0x1000
MEM_RESERVE = 0x2000
MEM_RELEASE = 0x8000
PAGE_READWRITE = 0x04

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

user32.FindWindowW.restype = wintypes.HWND
user32.FindWindowExW.restype = wintypes.HWND
user32.FindWindowExW.argtypes = [wintypes.HWND, wintypes.HWND, wintypes.LPCWSTR, wintypes.LPCWSTR]
kernel32.OpenProcess.restype = wintypes.HANDLE
kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel32.VirtualAllocEx.restype = wintypes.LPVOID
kernel32.VirtualAllocEx.argtypes = [wintypes.HANDLE, wintypes.LPVOID, ctypes.c_size_t,
                                    wintypes.DWORD, wintypes.DWORD]
kernel32.VirtualFreeEx.argtypes = [wintypes.HANDLE, wintypes.LPVOID, ctypes.c_size_t, wintypes.DWORD]
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]

ENUM_PROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


class POINT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


class LVITEMW(ctypes.Structure):
    """64 位下的 LVITEMW 布局，ctypes 自动处理对齐"""
    _fields_ = [
        ("mask", ctypes.c_uint),
        ("iItem", ctypes.c_int),
        ("iSubItem", ctypes.c_int),
        ("state", ctypes.c_uint),
        ("stateMask", ctypes.c_uint),
        ("pszText", ctypes.c_void_p),
        ("cchTextMax", ctypes.c_int),
        ("iImage", ctypes.c_int),
        ("lParam", ctypes.c_ssize_t),
        ("iIndent", ctypes.c_int),
        ("iGroupId", ctypes.c_int),
        ("cColumns", ctypes.c_uint),
        ("puColumns", ctypes.c_void_p),
        ("piColFmt", ctypes.c_void_p),
        ("iGroup", ctypes.c_int),
    ]


# ---------------------------------------------------------------- 基础封装
def enable_dpi_awareness():
    """让坐标使用物理像素，避免 125%/150% 缩放下错位"""
    try:
        # PER_MONITOR_AWARE_V2 = -4
        user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
        return
    except Exception:
        pass
    try:
        ctypes.WinDLL("shcore").SetProcessDpiAwareness(2)
    except Exception:
        try:
            user32.SetProcessDPIAware()
        except Exception:
            pass


def send_msg(hwnd, msg, wparam, lparam):
    result = ctypes.c_size_t(0)
    ok = user32.SendMessageTimeoutW(hwnd, msg, wparam, lparam,
                                    SMTO_ABORTIFHUNG, SEND_TIMEOUT, ctypes.byref(result))
    if not ok:
        raise OSError(f"SendMessageTimeout 失败 (msg=0x{msg:X}), last_error={ctypes.get_last_error()}")
    return result.value


def find_desktop_listview():
    """定位桌面图标所在的 SysListView32（兼容 WorkerW 壁纸场景）"""
    found = []

    def _check_top(hwnd, _):
        defview = user32.FindWindowExW(hwnd, None, "SHELLDLL_DefView", None)
        if defview:
            lv = user32.FindWindowExW(defview, None, "SysListView32", None)
            if lv:
                found.append(lv)
        return True

    cb = ENUM_PROC(_check_top)
    user32.EnumWindows(cb, 0)
    if not found:
        raise RuntimeError("找不到桌面图标列表（explorer 是否正在运行？桌面图标是否被隐藏？）")
    return found[0]


class Explorer(object):
    """跨进程访问 explorer 里的桌面 ListView"""

    def __init__(self):
        self.lv = find_desktop_listview()
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(self.lv, ctypes.byref(pid))
        self.pid = pid.value
        access = (PROCESS_VM_OPERATION | PROCESS_VM_READ |
                  PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION)
        self.hproc = kernel32.OpenProcess(access, False, self.pid)
        if not self.hproc:
            err = ctypes.get_last_error()
            if err == 5:
                raise RuntimeError("打开 explorer 进程被拒绝（权限不足）。"
                                   "请用与桌面相同的权限运行，不要用管理员身份")
            raise OSError(f"OpenProcess 失败, last_error={err}")
        self.remote = kernel32.VirtualAllocEx(self.hproc, None, 4096,
                                              MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
        if not self.remote:
            raise OSError("VirtualAllocEx 失败，无法在 explorer 中分配内存")
        self.text_off = 1024

    # --- 内存读写 ---
    def _read(self, addr, size):
        buf = ctypes.create_string_buffer(size)
        got = ctypes.c_size_t(0)
        if not kernel32.ReadProcessMemory(self.hproc, ctypes.c_void_p(addr), buf,
                                         size, ctypes.byref(got)):
            raise OSError(f"ReadProcessMemory 失败, last_error={ctypes.get_last_error()}")
        return buf.raw

    def _write(self, addr, data):
        buf = ctypes.create_string_buffer(data, len(data))
        put = ctypes.c_size_t(0)
        if not kernel32.WriteProcessMemory(self.hproc, ctypes.c_void_p(addr), buf,
                                          len(data), ctypes.byref(put)):
            raise OSError(f"WriteProcessMemory 失败, last_error={ctypes.get_last_error()}")

    # --- 状态 ---
    def count(self):
        return send_msg(self.lv, LVM_GETITEMCOUNT, 0, 0)

    def auto_arrange(self):
        style = user32.GetWindowLongW(self.lv, GWL_STYLE)
        return bool(style & LVS_AUTOARRANGE)

    def disable_auto_arrange(self):
        style = user32.GetWindowLongW(self.lv, GWL_STYLE)
        if style & LVS_AUTOARRANGE:
            # SetWindowLongW 只改本地副本的样式位，需配合 LVM_ARRANGE 生效；
            # 更可靠的做法是走桌面右键菜单，所以这里把 style 位清掉后再刷新
            user32.SetWindowLongW(self.lv, GWL_STYLE, style & ~LVS_AUTOARRANGE)
            send_msg(self.lv, LVM_ARRANGE, LVA_DEFAULT, 0)
            return True
        return False

    # --- 读 ---
    def item_name(self, index):
        remote_text = self.remote + self.text_off
        item = LVITEMW()
        item.mask = LVIF_TEXT
        item.iItem = 0
        item.iSubItem = 0
        item.pszText = ctypes.c_void_p(remote_text)
        item.cchTextMax = 260
        self._write(self.remote, bytes(item))
        send_msg(self.lv, LVM_GETITEMTEXTW, index, self.remote)
        raw = self._read(remote_text, 520)
        return raw.decode("utf-16-le", "ignore").split("\x00", 1)[0]

    def item_pos(self, index):
        self._write(self.remote, b"\x00" * 8)
        send_msg(self.lv, LVM_GETITEMPOSITION, index, self.remote)
        raw = self._read(self.remote, 8)
        pt = POINT.from_buffer_copy(raw)
        return int(pt.x), int(pt.y)

    def snapshot(self):
        icons = []
        for i in range(self.count()):
            x, y = self.item_pos(i)
            icons.append({"name": self.item_name(i), "x": x, "y": y})
        return icons

    # --- 写 ---
    def set_pos(self, index, x, y):
        # 用 SETITEMPOSITION32 + 远程 POINT：支持负坐标（副屏在主屏左侧）和 > 65535 的坐标
        pt = POINT(x, y)
        self._write(self.remote, bytes(pt))
        send_msg(self.lv, LVM_SETITEMPOSITION32, index, self.remote)

    def close(self):
        if self.remote:
            kernel32.VirtualFreeEx(self.hproc, self.remote, 0, MEM_RELEASE)
            self.remote = None
        if self.hproc:
            kernel32.CloseHandle(self.hproc)
            self.hproc = None


# ---------------------------------------------------------------- 布局存取
def store_dir():
    base = os.environ.get("APPDATA") or os.path.expanduser("~")
    path = os.path.join(base, "DesktopIconPos", "layouts")
    os.makedirs(path, exist_ok=True)
    return path


def safe_name(name):
    bad = '<>:"/\\|?*'
    for ch in bad:
        name = name.replace(ch, "_")
    return name.strip() or "layout"


def layout_path(name):
    return os.path.join(store_dir(), safe_name(name) + ".json")


def list_layouts():
    d = store_dir()
    out = []
    for fn in os.listdir(d):
        if fn.lower().endswith(".json"):
            p = os.path.join(d, fn)
            try:
                with open(p, "r", encoding="utf-8") as f:
                    data = json.load(f)
            except Exception:
                continue
            out.append({
                "name": fn[:-5],
                "created": data.get("created", "?"),
                "count": len(data.get("icons", [])),
                "screen": data.get("screen", {}),
                "path": p,
            })
    out.sort(key=lambda x: x["created"], reverse=True)
    return out


def latest_layout():
    items = list_layouts()
    return items[0]["name"] if items else None


def load_layout(name):
    p = layout_path(name)
    if not os.path.exists(p):
        raise FileNotFoundError(f"布局不存在: {name}")
    with open(p, "r", encoding="utf-8") as f:
        return json.load(f)


def screen_size():
    try:
        return {"w": user32.GetSystemMetrics(0), "h": user32.GetSystemMetrics(1)}
    except Exception:
        return {}


# ---------------------------------------------------------------- 命令
def cmd_save(name=None):
    name = name or time.strftime("auto-%Y%m%d-%H%M%S")
    ex = Explorer()
    try:
        icons = ex.snapshot()
        if not icons:
            print("桌面上没有图标（或图标被隐藏），未保存")
            return 1
        data = {
            "name": name,
            "created": time.strftime("%Y-%m-%d %H:%M:%S"),
            "screen": screen_size(),
            "auto_arrange": ex.auto_arrange(),
            "icons": icons,
        }
        p = layout_path(name)
        with open(p, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        print(f"已保存 {len(icons)} 个图标的位置 -> {p}")
        return 0
    finally:
        ex.close()


def cmd_show(name=None):
    name = name or latest_layout()
    if not name:
        print("还没有任何布局，先执行 save")
        return 1
    data = load_layout(name)
    print(f"[{data.get('name')}]  {data.get('created')}  "
          f"屏幕 {data.get('screen', {}).get('w')}x{data.get('screen', {}).get('h')}")
    for it in data["icons"]:
        print(f"  ({it['x']:>6}, {it['y']:>6})  {it['name']}")
    return 0


def _plan(ex, data):
    """算出要移动的图标：返回 (to_move, missing, extra)"""
    saved = {it["name"]: it for it in data["icons"]}
    current = {}
    for i in range(ex.count()):
        current[ex.item_name(i)] = i
    to_move, missing = [], []
    for nm, it in saved.items():
        if nm in current:
            to_move.append((current[nm], nm, it["x"], it["y"]))
        else:
            missing.append(nm)
    extra = [k for k in current if k not in saved]
    return to_move, missing, extra


def cmd_diff(name=None):
    name = name or latest_layout()
    if not name:
        print("还没有任何布局，先执行 save")
        return 1
    data = load_layout(name)
    ex = Explorer()
    try:
        to_move, missing, extra = _plan(ex, data)
        moved = 0
        for idx, nm, x, y in to_move:
            cx, cy = ex.item_pos(idx)
            if (cx, cy) != (x, y):
                moved += 1
        print(f"布局 [{name}]：{len(data['icons'])} 个图标")
        print(f"  需要移动: {moved}")
        print(f"  已消失  : {len(missing)}" + ("  " + ", ".join(missing[:5]) + ("..." if len(missing) > 5 else "") if missing else ""))
        print(f"  新增未记录: {len(extra)}" + ("  " + ", ".join(extra[:5]) + ("..." if len(extra) > 5 else "") if extra else ""))
        return 0
    finally:
        ex.close()


def cmd_restore(name=None, dry_run=False):
    name = name or latest_layout()
    if not name:
        print("还没有任何布局，先执行 save")
        return 1
    data = load_layout(name)
    ex = Explorer()
    try:
        if ex.auto_arrange():
            if dry_run:
                print("提示: 当前开着「自动排列图标」，恢复前需要关掉（正式恢复会自动处理）")
            else:
                ex.disable_auto_arrange()
                print("已关闭「自动排列图标」")
                time.sleep(0.2)

        to_move, missing, extra = _plan(ex, data)
        if dry_run:
            for idx, nm, x, y in to_move:
                cx, cy = ex.item_pos(idx)
                flag = "移动" if (cx, cy) != (x, y) else "不动"
                print(f"  [{flag}] {nm}: ({cx},{cy}) -> ({x},{y})")
            print(f"合计 {len(to_move)} 个，未找到 {len(missing)} 个")
            return 0

        for idx, nm, x, y in to_move:
            ex.set_pos(idx, x, y)
        print(f"已恢复 {len(to_move)} 个图标到布局 [{name}]")
        if missing:
            print(f"  未找到 {len(missing)} 个（可能已删除或改名）: "
                  + ", ".join(missing[:8]) + ("..." if len(missing) > 8 else ""))
        if extra:
            print(f"  新增 {len(extra)} 个图标未在布局中，保持原位")
        return 0
    finally:
        ex.close()


def cmd_list():
    items = list_layouts()
    if not items:
        print(f"还没有任何布局（目录: {store_dir()}）")
        return 0
    print(f"布局目录: {store_dir()}")
    for it in items:
        sc = it["screen"]
        print(f"  {it['name']:<28} {it['count']:>3} 个图标   {it['created']}   "
              f"{sc.get('w', '?')}x{sc.get('h', '?')}")
    return 0


def cmd_delete(name):
    p = layout_path(name)
    if not os.path.exists(p):
        print(f"布局不存在: {name}")
        return 1
    os.remove(p)
    print(f"已删除: {name}")
    return 0


def cmd_status():
    ex = Explorer()
    try:
        print(f"explorer pid      : {ex.pid}")
        print(f"桌面图标数量      : {ex.count()}")
        print(f"自动排列图标      : {'开启（必须先关闭才能恢复）' if ex.auto_arrange() else '已关闭 ✓'}")
        sc = screen_size()
        print(f"主屏分辨率        : {sc.get('w')}x{sc.get('h')}")
        print(f"布局目录          : {store_dir()}")
        print(f"已存布局数量      : {len(list_layouts())}")
        return 0
    finally:
        ex.close()


def main(argv):
    enable_dpi_awareness()
    if len(argv) < 2:
        print(__doc__)
        return 0
    cmd = argv[1].lower()
    args = argv[2:]
    dry = "--dry-run" in args
    args = [a for a in args if not a.startswith("--")]
    name = args[0] if args else None

    try:
        if cmd == "save":
            return cmd_save(name)
        elif cmd == "restore":
            return cmd_restore(name, dry_run=dry)
        elif cmd == "diff":
            return cmd_diff(name)
        elif cmd == "show":
            return cmd_show(name)
        elif cmd == "list":
            return cmd_list()
        elif cmd == "delete":
            if not name:
                print("用法: delete 名称")
                return 1
            return cmd_delete(name)
        elif cmd == "status":
            return cmd_status()
        else:
            print(__doc__)
            return 1
    except Exception as e:
        print(f"错误: {e}")
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
