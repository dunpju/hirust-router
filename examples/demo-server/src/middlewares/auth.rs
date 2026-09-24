//! 中间件示例 —— 与 Go 版原项目的前置函数同构：
//! `async fn(HttpRequest) -> Result<HttpRequest, actix_web::Error>`

use actix_web::{Error, HttpMessage, HttpRequest};

use crate::models::UserInfo;

/// 校验请求头 `token` 是否为合法令牌（拦截则返回 401）
pub async fn auth(req: HttpRequest) -> Result<HttpRequest, Error> {
    let token = req
        .headers()
        .get("token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !token.starts_with("secret-token") {
        return Err(actix_web::error::ErrorUnauthorized("token invalid"));
    }
    Ok(req)
}

/// 鉴权后注入用户信息：handler 中的 `ReqData<Option<UserInfo>>`
/// 提取的正是这里写入 req.extensions 的数据（演示中间件注入 → 提取的完整链路）
pub async fn my_auth_middleware(req: HttpRequest) -> Result<HttpRequest, Error> {
    let user = req
        .headers()
        .get("token")
        .and_then(|value| value.to_str().ok())
        .and_then(|token| token.strip_prefix("secret-token"))
        .and_then(|suffix| suffix.parse::<u64>().ok())
        .map(|id| UserInfo {
            id,
            username: format!("user-{}", id),
        });
    req.extensions_mut().insert(user);
    Ok(req)
}

/// `auth = true` 路由的默认鉴权校验器（经 hirust_router::set_auth_validator 注入）：
/// 校验 `Authorization: Bearer <jwt>` 非空
pub fn jwt_check(req: &HttpRequest) -> Result<(), Error> {
    let ok = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .map_or(false, |value| value.starts_with("Bearer "));
    if ok {
        Ok(())
    } else {
        Err(actix_web::error::ErrorUnauthorized("missing bearer token"))
    }
}
