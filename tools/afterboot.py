"""登录后自动运行一次：记录真实重启后的桌面状态，并尝试恢复。跑完自删 RunOnce。

不是常驻程序：由 RunOnce 触发，执行完退出，注册表项自动删除。
"""
import os
import sys
import json
import time
import winreg
import datetime
import traceback

ROOT = r"C:\Users\Administrator\Desktop\项目集合\DeskPos"
sys.path.insert(0, os.path.join(ROOT, "cli"))

LOG_DIR = os.path.join(ROOT, "diag")
LAY = os.path.join(os.environ["APPDATA"], "DesktopIconPos", "layouts", "当前桌面.json")
RUNONCE = r"Software\Microsoft\Windows\CurrentVersion\RunOnce"
VALUE_NAME = "DeskPosBootDiag"

X0, DX, Y0, DY = 17, 95, 2, 122


def log(name, data):
    os.makedirs(LOG_DIR, exist_ok=True)
    p = os.path.join(LOG_DIR, name)
    with open(p, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=1)
    return p


def drop_runonce():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUNONCE, 0, winreg.KEY_SET_VALUE) as k:
            winreg.DeleteValue(k, VALUE_NAME)
    except OSError:
        pass


def snap(deskpos):
    deskpos.enable_dpi_awareness()
    ex = deskpos.Explorer()
    try:
        order = [ex.item_name(i) for i in range(ex.count())]
        pos = {ex.item_name(i): ex.item_pos(i) for i in range(ex.count())}
        return order, pos
    finally:
        ex.close()


def bag_info():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                            r"Software\Microsoft\Windows\Shell\Bags\1\Desktop") as k:
            b = winreg.QueryValueEx(k, "IconLayouts")[0]
        import hashlib
        return {"len": len(b), "sha1": hashlib.sha1(b).hexdigest()[:16]}
    except OSError as e:
        return {"error": str(e)}


def main():
    # 等桌面完全就绪
    for _ in range(30):
        time.sleep(1)
        try:
            import deskpos
            _, pos = snap(deskpos)
            if pos:
                break
        except Exception:
            continue

    result = {"time": datetime.datetime.now().isoformat(timespec="seconds")}
    try:
        import deskpos
        want = {it["name"]: (it["x"], it["y"]) for it in
                json.load(open(LAY, encoding="utf-8"))["icons"]}

        time.sleep(3)
        order, pos = snap(deskpos)
        result["icons"] = len(pos)
        result["bag_before"] = bag_info()
        bad = [{"name": n, "want": list(want[n]), "cur": list(pos[n])}
               for n in want if pos.get(n) != want[n]]
        result["drift_before"] = len(bad)
        result["bad_before"] = bad[:20]
        result["sample_before"] = {n: list(p) for n, p in list(pos.items())[:8]}

        # 尝试恢复
        if bad:
            t0 = time.time()
            for _ in range(6):
                ex = deskpos.Explorer()
                try:
                    cur, idxm = {}, {}
                    for i in range(ex.count()):
                        n = ex.item_name(i)
                        cur[n] = ex.item_pos(i)
                        idxm[n] = i
                    pending = {n: t for n, t in want.items() if n in cur and cur[n] != t}
                    if not pending:
                        break
                    owner = {}
                    for n, p in cur.items():
                        owner.setdefault(p, n)
                    guard = 0
                    while pending and guard < 800:
                        guard += 1
                        prog = False
                        for n in list(pending):
                            t = pending[n]
                            o = owner.get(t)
                            if o is None or o == n:
                                ex.set_pos(idxm[n], t[0], t[1])
                                owner[cur[n]] = None
                                owner[t] = n
                                cur[n] = t
                                del pending[n]
                                prog = True
                                time.sleep(0.09)
                        if not prog:
                            n = list(pending)[0]
                            occ = set(cur.values())
                            tgt = set(want.values())
                            cell = None
                            for k in range(30):
                                for j in range(26):
                                    p = (X0 + DX * j, Y0 + DY * k)
                                    if p not in occ and p not in tgt:
                                        cell = p
                                        break
                                if cell:
                                    break
                            if not cell:
                                break
                            ex.set_pos(idxm[n], cell[0], cell[1])
                            owner[cur[n]] = None
                            owner[cell] = n
                            cur[n] = cell
                            time.sleep(0.09)
                finally:
                    ex.close()
            result["restore_seconds"] = round(time.time() - t0, 1)
            time.sleep(2)
            _, pos2 = snap(deskpos)
            result["drift_after"] = sum(1 for n in want if pos2.get(n) != want[n])
            result["bag_after"] = bag_info()
    except Exception:
        result["error"] = traceback.format_exc()

    log("boot-%s.json" % datetime.datetime.now().strftime("%Y%m%d-%H%M%S"), result)
    drop_runonce()


if __name__ == "__main__":
    try:
        main()
    finally:
        try:
            drop_runonce()
        except Exception:
            pass
