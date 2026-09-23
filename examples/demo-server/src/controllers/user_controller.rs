//! 用户控制器 —— 演示 PutMapping / DeleteMapping / PatchMapping、
//! `:id` 参数段（自动改写为 actix 的 `{id}`）、分组与全局前缀取消。

use actix_web::web::{self, Data, Path};
use actix_web::{HttpRequest, Responder};
use hirust_router::{DeleteMapping, GetMapping, PatchMapping, PutMapping};
use serde::Deserialize;

use crate::models::{ApiResponse, UserInfo};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UpdateUser {
    pub username: String,
}

/// 更新用户：PUT /api/v1/user/{id}
#[PutMapping(path = "/user/:id", tag = "UserController.Update", desc = "更新用户")]
async fn update(
    path: Path<(u64,)>,
    payload: web::Json<UpdateUser>,
    _data: Data<AppState>,
) -> impl Responder {
    let id = path.0;
    web::Json(ApiResponse::ok(UserInfo { id, username: payload.username.clone() }))
}

/// 删除用户：DELETE /api/v1/user/{id}
#[DeleteMapping(path = "/user/:id", tag = "UserController.Delete", desc = "删除用户")]
async fn delete(path: Path<(u64,)>, data: Data<AppState>) -> impl Responder {
    let _ = &data.version;
    web::Json(ApiResponse::ok(format!("deleted {}", path.0)))
}

/// 局部更新：PATCH /api/v1/user/{id}
#[PatchMapping(path = "/user/:id", tag = "UserController.Patch", desc = "局部更新用户")]
async fn patch(path: Path<(u64,)>) -> impl Responder {
    web::Json(ApiResponse::ok(format!("patched {}", path.0)))
}

/// 分组路由：GET /api/v1/admin/user/list
/// group = "/admin" 对应 Go：AddGroup("/admin", func(){ Get("/user/list", ...) })
#[GetMapping(path = "/user/list", group = "/admin", tag = "UserController.AdminList", desc = "后台用户列表(分组)")]
async fn admin_list(_data: Data<AppState>) -> impl Responder {
    web::Json(ApiResponse::ok(vec![
        UserInfo { id: 1, username: "admin".to_string() },
        UserInfo { id: 2, username: "dunpju".to_string() },
    ]))
}

/// 取消全局前缀（对应 Go CancelGlobalGroupPrefix）：GET /health
#[GetMapping(
    path = "/health",
    tag = "UserController.Health",
    desc = "健康检查(取消全局前缀)",
    cancel_global_prefix = true,
    cancel_global_api_prefix = true,
    auth = false,
)]
async fn health(req: HttpRequest, data: Data<AppState>) -> impl Responder {
    web::Json(ApiResponse::ok(format!(
        "{} {} is healthy at {}",
        data.app_name,
        data.version,
        req.path()
    )))
}
