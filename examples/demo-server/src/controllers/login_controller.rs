//! 登录控制器 —— 演示 GetMapping / PostMapping / HeadMapping 及
//! 用户示例中的完整签名（HttpRequest + ReqData + web::Data）。

use actix_web::web::{self, Data, ReqData};
use actix_web::{HttpRequest, Responder};
use hirust_router::{GetMapping, HeadMapping, PostMapping};

use crate::models::{ApiResponse, LoginRequest, LoginResponse, UserInfo};
use crate::state::AppState;

/// 登录：POST /api/v1/login
#[PostMapping(path = "/login", tag = "LoginController.Login", desc = "用户登录", auth = false)]
async fn login(
    payload: web::Json<LoginRequest>,
    data: Data<AppState>,
) -> impl Responder {
    if payload.username.is_empty() || payload.password.is_empty() {
        return web::Json(ApiResponse::<LoginResponse>::err("username/password required"));
    }
    let _ = &data.app_name;
    web::Json(ApiResponse::ok(LoginResponse {
        // 演示用；真实项目请签发 JWT
        token: format!("secret-token1:{}", payload.username),
    }))
}

/// 用户信息：GET /api/v1/info
/// 与需求示例完全一致的签名：req + Option<ReqData<Option<UserInfo>>> + Data<AppState>
#[GetMapping(
    path = "/info",
    tag = "LoginController.Info",
    middleware = { crate::middlewares::auth::auth, crate::middlewares::auth::my_auth_middleware },
    desc = "用户信息",
    auth = false
)]
async fn info(
    req: HttpRequest,
    msg: Option<ReqData<Option<UserInfo>>>,
    data: Data<AppState>,
) -> impl Responder {
    let _ = req.path();
    match msg.as_deref() {
        // my_auth_middleware 注入的用户信息（ReqData<Option<UserInfo>>）
        Some(Some(user)) => web::Json(ApiResponse::ok(UserInfo {
            id: user.id,
            username: format!("{}@{}", user.username, data.app_name),
        })),
        // 中间件未注入（token 不带用户号）→ 回退到默认用户
        _ => web::Json(ApiResponse::ok(data.find_user(1).unwrap())),
    }
}

/// 需要默认鉴权（Bearer Token）：GET /api/v1/profile
#[GetMapping(path = "/profile", tag = "LoginController.Profile", desc = "个人中心(需鉴权)", auth = true)]
async fn profile(data: Data<AppState>) -> impl Responder {
    web::Json(ApiResponse::ok(UserInfo {
        id: 1,
        username: format!("{}-profile", data.app_name),
    }))
}

/// 探活：HEAD /api/v1/ping
#[HeadMapping(path = "/ping", tag = "LoginController.Ping", desc = "探活", auth = false)]
async fn ping() -> impl Responder {
    actix_web::HttpResponse::Ok().finish()
}

/// 仪表盘：GET /api/v1/dashboard
/// 未写 tag —— 缺省使用方法完整模块路径：demo_server::controllers::login_controller::dashboard
#[GetMapping(path = "/dashboard", desc = "仪表盘(缺省tag=完整模块路径)")]
async fn dashboard(data: Data<AppState>) -> impl Responder {
    web::Json(ApiResponse::ok(format!("{} v{}", data.app_name, data.version)))
}
