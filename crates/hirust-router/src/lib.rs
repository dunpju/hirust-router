//! # hirust-router (Rust / actix-web)
//!
//! Go 版声明式路由库的
//! Rust 迁移版：基于 actix-web 4 的声明式路由注册。
//!
//! ## 快速上手
//!
//! ```ignore
//! use actix_web::{web, App, HttpRequest, HttpServer, Responder};
//! use actix_web::dev::ReqData;
//! use hirust_router::{GetMapping, PostMapping};
//!
//! // 声明即注册：标注在 async fn 上
//! #[GetMapping(path = "/info", tag = "LoginController.Info",
//!              middleware = {middlewares::auth::auth, middlewares::auth::my_auth_middleware},
//!              desc = "用户信息", auth = false)]
//! async fn info(req: HttpRequest, msg: Option<ReqData<Option<String>>>,
//!               data: web::Data<AppState>) -> impl Responder {
//!     web::Json(serde_json::json!({ "ok": true }))
//! }
//!
//! #[actix_web::main]
//! async fn main() -> std::io::Result<()> {
//!     HttpServer::new(move || {
//!         App::new()
//!             .app_data(web::Data::new(AppState::default()))
//!             .configure(hirust_router::configure)   // 一行完成全部声明式路由注册
//!     })
//!     .bind(("127.0.0.1", 8080))?
//!     .run()
//!     .await
//! }
//! ```
//!
//! 完整设计（与 Go 版逐模块对应关系）见仓库根目录 `DESIGN.md`。

pub mod group;
pub mod middleware;
pub mod registry;
pub mod route;
pub mod trie;

mod md5;

pub use inventory as __inventory;

// 五个 Mapping 宏（及 Go 版同源的 Patch/Options）
pub use hirust_router_macro::{
    DeleteMapping, GetMapping, HeadMapping, OptionsMapping, PatchMapping, PostMapping, PutMapping,
};

pub use group::{add_group, find_group};
// boxed_middleware 是 #[macro_export] 宏，已位于 crate 根
pub use middleware::{
    default_auth, set_auth_validator, set_global_middleware, AuthValidator, BoxMiddlewareFuture,
    SimpleMiddleware,
};
pub use registry::{
    configure, configure_named, configure_with, exist, print_route_table, route_table, search,
    RouterConfig, SearchResult, ONLY_SUPPORT_METHODS,
};
pub use route::{uni_md5, unique, RouteInfo};
