//! deskpos —— 记住 / 恢复 Windows 桌面图标位置（命令行版）
//!
//! 用法:
//!   deskpos save  [名称]        保存当前桌面布局（默认名 auto-时间戳）
//!   deskpos restore [名称]      恢复布局（留空 = 最近一次）
//!   deskpos list                列出所有已保存布局
//!   deskpos show  [名称]        查看布局内容
//!   deskpos diff  [名称]        预览：恢复后哪些图标会动（不改动桌面）
//!   deskpos delete 名称         删除布局
//!   deskpos status              图标数 / 是否开着自动排列 / 布局目录
//!   deskpos repair              修复「重启后固定错乱同一批图标」：清掉损坏的系统布局缓存
//!
//! 选项:
//!   --dry-run   配合 restore：只演练，不写入

use deskpos::{actions, repair, store};

fn main() {
    deskpos::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args);
    std::process::exit(code);
}

fn usage() -> &'static str {
    "deskpos —— 记住 / 恢复 Windows 桌面图标位置

用法:
  deskpos save  [名称]        保存当前桌面布局（默认名 auto-时间戳）
  deskpos restore [名称]      恢复布局（留空 = 最近一次）
  deskpos list                列出所有已保存布局
  deskpos show  [名称]        查看布局内容
  deskpos diff  [名称]        预览：恢复后哪些图标会动（不改动桌面）
  deskpos delete 名称         删除布局
  deskpos status              图标数 / 是否开着自动排列 / 布局目录
  deskpos repair              修复「重启后固定错乱同一批图标」

选项:
  --dry-run                   配合 restore：只演练，不写入

注意:
  * 桌面右键 -> 查看 -> 必须关闭「自动排列图标」，否则恢复会被立刻重排
    （检测到会自动帮你关掉，并给出提示）
  * 图标按显示名称匹配，重命名文件后该图标的位置会失配"
}

fn run(argv: &[String]) -> i32 {
    if argv.is_empty() {
        println!("{}", usage());
        return 0;
    }
    let cmd = argv[0].to_lowercase();
    let dry = argv.iter().any(|a| a == "--dry-run");
    let name = argv[1..].iter().find(|a| !a.starts_with("--")).cloned();

    let r = match cmd.as_str() {
        "save" => cmd_save(name),
        "restore" => cmd_restore(name, dry),
        "diff" => cmd_diff(name),
        "show" => cmd_show(name),
        "list" | "ls" => cmd_list(),
        "delete" | "rm" => cmd_delete(name),
        "status" => cmd_status(),
        "probe" => cmd_probe(),
        "repair" | "fix" => cmd_repair(),
        "help" | "-h" | "--help" => {
            println!("{}", usage());
            return 0;
        }
        other => {
            eprintln!("未知命令: {}", other);
            println!("{}", usage());
            return 1;
        }
    };

    match r {
        Ok(code) => code,
        Err(e) => {
            println!("错误: {:#}", e);
            2
        }
    }
}

fn cmd_save(name: Option<String>) -> anyhow::Result<i32> {
    let name = name.unwrap_or_else(|| chrono::Local::now().format("auto-%Y%m%d-%H%M%S").to_string());
    let layout = actions::snapshot(&name)?;
    if layout.icons.is_empty() {
        println!("桌面上没有图标（或图标被隐藏），未保存");
        return Ok(1);
    }
    let path = store::save(&layout)?;
    println!("已保存 {} 个图标的位置 -> {}", layout.icons.len(), path.display());
    Ok(0)
}

fn resolve_name(name: Option<String>) -> anyhow::Result<String> {
    if let Some(n) = name {
        return Ok(n);
    }
    match store::latest() {
        Some(n) => Ok(n),
        None => anyhow::bail!("还没有任何布局，先执行 save"),
    }
}

fn cmd_show(name: Option<String>) -> anyhow::Result<i32> {
    let name = resolve_name(name)?;
    let data = store::load(&name)?;
    println!(
        "[{}]  {}  屏幕 {}x{}",
        data.name, data.created, data.screen.w, data.screen.h
    );
    for it in &data.icons {
        println!("  ({:>6}, {:>6})  {}", it.x, it.y, it.name);
    }
    Ok(0)
}

fn cmd_diff(name: Option<String>) -> anyhow::Result<i32> {
    let name = resolve_name(name)?;
    let data = store::load(&name)?;
    let p = actions::plan(&data)?;
    println!("布局 [{}]：{} 个图标", name, p.total);
    println!("  需要移动: {}", p.to_move);
    println!(
        "  已消失  : {}{}",
        p.missing.len(),
        suffix(&p.missing, 5)
    );
    println!(
        "  新增未记录: {}{}",
        p.extra.len(),
        suffix(&p.extra, 5)
    );
    Ok(0)
}

fn cmd_restore(name: Option<String>, dry_run: bool) -> anyhow::Result<i32> {
    let name = resolve_name(name)?;
    let data = store::load(&name)?;

    if dry_run {
        let p = actions::plan(&data)?;
        let cur = actions::current_positions()?;
        let now: std::collections::HashMap<String, (i32, i32)> = cur
            .iter()
            .map(|i| (i.name.to_lowercase(), (i.x, i.y)))
            .collect();
        for it in &data.icons {
            let Some(&(cx, cy)) = now.get(&it.name.to_lowercase()) else {
                continue;
            };
            let flag = if (cx, cy) != (it.x, it.y) { "移动" } else { "不动" };
            println!("  [{}] {}: ({},{}) -> ({},{})", flag, it.name, cx, cy, it.x, it.y);
        }
        println!("合计 {} 个，未找到 {} 个", p.total, p.missing.len());
        if p.to_move > 0 {
            println!("其中 {} 个位置不一致（正式恢复会写入）", p.to_move);
        }
        return Ok(0);
    }

    let r = actions::restore(&data)?;

    println!(
        "已恢复 {} 个图标到布局 [{}]（移动 {} 个，{} 轮写入）",
        r.restored, name, r.moved, r.rounds
    );
    if r.auto_arrange_disabled {
        println!("已关闭「自动排列图标」");
    }
    if !r.failed.is_empty() {
        println!("  {} 个没摆到位:{}", r.failed.len(), suffix(&r.failed, 6));
        println!("  如果数量较多，检查是否开着「自动排列图标」或「将图标与网格对齐」");
    }
    if !r.missing.is_empty() {
        println!(
            "  未找到 {} 个（可能已删除或改名）: {}",
            r.missing.len(),
            suffix(&r.missing, 8)
        );
    }
    if !r.extra.is_empty() {
        println!("  新增 {} 个图标未在布局中，保持原位", r.extra.len());
    }
    println!(
        "  {}",
        if r.used_shell_api {
            "位置已由 shell 写入，重启后保持"
        } else {
            "shell 接口不可用，只改了内存，重启可能被系统覆盖（建议用 GUI 版恢复一次）"
        }
    );
    if !r.failed.is_empty() || !r.used_shell_api {
        println!("  提示: 出现较多图标没摆到位时，先跑一次 repair 清理系统布局缓存");
    }
    Ok(0)
}

fn cmd_list() -> anyhow::Result<i32> {
    let items = store::list();
    if items.is_empty() {
        println!("还没有任何布局（目录: {}）", store::dir().display());
        return Ok(0);
    }
    println!("布局目录: {}", store::dir().display());
    for it in &items {
        println!(
            "  {:<28} {:>3} 个图标   {}   {}x{}",
            it.display_name(),
            it.count(),
            it.stamp,
            it.data.screen.w,
            it.data.screen.h
        );
    }
    Ok(0)
}

fn cmd_delete(name: Option<String>) -> anyhow::Result<i32> {
    let Some(name) = name else {
        println!("用法: deskpos delete 名称");
        return Ok(1);
    };
    store::delete(&name)?;
    println!("已删除: {}", name);
    Ok(0)
}

fn cmd_status() -> anyhow::Result<i32> {
    let d = deskpos::desktop::Desktop::open()?;
    println!("explorer pid      : {}", d.pid());
    println!("桌面图标数量      : {}", d.count()?);
    println!(
        "自动排列图标      : {}",
        if d.auto_arrange() {
            "开启（必须先关闭才能恢复）"
        } else {
            "已关闭 ✓"
        }
    );
    let (w, h) = deskpos::sys::screen_size();
    println!("主屏分辨率        : {}x{}", w, h);
    println!("布局目录          : {}", store::dir().display());
    println!("已存布局数量      : {}", store::list().len());
    println!("系统布局缓存      : {}", repair::describe_cache());
    Ok(0)
}

/// 诊断：shell 官方接口（IFolderView）能不能拿到桌面视图
fn cmd_probe() -> anyhow::Result<i32> {
    match deskpos::shellview::ShellView::try_open() {
        Some(sv) => {
            println!("shell 桌面视图: 可用");
            println!("条目数量      : {}", sv.count());
            for it in sv.items().iter().take(6) {
                println!("  ({:>5}, {:>5})  {}", it.x, it.y, it.name);
            }
            Ok(0)
        }
        None => {
            println!("shell 桌面视图: 拿不到（恢复会退回改内存的方式）");
            Ok(1)
        }
    }
}

fn cmd_repair() -> anyhow::Result<i32> {
    match repair::repair()? {
        None => {
            println!("系统里没有 IconLayouts 缓存，已经是干净状态，不需要修复");
            Ok(0)
        }
        Some(info) => {
            println!(
                "已备份原缓存 -> {}（{} 字节）",
                info.backup.display(),
                info.bytes
            );
            println!("已删除 IconLayouts（系统下次启动会重建一份干净的）");
            println!();
            println!("接着做两步：");
            println!("  1. 重启电脑 —— 桌面会先变成默认排列，这是正常的");
            println!("  2. 重启后执行：deskpos restore");
            println!("  之后再重启就能正常保持了");
            Ok(0)
        }
    }
}

/// `a, b, c...` 这种截断列表
fn suffix(list: &[String], max: usize) -> String {
    if list.is_empty() {
        return String::new();
    }
    let head: Vec<String> = list.iter().take(max).cloned().collect();
    let mut s = format!("  {}", head.join(", "));
    if list.len() > max {
        s.push_str("...");
    }
    s
}
