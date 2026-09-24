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
    /// 标题（对应 Route.title，元数据；供 API 文档/菜单生成等外部系统消费）
    pub title: &'static str,
    /// 前端菜单路由（对应 Route.frontPath，元数据）
    pub front_path: &'static str,
    /// 数据权限（对应 Route.isDataAuth，元数据；
    /// None = 未声明默认 false —— Go 版的组级数据鉴权继承未迁移）
    pub is_data_auth: Option<bool>,
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
    /// 是否 WebSocket 升级路由（对应 Go 版 IsWs 属性）。
    /// hirust-router 自身的 Mapping 宏恒为 false；hirust-wsock 的 `#[WsMapping]`
    /// 提交的条目为 true（attach 挂载 actix-ws 握手而非普通 handler）。
    pub is_ws: bool,
}

// inventory 收集入口：宏生成的 submit 均提交此类型
inventory::collect!(RouteEntry);

/// 唯一键 —— 对应 Go `Unique(method, absolutePath)` / `UniMd5`。
pub fn unique(method: &str, absolute_path: &str) -> String {
    format!("{}@{}", method.to_uppercase(), absolute_path)
}

/// 路由唯一键的 MD5 —— 对应 Go `UniMd5(method, absolutePath)`。
/// 非安全用途（常用作前端权限系统的权限码）。
pub fn uni_md5(method: &str, absolute_path: &str) -> String {
    crate::md5::md5_hex(unique(method, absolute_path).as_bytes())
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
    /// 标题（对应 Route.title）
    pub title: String,
    /// 前端菜单路由（对应 Route.frontPath）
    pub front_path: String,
    /// 数据权限（对应 Route.isDataAuth）
    pub is_data_auth: bool,
    pub auth: bool,
    /// 是否 WebSocket 升级路由（对应 Go Route.isWs；路由表显示为 `GET(WS)`）
    pub is_ws: bool,
    /// 服务名（对应 Route.serve；单 App 下为标签，取首个 configure 的配置）
    pub service: String,
    pub middleware_names: Vec<String>,
}

impl RouteInfo {
    /// 唯一键（METHOD@绝对路径）
    pub fn unique(&self) -> String {
        unique(&self.method, &self.absolute_path)
    }

    /// 唯一键的 MD5（对应 Go UniMd5）
    pub fn uni_md5(&self) -> String {
        uni_md5(&self.method, &self.absolute_path)
    }

    /// 单行文本（打印路由表用）
    pub fn to_row(&self) -> String {
        // ws 行方法列显示 GET(WS)（恰 7 字符，与 HTTP 方法列宽一致）
        let method = if self.is_ws {
            format!("{}(WS)", self.method)
        } else {
            self.method.clone()
        };
        format!(
            "{:<7} {:<40} {:<56} {:<8} {:<6} {}",
            method,
            self.absolute_path,
            self.tag,
            if self.auth { "true" } else { "false" },
            "",
            self.middleware_names.join(",")
        )
    }
}
