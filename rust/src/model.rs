//! 布局文件数据模型 —— 字段名与 C# / Python 版完全一致，三端共用同一份 JSON

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Screen {
    #[serde(default)]
    pub w: i32,
    #[serde(default)]
    pub h: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IconPos {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Layout {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub screen: Screen,
    #[serde(default)]
    pub auto_arrange: bool,
    #[serde(default)]
    pub icons: Vec<IconPos>,
}

impl Layout {
    pub fn screen_text(&self) -> String {
        if self.screen.w > 0 && self.screen.h > 0 {
            format!("{}×{}", self.screen.w, self.screen.h)
        } else {
            "分辨率未知".to_string()
        }
    }
}

/// 布局目录里的一个条目
#[derive(Debug, Clone)]
pub struct LayoutEntry {
    pub file_name: String,
    pub path: std::path::PathBuf,
    pub stamp: String,
    pub data: Layout,
}

impl LayoutEntry {
    pub fn display_name(&self) -> &str {
        if self.data.name.is_empty() {
            &self.file_name
        } else {
            &self.data.name
        }
    }

    pub fn count(&self) -> usize {
        self.data.icons.len()
    }
}

/// 一次恢复的结果
#[derive(Debug, Default, Clone)]
pub struct RestoreResult {
    /// 最终与布局一致（按名字能对上）的图标数
    pub restored: usize,
    /// 本次真正移动过的图标数
    pub moved: usize,
    /// 重试轮数
    pub rounds: usize,
    /// 是否用上了 shell 官方接口（能持久化）
    pub used_shell_api: bool,
    /// 恢复前是否帮你关掉了自动排列
    pub auto_arrange_disabled: bool,
    /// 布局里有、桌面上找不到的
    pub missing: Vec<String>,
    /// 桌面上有、布局里没有的（保持原位）
    pub extra: Vec<String>,
    /// 多轮重试后仍然摆不到位的
    pub failed: Vec<String>,
}

/// 恢复前的差异预览
#[derive(Debug, Default, Clone)]
pub struct Plan {
    /// 需要移动的图标数
    pub to_move: usize,
    /// 布局里有、桌面上找不到的
    pub missing: Vec<String>,
    /// 桌面上有、布局里没有的
    pub extra: Vec<String>,
    /// 布局里的图标总数
    pub total: usize,
}
