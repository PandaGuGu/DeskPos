#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
diag.py -- DeskPos「重启后布局丢失」诊断记录器

它不修改桌面、不修改注册表（除了登录后自动续录用的 RunOnce 项），只做记录：

  watch  点「恢复」前后逐帧记录：每个图标的坐标、自动排列开关、explorer 进程、
         以及注册表里 Windows 真正用于持久化的布局块（Win11 是
         Bags\\1\\Desktop\\IconLayouts 二进制）到底有没有被改写。
         录完自动注册「登录时续录」，重启后无需手动操作。

  post   重启登录后自动跑：从桌面刚出现的第一帧开始记录，看布局是「一开始就对、
         后来被改」还是「一登录就是旧布局」，并记录那一刻附近启动的进程。

  report 把三次快照（恢复前 / 恢复后 / 重启后）逐图标对比，生成 report.md：
         谁回退了、整体有没有统一偏移、注册表有没有变、嫌疑进程是哪些。

  restart 不想真重启电脑时用它：重启 explorer 就等于让系统重新读一遍它保存的布局，
         20 秒就能复现问题，并直接出报告。

用法:
  python tools/diag.py watch              # 1. 录制「点恢复」这一下
  （重启电脑，登录后会自动续录 90 秒）
  python tools/diag.py report            # 2. 看结论（post 阶段也会自动生成）

  python tools/diag.py restart           # 快捷路径：不重启电脑，直接复现 + 出报告
  python tools/diag.py watch --seconds 180 --no-runonce
  python tools/diag.py post --session diag/20261021-104500
  python tools/diag.py snapshot --tag manual

产物（默认在项目下 diag/<时间戳>/）:
  session.json              会话信息（系统版本、显示器、DPI、布局文件清单）
  events.jsonl              带时间戳的事件流（位置变化 / 注册表变化 / 进程重启）
  snapshots/*.json          各阶段完整快照
  raw/iconlayouts-*.bin     注册表布局块原始字节（可做二进制对比）
  report.md                 结论报告
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import os
import platform
import sys
import threading
import time
import winreg

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "cli"))
import deskpos  # 复用 deskpos.py 里已经调好的 explorer 内存读写

# ---------------------------------------------------------------- Win32 补充

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
user32 = deskpos.user32
psapi = ctypes.WinDLL("psapi", use_last_error=True)

kernel32.OpenProcess.restype = wt.HANDLE
kernel32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
kernel32.CloseHandle.argtypes = [wt.HANDLE]
kernel32.QueryFullProcessImageNameW.argtypes = [
    wt.HANDLE, wt.DWORD, wt.LPWSTR, ctypes.POINTER(wt.DWORD)]
kernel32.GetProcessTimes.argtypes = [
    wt.HANDLE] + [ctypes.POINTER(ctypes.c_ulonglong)] * 4
kernel32.GetTickCount64.restype = ctypes.c_ulonglong

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
LVM_GETEXTENDEDLISTVIEWSTYLE = 0x1000 + 55
LVS_EX_SNAPTOGRID = 0x00080000

BAG_ROOT = r"Software\Microsoft\Windows\Shell\Bags"
DESK_BAG = r"Software\Microsoft\Windows\Shell\Bags\1\Desktop"
RUNONCE_KEY = r"Software\Microsoft\Windows\CurrentVersion\RunOnce"
RUNONCE_VALUE = "DeskPosDiagPost"
LAYOUT_DIR = os.path.join(os.environ.get("APPDATA", ""), "DesktopIconPos", "layouts")


class RECT(ctypes.Structure):
    _fields_ = [("left", wt.LONG), ("top", wt.LONG),
                ("right", wt.LONG), ("bottom", wt.LONG)]


class MONITORINFO(ctypes.Structure):
    _fields_ = [("cbSize", wt.DWORD), ("rcMonitor", RECT),
                ("rcWork", RECT), ("dwFlags", wt.DWORD)]


MONITORENUMPROC = ctypes.WINFUNCTYPE(
    wt.BOOL, wt.HANDLE, wt.HDC, ctypes.POINTER(RECT), wt.LPARAM)

# ---------------------------------------------------------------- 小工具


def now_iso():
    return time.strftime("%Y-%m-%d %H:%M:%S")


def sha1(data):
    return hashlib.sha1(data).hexdigest()


def uptime():
    return round(kernel32.GetTickCount64() / 1000.0, 1)


def jdump(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f, ensure_ascii=False, indent=2)


def jload(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def monitors():
    out = []
    try:
        shcore = ctypes.WinDLL("shcore")

        def _cb(hmon, hdc, lprc, lparam):
            mi = MONITORINFO()
            mi.cbSize = ctypes.sizeof(MONITORINFO)
            if user32.GetMonitorInfoW(hmon, ctypes.byref(mi)):
                dpi_x, dpi_y = ctypes.c_uint(0), ctypes.c_uint(0)
                try:
                    shcore.GetDpiForMonitor(hmon, 0, ctypes.byref(dpi_x),
                                            ctypes.byref(dpi_y))
                except Exception:
                    pass
                out.append({
                    "rect": [mi.rcMonitor.left, mi.rcMonitor.top,
                             mi.rcMonitor.right, mi.rcMonitor.bottom],
                    "work": [mi.rcWork.left, mi.rcWork.top,
                             mi.rcWork.right, mi.rcWork.bottom],
                    "primary": bool(mi.dwFlags & 1),
                    "dpi": [dpi_x.value, dpi_y.value],
                })
            return True

        user32.EnumDisplayMonitors(None, None, MONITORENUMPROC(_cb), 0)
    except Exception as e:
        out.append({"error": str(e)})
    return out


def proc_start_time(pid):
    h = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not h:
        return None, None
    try:
        buf = ctypes.create_unicode_buffer(1024)
        size = wt.DWORD(1024)
        name = None
        if kernel32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)):
            name = os.path.basename(buf.value)
        c = ctypes.c_ulonglong(0)
        e = ctypes.c_ulonglong(0)
        k = ctypes.c_ulonglong(0)
        u = ctypes.c_ulonglong(0)
        started = None
        if kernel32.GetProcessTimes(h, ctypes.byref(c), ctypes.byref(e),
                                   ctypes.byref(k), ctypes.byref(u)):
            started = round(c.value / 1e7 - 11644473600, 1)
        return name, started
    finally:
        kernel32.CloseHandle(h)


def list_processes():
    arr = (ctypes.c_ulong * 8192)()
    need = ctypes.c_ulong(0)
    if not psapi.EnumProcesses(arr, ctypes.sizeof(arr), ctypes.byref(need)):
        return []
    n = need.value // ctypes.sizeof(ctypes.c_ulong)
    out = []
    seen = set()
    for i in range(n):
        pid = arr[i]
        if not pid or pid in seen:
            continue
        seen.add(pid)
        name, started = proc_start_time(pid)
        if name:
            out.append({"pid": pid, "name": name, "start": started})
    return out


def recent_processes(within=20.0, exclude_system=True):
    """列出最近 within 秒内启动的进程——用来抓「谁在背后重排了桌面」"""
    now = time.time()
    res = []
    for p in list_processes():
        st = p.get("start")
        if not st:
            continue
        age = now - st
        if age <= within:
            res.append({"pid": p["pid"], "name": p["name"],
                        "age": round(age, 1)})
    res.sort(key=lambda x: x["age"])
    return res

# ---------------------------------------------------------------- 注册表布局


def _bag_paths():
    """找出所有存着图标布局的 bag 键（Win10 的 ItemPos* / Win11 的 IconLayouts）"""
    found = []
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, BAG_ROOT) as root:
            n_sub = winreg.QueryInfoKey(root)[0]
            subs = [winreg.EnumKey(root, i) for i in range(n_sub)]
    except OSError:
        return found

    def _walk(path, depth):
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as k:
                n_val = winreg.QueryInfoKey(k)[1]
                names = [winreg.EnumValue(k, i)[0] for i in range(n_val)]
                if any(n.startswith(("ItemPos", "IconLayouts")) or n in
                       ("FFlags", "Mode", "LogicalViewMode") for n in names):
                    found.append(path)
                if depth >= 3:
                    return
                n_sub2 = winreg.QueryInfoKey(k)[0]
                for i in range(n_sub2):
                    _walk(path + "\\" + winreg.EnumKey(k, i), depth + 1)
        except OSError:
            pass

    for s in subs:
        _walk(BAG_ROOT + "\\" + s, 0)
    return found


def _value_digest(name, val, typ):
    if isinstance(val, (bytes, bytearray)):
        return {"type": typ, "len": len(val), "sha1": sha1(bytes(val))[:16]}
    return {"type": typ, "value": val}


def bag_state():
    """轻量状态：每个 bag 键每个值 → 类型/长度/摘要。用于逐秒对比有没有落盘。"""
    state = {}
    for path in _bag_paths():
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as k:
                n_val = winreg.QueryInfoKey(k)[1]
                vals = {}
                for i in range(n_val):
                    name, val, typ = winreg.EnumValue(k, i)
                    vals[name] = _value_digest(name, val, typ)
                state[path] = vals
        except OSError:
            continue
    return state


def dump_bags():
    """完整 dump（含递归子键），存进快照，用于事后 diff"""
    def _walk(path, depth):
        out = {"values": {}, "keys": {}}
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as k:
                n_sub, n_val, _ = winreg.QueryInfoKey(k)
                for i in range(n_val):
                    name, val, typ = winreg.EnumValue(k, i)
                    out["values"][name] = _value_digest(name, val, typ)
                if depth < 4:
                    for i in range(n_sub):
                        sk = winreg.EnumKey(k, i)
                        out["keys"][sk] = _walk(path + "\\" + sk, depth + 1)
        except OSError:
            pass
        return out

    return _walk(BAG_ROOT, 0)


def desktop_bag_blob():
    """取桌面 bag 的二进制值（IconLayouts / ItemPos*），返回 {值名: bytes}"""
    out = {}
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, DESK_BAG) as k:
            for i in range(winreg.QueryInfoKey(k)[1]):
                name, val, typ = winreg.EnumValue(k, i)
                if isinstance(val, (bytes, bytearray)) and \
                        name.startswith(("IconLayouts", "ItemPos")):
                    out[name] = bytes(val)
    except OSError:
        pass
    return out


def layouts_state():
    """布局 JSON 文件清单 + 摘要，防止「布局文件本身被改写」这第三种可能"""
    out = []
    if not os.path.isdir(LAYOUT_DIR):
        return out
    for fn in sorted(os.listdir(LAYOUT_DIR)):
        if not fn.lower().endswith(".json"):
            continue
        p = os.path.join(LAYOUT_DIR, fn)
        try:
            with open(p, "rb") as f:
                raw = f.read()
            data = json.loads(raw.decode("utf-8"))
            out.append({
                "file": fn,
                "mtime": time.strftime("%Y-%m-%d %H:%M:%S",
                                       time.localtime(os.path.getmtime(p))),
                "sha1": sha1(raw)[:16],
                "count": len(data.get("icons", [])),
                "screen": data.get("screen"),
            })
        except Exception as e:
            out.append({"file": fn, "error": str(e)})
    return out

# ---------------------------------------------------------------- 桌面探针


class Probe(object):
    """explorer 的桌面 ListView 句柄可能因为 explorer 重启而失效，这里自动重连"""

    def __init__(self):
        self.ex = None
        self.pid = None
        self.names = {}
        self.count = -1

    def _open(self):
        if self.ex is None:
            self.ex = deskpos.Explorer()
            self.pid = self.ex.pid
            self.names = {}
            self.count = -1

    def close(self):
        if self.ex is not None:
            try:
                self.ex.close()
            except Exception:
                pass
            self.ex = None

    def read(self):
        self._open()
        n = self.ex.count()
        if n != self.count:
            self.names = {}
            self.count = n
        icons = []
        for i in range(n):
            nm = self.names.get(i)
            if nm is None:
                nm = self.ex.item_name(i)
                self.names[i] = nm
            x, y = self.ex.item_pos(i)
            icons.append({"i": i, "name": nm, "x": x, "y": y})
        return icons

    def flags(self):
        """自动排列 / 对齐网格 两个开关"""
        self._open()
        auto = bool(self.ex.auto_arrange())
        snap = None
        try:
            res = ctypes.c_size_t(0)
            user32.SendMessageTimeoutW(
                self.ex.lv, LVM_GETEXTENDEDLISTVIEWSTYLE, 0, 0,
                deskpos.SMTO_ABORTIFHUNG, deskpos.SEND_TIMEOUT,
                ctypes.byref(res))
            snap = bool(res.value & LVS_EX_SNAPTOGRID)
        except Exception:
            pass
        return auto, snap


def pos_dict(icons):
    """name -> (x,y)；重名时后面那个加 #2，避免互相覆盖"""
    out = {}
    for it in icons:
        key = it["name"]
        if key in out:
            k = 2
            while "%s#%d" % (key, k) in out:
                k += 1
            key = "%s#%d" % (key, k)
        out[key] = (it["x"], it["y"])
    return out

# ---------------------------------------------------------------- 快照


def capture(probe, ses, tag):
    snap = {
        "tag": tag,
        "phase": ses.meta.get("phase", "watch"),
        "time": now_iso(),
        "uptime": uptime(),
        "icons": [],
        "auto_arrange": None,
        "snap_to_grid": None,
        "explorer": None,
        "monitors": monitors(),
        "screen": [user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)],
        "bag_state": {},
        "bags": {},
        "layouts": layouts_state(),
    }
    err = None
    try:
        snap["icons"] = probe.read()
        auto, snapg = probe.flags()
        snap["auto_arrange"] = auto
        snap["snap_to_grid"] = snapg
        name, started = proc_start_time(probe.pid)
        snap["explorer"] = {"pid": probe.pid, "name": name, "start": started}
    except Exception as e:
        err = str(e)
        snap["error"] = err

    snap["bag_state"] = bag_state()
    snap["bags"] = dump_bags()

    # 把二进制布局块落盘，方便二进制 diff / 交给别人看
    for vname, blob in desktop_bag_blob().items():
        raw = os.path.join(ses.dir, "raw", "%s-%s-%s.bin"
                           % (tag, vname.replace("(", "_").replace(")", ""),
                              sha1(blob)[:8]))
        os.makedirs(os.path.dirname(raw), exist_ok=True)
        if not os.path.exists(raw):
            with open(raw, "wb") as f:
                f.write(blob)
        snap.setdefault("bag_blobs", {})[vname] = {
            "len": len(blob), "sha1": sha1(blob), "file": os.path.relpath(raw, ses.dir)}

    jdump(os.path.join(ses.dir, "snapshots", tag + ".json"), snap)
    return snap


def icons_of(snap):
    return pos_dict(snap.get("icons") or [])

# ---------------------------------------------------------------- 会话


class Session(object):
    def __init__(self, path):
        self.dir = path
        self.meta_path = os.path.join(path, "session.json")
        self.events_path = os.path.join(path, "events.jsonl")
        self.meta = {}
        self._lock = threading.Lock()
        self.t0 = time.time()

    @staticmethod
    def create(out_dir, keep=None):
        base = out_dir or os.path.join(ROOT, "diag")
        stamp = time.strftime("%Y%m%d-%H%M%S")
        path = os.path.join(base, stamp)
        os.makedirs(path, exist_ok=True)
        ses = Session(path)
        ses.meta = {
            "created": now_iso(),
            "dir": path,
            "os": platform.platform(),
            "build": platform.version(),
            "python": sys.version.split()[0],
            "screen": [user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)],
            "monitors": monitors(),
            "uptime_at_start": uptime(),
            "phase": "watch",
        }
        jdump(ses.meta_path, ses.meta)
        return ses

    @staticmethod
    def load(path=None):
        if not path:
            base = os.path.join(ROOT, "diag")
            if not os.path.isdir(base):
                raise SystemExit("还没录制过。先运行: python tools/diag.py watch")
            dirs = [d for d in os.listdir(base)
                    if os.path.isdir(os.path.join(base, d))]
            if not dirs:
                raise SystemExit("还没录制过。先运行: python tools/diag.py watch")
            dirs.sort()
            path = os.path.join(base, dirs[-1])
        ses = Session(path)
        if os.path.exists(ses.meta_path):
            ses.meta = jload(ses.meta_path)
        ses.t0 = time.time()
        return ses

    def save_meta(self, **kw):
        self.meta.update(kw)
        jdump(self.meta_path, self.meta)

    def event(self, obj, echo=True):
        obj = dict(obj)
        obj.setdefault("time", now_iso())
        obj["t"] = round(time.time() - self.t0, 2)
        line = json.dumps(obj, ensure_ascii=False)
        with self._lock:
            with open(self.events_path, "a", encoding="utf-8") as f:
                f.write(line + "\n")
        if echo:
            print("  " + _fmt_event(obj), flush=True)

    def events(self):
        if not os.path.exists(self.events_path):
            return []
        out = []
        with open(self.events_path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        out.append(json.loads(line))
                    except Exception:
                        pass
        return out

    def snapshots(self):
        d = os.path.join(self.dir, "snapshots")
        out = {}
        if os.path.isdir(d):
            for fn in sorted(os.listdir(d)):
                if fn.endswith(".json"):
                    try:
                        out[fn[:-5]] = jload(os.path.join(d, fn))
                    except Exception:
                        pass
        return out


def _fmt_event(ev):
    t = ev.get("t", 0)
    kind = ev.get("type")
    if kind == "positions":
        ch = ev.get("changed", [])
        nm = ", ".join(c["name"] for c in ch[:3])
        more = "" if len(ch) <= 3 else " 等"
        return "[+%6.2fs] 位置变化 %d 个: %s%s" % (t, len(ch), nm, more)
    if kind == "bag":
        return "[+%6.2fs] 注册表布局变化: %s" % (t, ev.get("detail", ""))
    if kind == "flags":
        return "[+%6.2fs] 开关变化: 自动排列=%s 对齐网格=%s" % (
            t, ev.get("auto_arrange"), ev.get("snap_to_grid"))
    if kind == "explorer":
        return "[+%6.2fs] explorer 变化: pid %s -> %s" % (
            t, ev.get("old"), ev.get("new"))
    if kind == "layouts":
        return "[+%6.2fs] 布局文件变化: %s" % (t, ev.get("detail", ""))
    if kind == "count":
        return "[+%6.2fs] 图标数量: %s -> %s" % (t, ev.get("old"), ev.get("new"))
    if kind == "error":
        return "[+%6.2fs] 读取失败: %s" % (t, ev.get("message"))
    return "[+%6.2fs] %s" % (t, json.dumps(ev, ensure_ascii=False)[:160])

# ---------------------------------------------------------------- 采样循环


def sample_loop(ses, probe, seconds, interval=0.4, reg_interval=1.0,
                phase="watch", quiet=False):
    """逐帧记录。返回最后一份 (icons, states)"""
    prev = {"icons": None, "auto": None, "snap": None, "pid": None,
            "bag": None, "layouts": None}
    last_reg = 0.0
    end = time.time() + seconds
    err_count = 0
    started = [False]

    while time.time() < end:
        time.sleep(interval)
        try:
            icons = probe.read()
        except Exception as e:
            probe.close()
            err_count += 1
            if err_count <= 3:
                ses.event({"type": "error", "message": str(e)[:200],
                           "phase": phase}, echo=not quiet)
            continue
        err_count = 0

        cur = pos_dict(icons)

        if prev["pid"] is not None and prev["pid"] != probe.pid:
            ses.event({"type": "explorer", "old": prev["pid"],
                       "new": probe.pid, "phase": phase}, echo=not quiet)
        prev["pid"] = probe.pid

        if prev["icons"] is not None and cur != prev["icons"]:
            changed = []
            for k, v in cur.items():
                old = prev["icons"].get(k)
                if old != v:
                    changed.append({"name": k, "from": list(old) if old else None,
                                    "to": list(v)})
            for k in prev["icons"]:
                if k not in cur:
                    changed.append({"name": k, "from": list(prev["icons"][k]),
                                    "to": None})
            ev = {"type": "positions", "phase": phase, "changed": changed,
                  "auto_arrange": prev["auto"]}
            if not started[0]:
                started[0] = True
                ev["first_change"] = True
            ses.event(ev, echo=not quiet)
            if not quiet:
                print("        最近 20 秒启动的进程: " + (
                    ", ".join(p["name"] for p in recent_processes(20)[:6]) or "无"),
                    flush=True)
        prev["icons"] = cur

        # 开关状态（便宜，每帧都读）
        try:
            auto, snapg = probe.flags()
            if prev["auto"] is not None and (auto != prev["auto"] or snapg != prev["snap"]):
                ses.event({"type": "flags", "phase": phase,
                           "auto_arrange": auto, "snap_to_grid": snapg},
                          echo=not quiet)
            prev["auto"], prev["snap"] = auto, snapg
        except Exception:
            pass

        # 注册表 / 布局文件（每秒）
        if time.time() - last_reg >= reg_interval:
            last_reg = time.time()
            bs = bag_state()
            if prev["bag"] is not None and bs != prev["bag"]:
                detail = []
                for path, vals in bs.items():
                    old = prev["bag"].get(path, {})
                    for vname, meta in vals.items():
                        o = old.get(vname)
                        if o != meta:
                            olen = o.get("len") if o and "len" in o else "?"
                            detail.append("%s → %s: %s→%s 字节" % (
                                path.split("\\")[-2] + "\\" + path.split("\\")[-1],
                                vname, olen, meta.get("len", "?")))
                ses.event({"type": "bag", "phase": phase,
                           "detail": "; ".join(detail[:6]) or "结构变化",
                           "changed": detail}, echo=not quiet)
            prev["bag"] = bs

            ls = layouts_state()
            if prev["layouts"] is not None and ls != prev["layouts"]:
                ses.event({"type": "layouts", "phase": phase,
                           "detail": "; ".join(x["file"] for x in ls)},
                          echo=not quiet)
            prev["layouts"] = ls

    return prev


def runonce_command(session_dir):
    py = sys.executable
    cand = os.path.join(os.path.dirname(py), "pythonw.exe")
    exe = cand if os.path.exists(cand) else py
    script = os.path.abspath(__file__)
    return '"%s" "%s" post --session "%s"' % (exe, script, session_dir)


def register_runonce(session_dir):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUNONCE_KEY, 0,
                            winreg.KEY_SET_VALUE) as k:
            winreg.SetValueEx(k, RUNONCE_VALUE, 0, winreg.REG_SZ,
                              runonce_command(session_dir))
        return True
    except OSError as e:
        print("  ! 注册登录自动续录失败: %s" % e)
        return False


def unregister_runonce():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUNONCE_KEY, 0,
                            winreg.KEY_SET_VALUE) as k:
            winreg.DeleteValue(k, RUNONCE_VALUE)
        return True
    except OSError:
        return False

# ---------------------------------------------------------------- 命令


def cmd_watch(args):
    ses = Session.create(args.out)
    ses.save_meta(phase="watch")
    print("=" * 68)
    print("DeskPos 布局持久化诊断 · 第 1 阶段（点恢复）")
    print("=" * 68)
    print("录制目录: %s" % ses.dir)
    print()

    probe = Probe()
    before = capture(probe, ses, "00-before")
    print("已记录恢复前状态：%d 个图标，自动排列=%s，分辨率=%dx%d" % (
        len(before["icons"]), before["auto_arrange"],
        before["screen"][0], before["screen"][1]))
    blobs = before.get("bag_blobs") or {}
    for k, v in blobs.items():
        print("  注册表布局块 %s: %d 字节 (sha1 %s)" % (k, v["len"], v["sha1"][:12]))
    print()
    print("现在请这样做（不要关闭这个窗口）：")
    print("  1) 打开 DeskPos，选中要恢复的那份布局")
    print("  2) 点「恢复选中布局」，等提示完成")
    print("  3) 回到这个窗口等录制结束（%d 秒）" % args.seconds)
    print("-" * 68)
    ses.event({"type": "watch-start", "seconds": args.seconds})

    try:
        sample_loop(ses, probe, args.seconds, interval=args.interval,
                    reg_interval=args.reg_interval, phase="watch")
    except KeyboardInterrupt:
        print("\n手动结束录制")
    finally:
        probe.close()

    after = capture(Probe(), ses, "10-after-restore")
    ses.save_meta(phase="watch-done")

    b, a = icons_of(before), icons_of(after)
    moved = [k for k in a if b.get(k) != a[k]]
    print("-" * 68)
    print("录制结束")
    print("  恢复前 %d 个图标 / 恢复后 %d 个图标，其中 %d 个被移动" % (
        len(b), len(a), len(moved)))

    bs0 = before.get("bag_blobs") or {}
    bs1 = after.get("bag_blobs") or {}
    changed_blob = [k for k in bs0 if bs1.get(k, {}).get("sha1") != bs0[k]["sha1"]]
    if not bs0:
        print("  ! 注册表里找不到布局块（既没有 IconLayouts 也没有 ItemPos*）")
    elif changed_blob:
        for k in changed_blob:
            print("  注册表布局块 %s: 已改写 ✓ (%d → %d 字节)" % (
                k, bs0[k]["len"], bs1[k]["len"]))
    else:
        print("  注册表布局块: 点击恢复后【没有任何变化】⚠ —— 这就意味着重启必然回退")

    print()
    if args.no_runonce:
        print("下一步：重启电脑，登录后立刻运行")
        print("  python tools/diag.py post --session \"%s\"" % ses.dir)
    elif register_runonce(ses.dir):
        print("下一步：直接重启电脑。登录后会自动续录 %d 秒并生成报告。" % args.post_seconds)
        print("（若不想自动续录，可运行: python tools/diag.py post）")
    return 0


def cmd_post(args):
    ses = Session.load(args.session)
    ses.save_meta(phase="post")
    os.makedirs(ses.dir, exist_ok=True)
    # 让事件时间线从本次 post 起点重新计
    ses.t0 = time.time()
    ses.event({"type": "post-start", "uptime": uptime()}, echo=False)

    # 等桌面图标列表出现（登录初期 explorer 还没画完）
    probe = Probe()
    deadline = time.time() + 300
    while time.time() < deadline:
        try:
            probe.read()
            break
        except Exception:
            probe.close()
            time.sleep(0.4)
    else:
        print("等不到桌面图标列表，放弃")
        return 2

    first = capture(probe, ses, "20-post-first")
    ses.event({"type": "post-first-seen", "icons": len(first["icons"]),
               "uptime": uptime()}, echo=False)

    sample_loop(ses, probe, args.seconds, interval=args.interval,
                reg_interval=args.reg_interval, phase="post", quiet=True)
    probe.close()

    capture(Probe(), ses, "21-post-final")
    ses.save_meta(phase="post-done")
    unregister_runonce()
    return cmd_report(argparse.Namespace(session=ses.dir, open=False))


def cmd_snapshot(args):
    ses = Session.load(args.session)
    probe = Probe()
    try:
        snap = capture(probe, ses, args.tag)
        print("已记录快照 %s：%d 个图标" % (args.tag, len(snap["icons"])))
        return 0
    finally:
        probe.close()


def _wait_desktop(limit=60):
    t0 = time.time()
    while time.time() - t0 < limit:
        try:
            deskpos.find_desktop_listview()
            return True
        except Exception:
            time.sleep(0.4)
    return False


def cmd_restart(args):
    """不重启电脑也能复现「重启后布局丢失」：重启 explorer = 让它重新读注册表布局

    这会关掉你所有已打开的文件夹窗口（不影响其他程序），explorer 会自动重启。
    """
    ses = Session.create(None) if not args.session else Session.load(args.session)
    ses.save_meta(phase="restart")
    ses.t0 = time.time()

    print("=" * 68)
    print("快速复现：重启 explorer（等价于登录时重新读取系统保存的布局）")
    print("=" * 68)
    print("会话: %s" % ses.dir)
    print("注意：会关闭所有已打开的文件夹窗口，其它程序不受影响。")

    before = capture(Probe(), ses, "30-pre-restart")
    b = icons_of(before)
    print("重启前：%d 个图标，自动排列=%s" % (len(b), before["auto_arrange"]))
    for k, v in (before.get("bag_blobs") or {}).items():
        print("  注册表布局块 %s: %d 字节 sha1 %s" % (k, v["len"], v["sha1"][:12]))

    import subprocess
    ses.event({"type": "explorer-restart", "action": "kill"}, echo=False)
    subprocess.run(["taskkill", "/F", "/IM", "explorer.exe"],
                   capture_output=True, creationflags=0x08000000)
    time.sleep(1.0)
    if not _wait_desktop(12):
        print("  explorer 未自动拉起，手动启动")
        subprocess.Popen(["explorer.exe"])
        if not _wait_desktop(60):
            print("  ! 等不到桌面图标列表，放弃")
            return 2
    time.sleep(args.wait)

    # 桌面出现后逐帧盯 20 秒，看它是不是先对后错
    probe = Probe()
    sample_loop(ses, probe, args.watch, interval=args.interval,
                reg_interval=args.reg_interval, phase="restart", quiet=False)
    probe.close()

    after = capture(Probe(), ses, "31-post-restart")
    a = icons_of(after)
    print("-" * 68)
    print("重启后：%d 个图标" % len(a))
    for k, v in (after.get("bag_blobs") or {}).items():
        print("  注册表布局块 %s: %d 字节 sha1 %s" % (k, v["len"], v["sha1"][:12]))
    diff = [k for k in b if a.get(k) != b[k]]
    print("与重启前不一致的图标: %d" % len(diff))
    for k in diff[:10]:
        print("    %s  %s -> %s" % (k, b[k], a.get(k)))
    ses.save_meta(phase="restart-done")
    return cmd_report(argparse.Namespace(session=ses.dir, open=False))


# ---------------------------------------------------------------- 报告


def median_delta(pairs):
    """pairs: [(cur, want)] → 出现最多的偏移量，用来发现「整体平移」类问题"""
    from collections import Counter
    c = Counter()
    for cur, want in pairs:
        if cur is None or want is None:
            continue
        c[(cur[0] - want[0], cur[1] - want[1])] += 1
    if not c:
        return None, 0
    (d, n) = c.most_common(1)[0]
    return d, n


def pick_layout(snaps):
    """猜用户点的是哪份布局：与「恢复后」快照吻合度最高的那份"""
    cands = []
    if not os.path.isdir(LAYOUT_DIR):
        return None
    after = None
    for k in ("10-after-restore", "30-pre-restart", "21-post-final",
              "20-post-first", "31-post-restart"):
        if k in snaps:
            after = icons_of(snaps[k])
            break
    for fn in os.listdir(LAYOUT_DIR):
        if not fn.lower().endswith(".json"):
            continue
        try:
            with open(os.path.join(LAYOUT_DIR, fn), "r", encoding="utf-8") as f:
                data = json.load(f)
        except Exception:
            continue
        saved = {}
        for it in data.get("icons", []):
            if it.get("name"):
                saved[it["name"]] = (it["x"], it["y"])
        hit = sum(1 for k, v in saved.items() if after and after.get(k) == v)
        cands.append({"file": fn, "name": data.get("name"),
                      "created": data.get("created"), "screen": data.get("screen"),
                      "icons": saved, "hit": hit, "count": len(saved)})
    if not cands:
        return None
    cands.sort(key=lambda x: (-x["hit"], -x["count"]))
    return cands[0]


def cmd_report(args):
    ses = Session.load(args.session)
    snaps = ses.snapshots()
    events = ses.events()
    if not snaps:
        raise SystemExit("这个会话里没有快照，无法生成报告")

    lay = pick_layout(snaps)
    L = lay["icons"] if lay else {}
    B = icons_of(snaps.get("00-before", {})) or icons_of(snaps.get("30-pre-restart", {}))
    A = icons_of(snaps.get("10-after-restore", {}))
    P1 = icons_of(snaps.get("20-post-first", {}))
    P2 = icons_of(snaps.get("21-post-final", {})) or icons_of(snaps.get("31-post-restart", {}))
    post = P2 or P1

    names = []
    seen = set()
    for src in (L, B, A, post):
        if not src:
            continue
        for k in src:
            if k not in seen:
                seen.add(k)
                names.append(k)

    rows = []
    for k in names:
        rows.append({
            "name": k,
            "L": L.get(k), "B": B.get(k), "A": A.get(k),
            "P": post.get(k),
        })

    def judge(r):
        if r["L"] is None:
            return "extra", "布局里没有（新增图标）"
        if r["P"] is None:
            return "gone", "重启后找不到"
        if r["P"] == r["L"]:
            return "ok", "重启后与布局一致"
        if r["A"] is not None and r["P"] == r["A"]:
            return "kept", "保持恢复后的位置（但和布局差一点）"
        if r["B"] is not None and r["P"] == r["B"]:
            return "reverted", "回退到恢复前的位置"
        return "other", "重启后到了第三个位置"

    for r in rows:
        r["verdict"], r["note"] = judge(r)

    order = {"reverted": 0, "other": 1, "gone": 2, "kept": 3, "ok": 4, "extra": 5}
    rows.sort(key=lambda r: (order[r["verdict"]], r["name"]))

    cnt = {}
    for r in rows:
        cnt[r["verdict"]] = cnt.get(r["verdict"], 0) + 1

    # 注册表
    bag_before = (snaps.get("00-before", {}).get("bag_blobs") or
                  snaps.get("30-pre-restart", {}).get("bag_blobs") or {})
    bag_after = (snaps.get("10-after-restore", {}).get("bag_blobs") or {})
    bag_post = (snaps.get("21-post-final", {}).get("bag_blobs") or
                snaps.get("20-post-first", {}).get("bag_blobs") or
                snaps.get("31-post-restart", {}).get("bag_blobs") or {})
    bag_persisted = any(bag_after.get(k, {}).get("sha1") != bag_before.get(k, {}).get("sha1")
                        for k in bag_before) if (bag_before and bag_after) else None

    d_before_after, n_ba = median_delta([(A.get(k), L.get(k)) for k in names])
    d_post_layout, n_pl = median_delta([(post.get(k), L.get(k)) for k in names])
    d_post_before, n_pb = median_delta([(post.get(k), B.get(k)) for k in names])

    # 登录后的事件
    post_events = [e for e in events if e.get("phase") in ("post", "restart")
                   and e.get("type") in ("positions", "flags", "explorer", "count")]

    md = []
    A_ = md.append
    A_("# DeskPos「重启后布局丢失」诊断报告")
    A_("")
    A_("- 会话目录: `%s`" % ses.dir)
    A_("- 开始: %s   报告生成: %s" % (ses.meta.get("created", "?"), now_iso()))
    A_("- 系统: %s (build %s)" % (ses.meta.get("os", "?"), ses.meta.get("build", "?")))
    A_("- 分辨率: %s  显示器: %s" % (ses.meta.get("screen"),
                                    json.dumps(ses.meta.get("monitors"), ensure_ascii=False)))
    if lay:
        A_("- 匹配到的布局: **%s**（%d 个图标，%s，屏幕 %s，吻合 %d 个）" % (
            lay["name"], lay["count"], lay["created"],
            lay["screen"], lay["hit"]))
    if "30-pre-restart" in snaps:
        A_("- 本会话是**快速复现**：重启 explorer（等价于登录时重新读取系统保存的布局），"
          "没有真的重启电脑")
    A_("")
    A_("## 一、结论")
    A_("")
    verdicts = []
    if bag_persisted is False:
        verdicts.append(
            "**恢复动作没有写进注册表。** 点击「恢复选中布局」后，"
            "`Bags\\1\\Desktop\\%s` 的字节内容与恢复前完全相同 —— "
            "也就是说 Windows 记住的那份布局根本没变，重启自然回到旧位置。"
            "问题出在恢复路径（`SelectAndPositionItems` 之后没有触发 shell 落盘），"
            "不是坐标算错。" % (list(bag_before.keys())[0] if bag_before else "IconLayouts"))
    elif bag_persisted is True:
        verdicts.append("**恢复动作确实写进了注册表**（布局块字节已变化），"
                        "所以回退发生在「重启之后」，需要看下面的登录后时间线。")
    if bag_before and bag_after and bag_post:
        for k in bag_before:
            if bag_after.get(k, {}).get("sha1") == bag_before[k]["sha1"]:
                continue
            if bag_post.get(k, {}).get("sha1") == bag_before[k]["sha1"]:
                verdicts.append(
                    "**重启过程把注册表布局块改回了「恢复前」的旧值**"
                    "（重启后的 sha1 与恢复前完全相同）。也就是说恢复写进去的内容"
                    "在 shell 启动时被它自己持有的旧布局覆盖掉了 —— 这是"
                    "「恢复后看着对、重启就没了」最直接的证据。")
                break
    if A and L and not any(A.get(k) == L.get(k) for k in names):
        verdicts.append("恢复这一步就没有一个图标对上布局，先看恢复逻辑本身。")
    if cnt.get("reverted"):
        verdicts.append("重启后有 **%d 个图标回退到恢复前的位置**。" % cnt["reverted"])
    if L and post and (cnt.get("other", 0) + cnt.get("reverted", 0)) >= max(3, len(names) // 4):
        verdicts.append("重启后只有 **%d/%d** 个图标还在布局里的位置，其余 %d 个被重排 —— "
                        "典型是 shell 启动时按「顺序」套用坐标，顺序一变就整体错位。"
                        % (cnt.get("ok", 0), len(names),
                           cnt.get("other", 0) + cnt.get("reverted", 0)))
    if not A and post and B:
        dev = [k for k in names if post.get(k) != B.get(k)]
        if dev:
            verdicts.append("重启后 **%d/%d** 个图标与重启前的桌面不一致 —— "
                            "系统恢复出来的不是当时运行中的那份布局。" % (len(dev), len(names)))
    if d_post_before and n_pb >= max(3, len(names) // 3) and d_post_before != (0, 0):
        verdicts.append("重启后整体相对「恢复前」统一偏移 `%s`（%d 个图标），"
                        "说明登录时使用的坐标基准不同。" % (d_post_before, n_pb))
    if d_before_after and d_before_after != (0, 0) and \
            n_ba >= max(3, len(names) // 3):
        verdicts.append("恢复后相对布局整体偏移 `%s`（%d 个图标）—— "
                        "恢复这一步就有系统性错位，多半是 DPI/坐标基准问题。"
                        % (d_before_after, n_ba))
    if post_events:
        first = post_events[0]
        if first.get("type") == "positions":
            verdicts.append("登录后 %.1f 秒桌面位置被改动过一次（%d 个图标），"
                            "说明「布局其实加载对了，之后被某个进程重排」。"
                            % (first.get("t", 0), len(first.get("changed", []))))
    if not verdicts:
        verdicts.append("数据不足以定论，请看下面的逐图标对比。")
    for i, v in enumerate(verdicts, 1):
        A_("%d. %s" % (i, v))
    A_("")

    A_("## 二、关键数字")
    A_("")
    A_("| 项目 | 值 |")
    A_("|---|---|")
    A_("| 布局图标数 | %s |" % (lay["count"] if lay else "?"))
    A_("| 恢复前图标数 | %d |" % len(B))
    if A:
        A_("| 恢复后图标数 | %d |" % len(A))
    A_("| 重启后图标数 | %d |" % len(post))
    if A:
        A_("| 恢复后已对齐布局 | %d |" % sum(1 for k in names
                                          if A.get(k) == L.get(k) and L.get(k)))
    A_("| **重启后仍与布局一致** | **%d** |" % cnt.get("ok", 0))
    A_("| 重启后回退到恢复前 | %d |" % cnt.get("reverted", 0))
    A_("| 重启后变成第三个位置 | %d |" % cnt.get("other", 0))
    A_("| 重启后找不到 | %d |" % cnt.get("gone", 0))
    A_("| 恢复后位置被保持（略偏） | %d |" % cnt.get("kept", 0))
    A_("")
    A_("注册表布局块：")
    A_("")
    A_("| 阶段 | 值名 | 长度 | sha1 |")
    A_("|---|---|---|---|")
    for label, blobs in (("恢复前", bag_before), ("恢复后", bag_after), ("重启后", bag_post)):
        for k, v in blobs.items():
            A_("| %s | %s | %d | `%s` |" % (label, k, v["len"], v["sha1"][:16]))
    if bag_before and bag_after:
        for k in bag_before:
            same = bag_before[k]["sha1"] == bag_after.get(k, {}).get("sha1")
            A_("")
            A_("- %s：恢复前后 %s" % (k, "**完全没变** ⚠" if same else "已被改写 ✓"))
    A_("")

    A_("## 三、登录后发生了什么（重启阶段事件）")
    A_("")
    if not post_events:
        A_("（没有记录到重启后的位置变动事件：要么没跑 post 阶段，"
          "要么桌面在采样开始前就已经是最终状态——这本身也说明问题发生在"
          "shell 加载布局的那一瞬间，而不是之后被别的程序改）")
    else:
        A_("| 时刻 | 事件 |")
        A_("|---|---|")
        for e in post_events[:40]:
            A_("| +%.1fs | %s |" % (e.get("t", 0), _fmt_event(e).split("] ", 1)[-1]))
    A_("")

    A_("## 四、逐图标对比")
    A_("")
    A_("列含义：布局 = 保存时的坐标；恢复前 / 恢复后 / 重启后 = 三次实测。"
      "坐标写成 `x,y`，`-` 表示该阶段没有这个图标。")
    A_("")
    A_("| 图标 | 布局 | 恢复前 | 恢复后 | 重启后 | 判定 |")
    A_("|---|---|---|---|---|---|")

    def fmt(p):
        return "-" if p is None else "%d,%d" % (p[0], p[1])

    for r in rows:
        A_("| %s | %s | %s | %s | %s | %s |" % (
            r["name"], fmt(r["L"]), fmt(r["B"]), fmt(r["A"]), fmt(r["P"]),
            r["note"]))
    A_("")

    A_("## 五、时间线（点恢复阶段）")
    A_("")
    ev_watch = [e for e in events if e.get("phase") == "watch"]
    if not ev_watch:
        A_("（无）")
    else:
        A_("```")
        for e in ev_watch[:60]:
            A_(_fmt_event(e))
        if len(ev_watch) > 60:
            A_("... 共 %d 条，见 events.jsonl" % len(ev_watch))
        A_("```")
    A_("")

    A_("## 六、修复建议")
    A_("")
    if bag_persisted is False:
        A_("1. 恢复完成后必须让 shell **自己落盘** 一次。当前 `ShellView.Apply()` 只调了")
        A_("   `IFolderView::SelectAndPositionItems`，这一步只改内存视图，不保证写 bag。")
        A_("   建议补三件事，任选其一先试：")
        A_("   - 通过 `IShellBrowser::QueryActiveShellView` 拿到 `IShellView`，调用")
        A_("     `IShellView::SaveViewState()`（IID `000214E3-0000-0000-C000-000000000046`）；")
        A_("   - 恢复后 `SHChangeNotify(SHCNE_ASSOCCHANGED/SHCNE_UPDATEDIR, ...)` 触发视图刷新；")
        A_("   - 恢复完成后 `PostMessage` 让 explorer 重绘，或在程序退出前重启 explorer。")
        A_("2. Win11 已不再写 `ItemPos*`，改用 `IconLayouts` 二进制块。"
          "如果 `SaveViewState()` 仍无效，就得自己按该格式回写 blob（风险较高，")
        A_("   但可先用本脚本收集到的 `raw/*.bin` 验证格式与坐标编码）。")
    elif bag_persisted is True:
        A_("1. 恢复已写入注册表，问题在登录阶段。优先排查：开机自启里会动桌面的程序")
        A_("   （Lively Wallpaper、壁纸工具、桌面整理工具、多屏切换工具）。")
        A_("2. 看本报告第五节，登录后位置被改动的那一刻启动了哪些进程。")
        A_("3. 若登录后「第一帧就已是旧布局」，那是 Windows 在登录时读了别的 bag"
          "（分辨率/DPI 索引不同的那份），需要检查 `IconLayouts` 是否按 DPI 分块。")
    else:
        A_("先把 watch 和 post 两个阶段都录一遍，再回来生成报告。")
    A_("")
    A_("---")
    A_("数据文件：`events.jsonl`（逐帧事件）、`snapshots/*.json`（各阶段完整快照）、"
      "`raw/*.bin`（注册表布局块原始字节）。")

    out = os.path.join(ses.dir, "report.md")
    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(md))

    print("=" * 68)
    print("报告已生成: %s" % out)
    print("=" * 68)
    for i, v in enumerate(verdicts, 1):
        print("%d. %s" % (i, v.replace("**", "")))
    print("-" * 68)
    print("重启后仍一致 %d / 回退 %d / 第三位置 %d / 消失 %d（共 %d）" % (
        cnt.get("ok", 0), cnt.get("reverted", 0), cnt.get("other", 0),
        cnt.get("gone", 0), len(names)))
    return 0


def main(argv=None):
    deskpos.enable_dpi_awareness()
    ap = argparse.ArgumentParser(
        description="DeskPos 布局持久化诊断记录器", add_help=True)
    sub = ap.add_subparsers(dest="cmd")

    w = sub.add_parser("watch", help="录制「点恢复」这一下")
    w.add_argument("--seconds", type=int, default=120)
    w.add_argument("--post-seconds", type=int, default=90)
    w.add_argument("--interval", type=float, default=0.4)
    w.add_argument("--reg-interval", type=float, default=1.0)
    w.add_argument("--out", default=None)
    w.add_argument("--no-runonce", action="store_true",
                   help="不注册登录自动续录，重启后手动跑 post")

    p = sub.add_parser("post", help="重启登录后记录（RunOnce 会自动调用）")
    p.add_argument("--session", default=None)
    p.add_argument("--seconds", type=int, default=90)
    p.add_argument("--interval", type=float, default=0.4)
    p.add_argument("--reg-interval", type=float, default=1.0)

    s = sub.add_parser("snapshot", help="手动打一个快照")
    s.add_argument("--session", default=None)
    s.add_argument("--tag", default="manual")

    rs = sub.add_parser("restart", help="重启 explorer 快速复现（不用真重启电脑）")
    rs.add_argument("--session", default=None)
    rs.add_argument("--wait", type=float, default=4.0)
    rs.add_argument("--watch", type=int, default=20)
    rs.add_argument("--interval", type=float, default=0.4)
    rs.add_argument("--reg-interval", type=float, default=1.0)

    r = sub.add_parser("report", help="生成报告")
    r.add_argument("--session", default=None)
    r.add_argument("--open", action="store_true")

    args = ap.parse_args(argv)
    if not args.cmd:
        print(__doc__)
        return 0
    try:
        if args.cmd == "watch":
            return cmd_watch(args)
        if args.cmd == "post":
            return cmd_post(args)
        if args.cmd == "snapshot":
            return cmd_snapshot(args)
        if args.cmd == "restart":
            return cmd_restart(args)
        if args.cmd == "report":
            return cmd_report(args)
    except KeyboardInterrupt:
        return 130
    except Exception as e:
        print("错误: %s" % e)
        import traceback
        traceback.print_exc()
        return 2


if __name__ == "__main__":
    sys.exit(main())
