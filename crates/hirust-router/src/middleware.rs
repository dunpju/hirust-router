//! 中间件约定与默认鉴权 —— 对应 Go 版 `router/Handler.go` 与 handlers 函数链。
//!
//! Go 版的 handlers 链是"前置函数序列"：
//! `globalMiddle → groupMiddle → route middleware → handle`。
//!
//! Rust 版把整条链**全部内联进宏生成的 handler 包装闭包**，顺序与 Go 完全一致：
//!
//! ```text
//! run_global_and_group_middleware(group)   // 全局中间件 → 分组中间件（运行期注册表）
//! default_auth (auth = true 时)            // 默认鉴权（可 set_auth_validator 替换）
//! <middleware = {...} 编译期内联链>          // 路由级中间件，按书写顺序
//! <原 async fn 处理函数>
//! ```
//!
//! 简单中间件（simple middleware）约定 —— 与 Go 版前置函数同构：
//!
//! ```ignore
//! pub async fn my_middleware(req: HttpRequest) -> Result<HttpRequest, actix_web::Error> {
//!     // 通过校验：返回 req（可注入 req.extensions_mut()，供后续 ReqData<T> 提取）
//!     // 拦截：返回 Err(ErrorUnauthorized("unauthorized")) 等，链短路
//! }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use actix_web::HttpRequest;
use actix_web::Error;

use crate::group;

/// 装箱 future 简写（运行期全局/分组中间件使用）。
pub type BoxMiddlewareFuture =
    Pin<Box<dyn Future<Output = Result<HttpRequest, Error>> + Send>>;

/// 运行期（全局/分组）简单中间件：`fn(HttpRequest) -> boxed future`。
/// 编译期（路由级）中间件无需装箱 —— 由宏直接内联调用用户 async fn。
pub type SimpleMiddleware = fn(HttpRequest) -> BoxMiddlewareFuture;

/// 把具体 future 装箱（配合 [`crate::boxed_middleware!`] 使用）。
pub fn box_future<F>(fut: F) -> BoxMiddlewareFuture
where
    F: Future<Output = Result<HttpRequest, Error>> + Send + 'static,
{
    Box::pin(fut)
}

/// 把普通 async fn 适配为 [`SimpleMiddleware`]：
///
/// ```ignore
/// hirust_router::add_group("/admin", None,
///     &[hirust_router::boxed_middleware!(auth)], Some(true));
/// ```
#[macro_export]
macro_rules! boxed_middleware {
    ($f:expr) => {
        |req: ::actix_web::HttpRequest| $crate::middleware::box_future($f(req))
    };
}

// ---------------------------------------------------------------------------
// 全局中间件注册表（对应 Go GlobalMiddle，作用于全部路由）
// ---------------------------------------------------------------------------

static GLOBAL_MIDDLEWARE: OnceLock<Vec<SimpleMiddleware>> = OnceLock::new();

/// 设置全局中间件（在 HttpServer 启动前调用一次；对应 Go `GlobalMiddle(...)`）。
pub fn set_global_middleware(middlewares: &[SimpleMiddleware]) {
    let _ = GLOBAL_MIDDLEWARE.set(middlewares.to_vec());
}

pub(crate) fn global_middleware() -> &'static [SimpleMiddleware] {
    GLOBAL_MIDDLEWARE.get().map(|v| v.as_slice()).unwrap_or(&[])
}

/// 宏生成的包装闭包链首调用：依次执行 全局中间件 → 分组中间件（外层组在前）。
/// 对应 Go 版 `globalMiddle → groupMiddle` 段。
pub async fn run_global_and_group_middleware(
    group_prefix: &str,
    req: HttpRequest,
) -> Result<HttpRequest, Error> {
    let mut req = req;
    for middleware in global_middleware() {
        req = middleware(req).await?;
    }
    for middleware in group::group_middleware(group_prefix) {
        req = middleware(req).await?;
    }
    Ok(req)
}

// ---------------------------------------------------------------------------
// 默认鉴权中间件（auth = true 时由宏插入链首）
// ---------------------------------------------------------------------------

/// 鉴权校验器：通过返回 Ok(())，否则返回 Err（Error 需实现 ResponseError，如 401）。
pub type AuthValidator = fn(&HttpRequest) -> Result<(), Error>;

/// 默认校验逻辑：要求请求头 `Authorization` 非空（演示用，请通过
/// [`set_auth_validator`] 替换为真实 JWT/会话校验）。
fn default_token_check(req: &HttpRequest) -> Result<(), Error> {
    let authorized = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .map_or(false, |token| !token.is_empty());
    if authorized {
        Ok(())
    } else {
        Err(actix_web::error::ErrorUnauthorized("unauthorized"))
    }
}

static AUTH_VALIDATOR: OnceLock<AuthValidator> = OnceLock::new();

/// 替换默认鉴权校验器（对应 Go 版：isAuth 标记的实际鉴权逻辑由外部注入）。
pub fn set_auth_validator(validator: AuthValidator) {
    let _ = AUTH_VALIDATOR.set(validator);
}

/// `auth = true` 的路由执行的默认鉴权中间件。
pub async fn default_auth(req: HttpRequest) -> Result<HttpRequest, Error> {
    let validator = AUTH_VALIDATOR.get().copied().unwrap_or(default_token_check);
    validator(&req)?;
    Ok(req)
}
