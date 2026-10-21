//! DeskPos · Rust 版核心库
//!
//! 与 C# (WPF) / Python 两端共用同一份布局文件：
//! `%APPDATA%\DesktopIconPos\layouts\*.json`

pub mod actions;
pub mod desktop;
pub mod model;
pub mod repair;
pub mod shellview;
pub mod store;
pub mod sys;

#[cfg(feature = "gui")]
pub mod gui_icon;

pub fn init() {
    sys::set_dpi_aware();
    sys::com_init();
}
