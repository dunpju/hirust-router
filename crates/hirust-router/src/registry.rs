//! 路由收集与注册 —— 对应 Go 版 `Routes.go`（CollectRoute/ForEach）、
//! `Serve.go`（多服务）与 `Unique.go`（唯一性）。
//!
//! `configure_with` 的流程（详见 DESIGN.md §4.2）：
//! 收集（inventory）→ tag 唯一性 → method 白名单 → 分组校验与鉴权继承 →
//! 前缀拼接 → Trie 冲突检测 → 按 path 合并挂载到 `web::Resource` → `cfg.service`。
//!
//! 注意：`HttpServer::new(|| App::new().configure(...))` 的工厂闭包会**每个
//! worker 线程执行一次**，因此"计划构建 + 校验"只做一次（OnceLock 幂等），
//! 注册动作（attach 静态函数指针）可安全重复执行。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use actix_web::web::{self, ServiceConfig};
use actix_web::Resource;

use crate::group;
use crate::route::{RouteEntry, RouteInfo};
use crate::trie::Trie;

/// 支持的 HTTP 方法白名单（对应 Go onlySupportMethods）。
pub const ONLY_SUPPORT_METHODS: &[&str] =
    &["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"];

/// 全局路由配置 —— 对应 Go 的 GlobalGroupPrefix / GlobalApiGroupPrefix /
/// GlobalGroupIsAuth / 默认 Serve。
#[derive(Clone, Debug)]
pub struct RouterConfig {
    /// 全局组前缀（对应 GlobalGroupPrefix；路由可用 cancel_global_prefix = true 跳过）
    pub global_prefix: String,
    /// 全局 API 组前缀（对应 GlobalApiGroupPrefix；cancel_global_api_prefix = true 跳过）
    pub global_api_prefix: String,
    /// 全局默认鉴权（对应 GlobalGroupIsAuth）
    pub global_auth: bool,
    /// 服务名（对应 Route.serve / DefaultServe；单 App 下作为路由表标签）
    pub service: String,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            global_prefix: String::new(),
            global_api_prefix: String::new(),
            global_auth: false,
            service: "http".to_string(),
        }
    }
}

/// configure 完成后待挂载的单条路由（含校验结果）。
struct PlannedRoute {
    method: String,
    full_path: String,
    group_prefix: String,
    relative_path: String,
    tag: String,
    desc: String,
    title: String,
    front_path: String,
    is_data_auth: bool,
    auth: bool,
    /// 是否 WebSocket 升级路由（透传 RouteEntry.is_ws）
    is_ws: bool,
    service: String,
    middleware_names: Vec<String>,
    attach: fn(Resource) -> Resource,
}

static PLAN: OnceLock<Vec<PlannedRoute>> = OnceLock::new();
static TABLE: OnceLock<Vec<RouteInfo>> = OnceLock::new();
/// 运行期查找索引（对应 Go Trie 的运行期职责）：终端负载 = TABLE 的下标
static SEARCH: OnceLock<Trie<usize>> = OnceLock::new();

/// 构建注册计划：校验 + 前缀拼接 + 冲突检测（每个进程只执行一次）。
fn build_plan(config: &RouterConfig) -> Vec<PlannedRoute> {
    group::set_global_auth(config.global_auth);
    group::freeze_resolved();

    // 稳定排序（inventory 跨编译单元顺序不稳定）：按 (模块路径, 行号)
    let mut entries: Vec<&RouteEntry> = inventory::iter::<RouteEntry>().collect();
    entries.sort_by(|a, b| (a.order.1, a.order.0).cmp(&(b.order.1, b.order.0)));

    let mut tags: Vec<&str> = Vec::new();
    let mut trie: Trie<()> = Trie::new();
    let mut plan = Vec::new();

    for entry in entries {
        // method 白名单（对应 Go CollectRoute 的 onlySupportMethods panic）
        let method = entry.method.to_uppercase();
        if !ONLY_SUPPORT_METHODS.contains(&method.as_str()) {
            panic!(
                "hirust-router: route {} {} error, only support: {}",
                config.service,
                method,
                ONLY_SUPPORT_METHODS.join("/")
            );
        }

        // tag 唯一性（对应 Go flag 语义）
        if tags.contains(&entry.tag) {
            panic!("hirust-router: route tag {} already exist", entry.tag);
        }
        tags.push(entry.tag);

        // 分组校验与解析
        if !entry.group.is_empty() && group::find_group(entry.group).is_none() {
            panic!(
                "hirust-router: route tag {} references unregistered group {} \
                 (register with hirust_router::add_group before startup)",
                entry.tag, entry.group
            );
        }
        let group_prefix = group::chain_prefix(entry.group);

        // 鉴权继承：路由显式 auth > 链上最内层组 auth > 全局 auth > false
        let auth = entry
            .auth
            .unwrap_or_else(|| group::inherited_auth(entry.group));

        // 前缀拼接：globalGroupPrefix + globalApiGroupPrefix + groupPrefix + relativePath
        // （对应 Go addRoute 的 route.groupPrefix 计算，含 Cancel 语义）
        let mut full_path = String::new();
        if !entry.cancel_global_prefix {
            full_path.push_str(&config.global_prefix);
        }
        if !entry.cancel_global_api_prefix {
            full_path.push_str(&config.global_api_prefix);
        }
        full_path.push_str(&group_prefix);
        full_path.push_str(entry.path);
        if full_path.is_empty() {
            full_path.push('/');
        }

        // (method, absolutePath) 冲突检测（对应 Go Trie.insert 的 already exist panic）
        trie.insert(&method, &full_path, ());

        plan.push(PlannedRoute {
            method,
            full_path,
            group_prefix,
            relative_path: entry.path.to_string(),
            tag: entry.tag.to_string(),
            desc: entry.desc.to_string(),
            title: entry.title.to_string(),
            front_path: entry.front_path.to_string(),
            is_data_auth: entry.is_data_auth.unwrap_or(false),
            auth,
            is_ws: entry.is_ws,
            service: config.service.clone(),
            middleware_names: entry
                .middleware_names
                .iter()
                .map(|name| name.to_string())
                .collect(),
            attach: entry.attach,
        });
    }
    plan
}

/// 声明式路由注册入口（默认配置）：
///
/// ```ignore
/// HttpServer::new(move || App::new().configure(hirust_router::configure))
/// ```
pub fn configure(cfg: &mut ServiceConfig) {
    configure_with(cfg, &RouterConfig::default());
}

/// 带全局配置的注册入口（对应 GlobalGroupPrefix/GlobalApiGroupPrefix/GlobalGroupIsAuth）。
pub fn configure_with(cfg: &mut ServiceConfig, config: &RouterConfig) {
    let plan = PLAN.get_or_init(|| build_plan(config));
    let _ = TABLE.get_or_init(|| {
        plan.iter()
            .map(|route| RouteInfo {
                method: route.method.clone(),
                group_prefix: route.group_prefix.clone(),
                relative_path: route.relative_path.clone(),
                absolute_path: route.full_path.clone(),
                tag: route.tag.clone(),
                desc: route.desc.clone(),
                title: route.title.clone(),
                front_path: route.front_path.clone(),
                is_data_auth: route.is_data_auth,
                auth: route.auth,
                is_ws: route.is_ws,
                service: route.service.clone(),
                middleware_names: route.middleware_names.clone(),
            })
            .collect()
    });
    // 运行期查找索引：模式终端 → 路由表下标（幂等，仅首个 worker 构建）
    let _ = SEARCH.get_or_init(|| {
        let mut trie = Trie::new();
        for (index, route) in plan.iter().enumerate() {
            trie.insert(&route.method, &route.full_path, index);
        }
        trie
    });

    // 按 full_path 分组合并：同路径的各方法挂到同一个 web::Resource，
    // 规避 actix "同 path 多 resource" 冲突；各方法自带独立的中间件链。
    let mut by_path: BTreeMap<&str, Vec<&PlannedRoute>> = BTreeMap::new();
    for route in plan {
        by_path
            .entry(route.full_path.as_str())
            .or_default()
            .push(route);
    }
    for (path, routes) in by_path {
        let mut resource: Resource = web::resource(path);
        for route in routes {
            resource = (route.attach)(resource);
        }
        cfg.service(resource);
    }
}

/// 具名服务注册（对应 Go AddServe，如 "https"）；单 App 下作为独立路由集标签。
pub fn configure_named(service: &str, cfg: &mut ServiceConfig, config: &RouterConfig) {
    let mut config = config.clone();
    config.service = service.to_string();
    configure_with(cfg, &config);
}

/// 外部路由查找结果（对应 Go `Trie.Search` 返回的 `*Node`：节点 + Route）。
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// 命中的路由元数据（对应 Node.Route）
    pub route: RouteInfo,
    /// 路径参数实际值，如 `/api/v1/user/{id}` 匹配 `/api/v1/user/42` → `[("id", "42")]`
    pub params: Vec<(String, String)>,
}

/// 外部路由查找（对应 Go `Routes.Search` / `Routes.Route`）：
/// 按注册模式匹配具体 URL，参数段 `{id}` / `:id` 提取实际值。
/// 需在 `configure` 之后调用（configure 时构建查找索引）；未命中返回 None。
///
/// ```ignore
/// if let Some(hit) = hirust_router::search("PUT", "/api/v1/user/42") {
///     // hit.route.absolute_path == "/api/v1/user/{id}"
///     // hit.route.tag / hit.route.auth / hit.route.middleware_names ...
///     // hit.params == [("id", "42")]
/// }
/// ```
pub fn search(method: &str, url: &str) -> Option<SearchResult> {
    let trie = SEARCH.get()?;
    let table = TABLE.get()?;
    let hit = trie.search(method, url)?;
    let route = table.get(*hit.value).cloned()?;
    Some(SearchResult {
        route,
        params: hit.params.clone(),
    })
}

/// 路由是否存在（URL 可含实际参数值；对应 Go `Routes.Exist`）。
pub fn exist(method: &str, url: &str) -> bool {
    search(method, url).is_some()
}

/// 已注册路由表（对应 Go `GetRoutes(name).ForEach`）。
/// 返回按 (模块路径, 行号) 稳定排序的路由信息；configure 尚未执行时为空表。
pub fn route_table() -> Vec<RouteInfo> {
    TABLE.get().cloned().unwrap_or_default()
}

/// 打印路由表（对应 Go test/main.go 的 ForEach 打印）。
pub fn print_route_table() {
    let table = route_table();
    println!(
        "{:<7} {:<40} {:<56} {:<6} MIDDLEWARE",
        "METHOD", "PATH", "TAG", "AUTH"
    );
    println!("{}", "-".repeat(150));
    for info in &table {
        println!("{}", info.to_row());
    }
    println!("{}", "-".repeat(150));
    println!("total: {} routes", table.len());
}
