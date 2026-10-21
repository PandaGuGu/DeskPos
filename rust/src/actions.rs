//! 高层动作：保存 / 差异预览 / 恢复 —— CLI 和 GUI 共用

use crate::desktop::Desktop;
use crate::model::{IconPos, Layout, Plan, RestoreResult, Screen};
use crate::shellview::ShellView;
use crate::sys;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// 抓一份当前桌面布局
pub fn snapshot(name: &str) -> Result<Layout> {
    let d = Desktop::open()?;
    let icons = d.snapshot()?;
    let (w, h) = sys::screen_size();
    Ok(Layout {
        name: name.to_string(),
        created: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        screen: Screen { w, h },
        auto_arrange: d.auto_arrange(),
        icons,
    })
}

/// 当前桌面的所有图标位置
pub fn current_positions() -> Result<Vec<IconPos>> {
    Desktop::open()?.snapshot()
}

/// 只看差异，不改动桌面
pub fn plan(layout: &Layout) -> Result<Plan> {
    let d = Desktop::open()?;
    let cur = d.snapshot()?;

    let saved: HashMap<String, &IconPos> = layout
        .icons
        .iter()
        .filter(|i| !i.name.is_empty())
        .map(|i| (i.name.to_lowercase(), i))
        .collect();
    let now: HashMap<String, &IconPos> = cur.iter().map(|i| (i.name.to_lowercase(), i)).collect();

    let mut out = Plan {
        total: saved.len(),
        ..Default::default()
    };
    for (k, want) in &saved {
        match now.get(k) {
            Some(cur) => {
                if cur.x != want.x || cur.y != want.y {
                    out.to_move += 1;
                }
            }
            None => out.missing.push(want.name.clone()),
        }
    }
    let saved_keys: HashSet<&String> = saved.keys().collect();
    for (k, it) in &now {
        if !saved_keys.contains(k) {
            out.extra.push(it.name.clone());
        }
    }
    Ok(out)
}

/// 当前和布局不一致的图标「现在在哪」（给预览画虚线用）
pub fn ghost(layout: &Layout) -> Result<Vec<IconPos>> {
    let cur = current_positions()?;
    let saved: HashMap<String, &IconPos> = layout
        .icons
        .iter()
        .filter(|i| !i.name.is_empty())
        .map(|i| (i.name.to_lowercase(), i))
        .collect();

    let mut out = Vec::new();
    for it in cur {
        if let Some(want) = saved.get(&it.name.to_lowercase()) {
            if want.x != it.x || want.y != it.y {
                out.push(it);
            }
        }
    }
    Ok(out)
}

/// 恢复布局：优先 shell 官方接口（可持久化），不可用时退回内存写入
pub fn restore(layout: &Layout) -> Result<RestoreResult> {
    let d = Desktop::open()?;

    let auto_off = if d.auto_arrange() {
        d.disable_auto_arrange()
    } else {
        false
    };
    let mut result = RestoreResult {
        auto_arrange_disabled: auto_off,
        ..Default::default()
    };

    // 名字 -> 目标坐标（大小写不敏感）
    let mut want: HashMap<String, (i32, i32)> = HashMap::new();
    for it in &layout.icons {
        if it.name.is_empty() {
            continue;
        }
        want.insert(it.name.to_lowercase(), (it.x, it.y));
    }
    if want.is_empty() {
        return Ok(result);
    }

    // 以当前桌面的图标名为基准算 missing / extra
    let cur = d.snapshot()?;
    let now: HashSet<String> = cur.iter().map(|i| i.name.to_lowercase()).collect();
    for it in &layout.icons {
        if it.name.is_empty() {
            continue;
        }
        if !now.contains(&it.name.to_lowercase()) {
            result.missing.push(it.name.clone());
        }
    }
    let saved_keys: HashSet<String> = want.keys().cloned().collect();
    for it in &cur {
        if !saved_keys.contains(&it.name.to_lowercase()) {
            result.extra.push(it.name.clone());
        }
    }

    // ---------- 首选：shell 官方接口 ----------
    if let Some(mut sv) = ShellView::try_open() {
        if let Ok((moved, failed, rounds)) = sv.apply(&want, 4) {
            result.used_shell_api = true;
            result.moved = moved;
            result.rounds = rounds;
            result.failed = failed;
            result.restored = want
                .len()
                .saturating_sub(result.missing.len() + result.failed.len());
            return Ok(result);
        }
        // 失败就走下面的内存方式
    }

    // ---------- 退路：内存写入 + 多轮校验 ----------
    let mut moved = 0usize;
    let mut rounds = 0usize;
    for round in 0..4 {
        let idx = d.index_map()?;
        let mut wrote = 0usize;
        for it in &layout.icons {
            if it.name.is_empty() {
                continue;
            }
            let Some(&i) = idx.get(&it.name.to_lowercase()) else {
                continue;
            };
            let (cx, cy) = d.item_pos(i)?;
            if cx == it.x && cy == it.y {
                continue;
            }
            d.set_pos(i, it.x, it.y)?;
            wrote += 1;
            if round == 0 {
                moved += 1;
            }
        }
        if wrote == 0 {
            break;
        }
        rounds += 1;
        std::thread::sleep(std::time::Duration::from_millis(140));
    }

    // 核对
    let idx = d.index_map()?;
    let mut ok = 0usize;
    let mut failed = Vec::new();
    for it in &layout.icons {
        if it.name.is_empty() {
            continue;
        }
        let Some(&i) = idx.get(&it.name.to_lowercase()) else {
            continue;
        };
        let (cx, cy) = d.item_pos(i)?;
        if cx == it.x && cy == it.y {
            ok += 1;
        } else {
            failed.push(it.name.clone());
        }
    }

    result.restored = ok;
    result.moved = moved;
    result.failed = failed;
    result.rounds = rounds.max(1);
    Ok(result)
}
