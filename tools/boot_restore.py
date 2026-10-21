"""登录后自动把桌面图标摆回保存的布局。

设计要点：
- 不常驻：由计划任务在登录时触发，执行完进程即退出
- 不显示窗口：用 pythonw 运行，无控制台
- 幂等：布局本来就对时直接退出，不做任何写入
- 安全：只移动图标，不改注册表、不删文件；失败只写日志
"""
import os
import sys
import json
import time
import traceback

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if getattr(sys, "frozen", False):
    # 打包成 exe 后：exe 在 dist/ 下，项目根是它的上一级
    ROOT = os.path.dirname(os.path.dirname(os.path.abspath(sys.executable)))
sys.path.insert(0, os.path.join(ROOT, "cli"))

LOG = os.path.join(os.environ.get("LOCALAPPDATA", ROOT), "DesktopIconPos", "boot_restore.log")
LAYOUT = os.path.join(os.environ["APPDATA"], "DesktopIconPos", "layouts", "当前桌面.json")

X0, DX, Y0, DY = 17, 95, 2, 122
MAX_WAIT = 90          # 最多等桌面就绪 90 秒
MAX_ROUNDS = 6


def log(msg):
    try:
        os.makedirs(os.path.dirname(LOG), exist_ok=True)
        with open(LOG, "a", encoding="utf-8") as f:
            f.write("%s  %s\n" % (time.strftime("%Y-%m-%d %H:%M:%S"), msg))
    except Exception:
        pass


def free_cell(occupied, targets):
    for k in range(30):
        for j in range(26):
            p = (X0 + DX * j, Y0 + DY * k)
            if p not in occupied and p not in targets:
                return p
    return (X0, Y0 + DY * 40)


def main():
    start = time.time()
    log("===== 启动 =====")

    with open(LAYOUT, encoding="utf-8") as f:
        data = json.load(f)
    want = {it["name"]: (it["x"], it["y"]) for it in data["icons"] if it.get("name")}
    log("布局: %s, %d 个图标" % (data.get("name", "?"), len(want)))

    try:
        import deskpos
    except ImportError:
        sys.path.insert(0, os.path.join(ROOT, "cli"))
        import deskpos
    except Exception:
        log("导入 deskpos 失败: " + traceback.format_exc())
        return 1
    deskpos.enable_dpi_awareness()

    # 等桌面图标加载出来（登录初期 ListView 还没准备好）
    ex = None
    for attempt in range(MAX_WAIT):
        try:
            ex = deskpos.Explorer()
            n = ex.count()
            if n >= max(1, int(len(want) * 0.8)):
                log("桌面就绪: %d 个图标（等待 %.0f 秒）" % (n, time.time() - start))
                break
        except Exception:
            pass
        finally:
            if ex is not None:
                try:
                    ex.close()
                except Exception:
                    pass
                ex = None
        time.sleep(1)
    else:
        log("等待超时，桌面图标未就绪")
        return 2

    # 布局本来就对就不用动
    for round_no in range(MAX_ROUNDS):
        ex = deskpos.Explorer()
        try:
            pos, idx = {}, {}
            for i in range(ex.count()):
                name = ex.item_name(i)
                pos[name] = ex.item_pos(i)
                idx[name] = i

            pending = {n: t for n, t in want.items() if n in pos and pos[n] != t}
            if not pending:
                log("第 %d 轮：已全部到位（共 %d 个），用时 %.1f 秒"
                    % (round_no + 1, len(pos), time.time() - start))
                return 0

            if round_no == 0:
                log("需要调整 %d 个图标" % len(pending))

            owner = {}
            for n, p in pos.items():
                owner.setdefault(p, n)

            guard = 0
            while pending and guard < 900:
                guard += 1
                moved = False
                for n in list(pending):
                    target = pending[n]
                    holder = owner.get(target)
                    if holder is None or holder == n:
                        ex.set_pos(idx[n], target[0], target[1])
                        owner[pos[n]] = None
                        owner[target] = n
                        pos[n] = target
                        del pending[n]
                        moved = True
                        time.sleep(0.08)
                if not moved:
                    # 循环依赖：先把一个挪到临时空位
                    n = list(pending)[0]
                    cell = free_cell(set(pos.values()), set(want.values()))
                    ex.set_pos(idx[n], cell[0], cell[1])
                    owner[pos[n]] = None
                    owner[cell] = n
                    pos[n] = cell
                    time.sleep(0.08)
        finally:
            ex.close()

    ex = deskpos.Explorer()
    try:
        pos = {ex.item_name(i): ex.item_pos(i) for i in range(ex.count())}
    finally:
        ex.close()
    bad = [n for n in want if pos.get(n) != want[n]]
    log("结束：%d 个图标，仍不一致 %d 个，用时 %.1f 秒"
        % (len(pos), len(bad), time.time() - start))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main() or 0)
    except Exception:
        log("异常: " + traceback.format_exc())
        sys.exit(1)
