//! 路由实体 —— 对应 Go 版 `router/Route.go`。
//!
//! `RouteEntry` 是宏在编译期通过 `inventory::submit!` 提交的静态元数据；
//! `RouteInfo` 是 configure 阶段完成前缀拼接/鉴权继承/冲突检测后生成的
//! 最终路由信息（即路由表的一行）。

use actix_web::Resource;

/// 编译期由 Mapping 宏提交的路由元数据（静态，'static 生命周期）。
///
/// 对应 Go `Route` 结构中的元数据字段；handler 本体不放在这里，
/// 而是封装在类型化的 `attach` 挂载函数中。
pub struct RouteEntry {
    /// HTTP 方法：GET/POST/PUT/DELETE/HEAD/PATCH/OPTIONS（对应 Route.method）
    pub method: &'static str,
    /// 相对路径（对应 Route.relativePath；`:id` 已在宏内改写为 `{id}`）
    pub path: &'static str,
    /// 唯一标识（对应 Route.flag；重复注册 panic）
    pub tag: &'static str,
    /// 接口描述（对应 Route.desc）
    pub desc: &'static str,
    /// 是否鉴权（对应 Route.isAuth）。
    /// None = 未显式声明 → 继承分组/全局配置，最终默认 false；Some(v) = 显式声明
    pub auth: Option<bool>,
    /// 归属分组前缀（对应 addGroup 嵌套前缀，如 "/admin/user"；"" = 根）
    pub group: &'static str,
    /// 中间件名列表（仅用于路由表展示；实际中间件已编译期内联进 handler）
    pub middleware_names: &'static [&'static str],
    /// 跳过全局组前缀（对应 CancelGlobalGroupPrefix）
    pub cancel_global_prefix: bool,
    /// 跳过全局 API 组前缀（对应 CancelGlobalApiGroupPrefix）
    pub cancel_global_api_prefix: bool,
    /// 排序键 (行号, 模块路径)：inventory 跨编译单元顺序不稳定，
    /// 以此保证路由表输出可复现（对应 Go Sort 的注册顺序保序）
    pub order: (u32, &'static str),
    /// 类型化挂载函数：把该路由的 method+handler（含中间件链）挂到 Resource 上
    pub attach: fn(Resource) -> Resource,
}

// inventory 收集入口：宏生成的 submit 均提交此类型
inventory::collect!(RouteEntry);

/// 唯一键 —— 对应 Go `Unique(method, absolutePath)` / `UniMd5`。
pub fn unique(method: &str, absolute_path: &str) -> String {
    format!("{}@{}", method.to_uppercase(), absolute_path)
}

/// configure 完成后的最终路由信息（路由表的一行，对应 Go `Routes.ForEach` 遍历到的 Route）。
#[derive(Clone, Debug)]
pub struct RouteInfo {
    pub method: String,
    /// 组前缀（全局前缀 + 分组前缀；对应 Route.groupPrefix）
    pub group_prefix: String,
    /// 相对路径（对应 Route.relativePath）
    pub relative_path: String,
    /// 绝对路径（对应 Route.absolutePath）
    pub absolute_path: String,
    pub tag: String,
    pub desc: String,
    pub auth: bool,
    pub middleware_names: Vec<String>,
}

impl RouteInfo {
    /// 唯一键（METHOD@绝对路径）
    pub fn unique(&self) -> String {
        unique(&self.method, &self.absolute_path)
    }

    /// 单行文本（打印路由表用）
    pub fn to_row(&self) -> String {
        format!(
            "{:<7} {:<40} {:<56} {:<8} {:<6} {}",
            self.method,
            self.absolute_path,
            self.tag,
            if self.auth { "true" } else { "false" },
            "",
            self.middleware_names.join(",")
        )
    }
}
