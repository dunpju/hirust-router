//! 分组 —— 对应 Go 版 `router/Collector.go` 的 `addGroup`。
//!
//! Go 版通过保存/恢复 `currentGroupPrefix / currentGroupIsAuth /
//! currentGroupMiddleware` 实现嵌套：前缀字符串拼接、isAuth 沿嵌套覆盖继承、
//! 组中间件沿嵌套叠加。Rust 版把嵌套关系显式化为 `parent` 引用：
//!
//! ```ignore
//! // router.AddGroup("/admin", func(){...}, IsAuth(true), GroupMiddle(m0))
//! hirust_router::add_group("/admin", None, &[boxed_middleware!(m0)], Some(true));
//! // router.AddGroup("/user", func(){...}, GroupMiddle(m1)) 嵌套在 /admin 内
//! hirust_router::add_group("/user", Some("/admin"), &[boxed_middleware!(m1)], None);
//!
//! // 路由归属分组（group = 分组完整前缀）：
//! #[GetMapping(path = "/list", group = "/admin/user", ...)]
//! ```
//!
//! 路由的完整路径 = global_prefix + global_api_prefix + 分组完整前缀 + path；
//! 中间件执行顺序 = 全局 → 链上各组（外层在前）→ 路由级；
//! 鉴权 = 路由显式 auth > 链上最内层显式组 auth > 全局 auth > false。
//!
//! 两阶段存储：注册期 `Mutex<Vec<GroupDef>>`（启动前 add_group 追加），
//! configure 时 `freeze_resolved` 冻结为免锁快照 —— 请求期读取无锁、无 clone。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::middleware::SimpleMiddleware;

/// 分组定义（对应 Go addGroup 的 prefix + GroupMiddle + IsAuth）。
#[derive(Clone)]
pub struct GroupDef {
    /// 分组自身段（如 "/user"）
    pub segment: String,
    /// 分组完整前缀（父完整前缀 + 自身段，如 "/admin/user"）；
    /// 宏的 `group = "..."` 参数与路由表中的 groupPrefix 均使用完整前缀
    pub full: String,
    /// 父分组完整前缀（嵌套；None = 挂在根上）
    pub parent: Option<String>,
    /// 组中间件（对应 GroupMiddle）
    pub middleware: Vec<SimpleMiddleware>,
    /// 组级鉴权（对应 addGroup 的 IsAuth 属性；None = 继承外层）
    pub auth: Option<bool>,
}

/// 冻结后的分组解析结果（请求期免锁读取）。
#[derive(Clone)]
struct ResolvedGroup {
    middleware: Vec<SimpleMiddleware>,
    auth: Option<bool>,
}

static GROUPS: OnceLock<Mutex<Vec<GroupDef>>> = OnceLock::new();
static RESOLVED: OnceLock<HashMap<String, ResolvedGroup>> = OnceLock::new();
static GLOBAL_AUTH: AtomicBool = AtomicBool::new(false);

fn registry() -> &'static Mutex<Vec<GroupDef>> {
    GROUPS.get_or_init(|| Mutex::new(Vec::new()))
}

/// configure 阶段写入全局默认鉴权（对应 Go `GlobalGroupIsAuth`）。
pub(crate) fn set_global_auth(auth: bool) {
    GLOBAL_AUTH.store(auth, Ordering::Relaxed);
}

/// 全局默认鉴权。
pub fn global_auth() -> bool {
    GLOBAL_AUTH.load(Ordering::Relaxed)
}

/// 注册分组（在 HttpServer 启动前调用；对应 Go `AddGroup`）。
/// `segment` 为分组自身前缀段；`parent` 为父分组的**完整前缀**（父分组须先注册）；
/// 重复注册同一完整前缀时 panic。
pub fn add_group(
    segment: &str,
    parent: Option<&str>,
    middleware: &[SimpleMiddleware],
    auth: Option<bool>,
) {
    let parent_full = parent.map(|parent| {
        // 父分组须已注册（对应 Go 嵌套 AddGroup 的外层先于内层执行）
        if find_group(parent).is_none() {
            panic!("hirust-router: parent group {} not registered", parent);
        }
        parent.to_string()
    });
    let mut full = parent_full.clone().unwrap_or_default();
    full.push_str(segment);

    let mut groups = registry().lock().unwrap();
    if groups.iter().any(|group| group.full == full) {
        panic!("hirust-router: group {} already exist", full);
    }
    groups.push(GroupDef {
        segment: segment.to_string(),
        full,
        parent: parent_full,
        middleware: middleware.to_vec(),
        auth,
    });
}

/// 按完整前缀查找分组（返回克隆）。
pub fn find_group(prefix: &str) -> Option<GroupDef> {
    registry()
        .lock()
        .unwrap()
        .iter()
        .find(|group| group.full == prefix)
        .cloned()
}

/// 分组链（从最外层祖先到自身）；在已持有锁的切片上计算（Mutex 不可重入）。
fn chain_of(groups: &[GroupDef], prefix: &str) -> Vec<GroupDef> {
    let mut result: Vec<GroupDef> = Vec::new();
    let mut cursor = Some(prefix.to_string());
    let mut guard = 0usize;
    while let Some(current) = cursor {
        guard += 1;
        if guard > 64 {
            panic!("hirust-router: group chain too deep or cyclic at {}", prefix);
        }
        match groups.iter().find(|group| group.full == current) {
            Some(group) => {
                cursor = group.parent.clone();
                result.push(group.clone());
            }
            None => break,
        }
    }
    result.reverse();
    result
}

/// 分组链（从最外层祖先到自身）。
fn chain(prefix: &str) -> Vec<GroupDef> {
    let groups = registry().lock().unwrap();
    chain_of(&groups, prefix)
}

/// configure 阶段（build_plan）调用：把各分组的解析结果冻结为免锁快照。
/// 此后 add_group 再注册将不会生效（与 Go 版"启动时一次性收集"语义一致）。
pub(crate) fn freeze_resolved() {
    let groups = registry().lock().unwrap();
    let _ = RESOLVED.set(
        groups
            .iter()
            .map(|group| {
                let chain_defs = chain_of(&groups, &group.full);
                let mut middleware = Vec::new();
                for def in &chain_defs {
                    middleware.extend(def.middleware.iter().copied());
                }
                let auth = chain_defs.iter().rev().find_map(|def| def.auth);
                (group.full.clone(), ResolvedGroup { middleware, auth })
            })
            .collect(),
    );
}

fn resolved(prefix: &str) -> Option<&'static ResolvedGroup> {
    RESOLVED.get().and_then(|map| map.get(prefix))
}

/// 分组链中间件（外层在前；优先读冻结快照，未冻结时实时计算）。
pub fn group_middleware(prefix: &str) -> Vec<SimpleMiddleware> {
    if let Some(group) = resolved(prefix) {
        return group.middleware.clone();
    }
    if prefix.is_empty() {
        return Vec::new();
    }
    let mut middleware = Vec::new();
    for def in chain(prefix) {
        middleware.extend(def.middleware);
    }
    middleware
}

/// 继承鉴权：分组链上最内层的显式 auth，否则全局 auth，否则 false。
/// 对应 Go：addGroup 的 IsAuth 覆盖外层，路由 IsAuth 再覆盖组。
pub fn inherited_auth(prefix: &str) -> bool {
    resolved(prefix)
        .and_then(|group| group.auth)
        .or_else(|| {
            if prefix.is_empty() {
                None
            } else {
                chain(prefix).iter().rev().find_map(|def| def.auth)
            }
        })
        .unwrap_or_else(global_auth)
}

/// 分组完整前缀（存在即返回自身 full；嵌套前缀已包含在 full 中，无需重复拼接）。
pub fn chain_prefix(prefix: &str) -> String {
    find_group(prefix)
        .map(|group| group.full)
        .unwrap_or_default()
}
