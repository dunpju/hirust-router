//! demo-server —— hirust-router(Rust 版) 声明式路由注册完整示例。
//!
//! 运行：cargo run -p demo-server
//!
//! 以下环节与 Go 版原项目的对应：
//! - `RouterConfig { global_prefix, global_api_prefix, global_auth }`
//!     ←→ `router.GlobalGroupPrefix("/api")` / `GlobalApiGroupPrefix("/v1")` / `GlobalGroupIsAuth`
//! - `hirust_router::add_group(...)` ←→ `router.AddGroup(prefix, fn, IsAuth, GroupMiddle)`
//! - `hirust_router::set_auth_validator(...)` ←→ isAuth 路由的实际鉴权逻辑注入
//! - `hirust_router::configure_with(cfg, &config)` ←→ 收集全部声明式路由并注册到 ServiceConfig
//! - `hirust_router::print_route_table()` ←→ `GetRoutes(DefaultServe).ForEach(...)` 打印路由表

mod controllers;
mod middlewares;
mod models;
mod state;

use actix_web::{App, HttpServer, web};

use crate::middlewares::auth::jwt_check;
use crate::state::AppState;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // 对应 Go：GlobalGroupPrefix("/api") + GlobalApiGroupPrefix("/v1")
    let config = hirust_router::RouterConfig {
        global_prefix: "/api".to_string(),
        global_api_prefix: "/v1".to_string(),
        global_auth: false,
        service: "http".to_string(),
    };

    // 对应 Go：AddGroup("/admin", ..., GroupMiddle(...)) —— 组前缀，未加组中间件
    hirust_router::add_group("/admin", None, &[], Some(false));

    // 对应 Go：auth 路由的实际鉴权逻辑（默认校验 Authorization: Bearer xxx）
    hirust_router::set_auth_validator(jwt_check);

    // 预热一次注册计划（幂等），随后即可打印路由表（对应 Go ForEach 打印）
    let _ = App::new().configure(|cfg| hirust_router::configure_with(cfg, &config));
    hirust_router::print_route_table();

    // 外部路由节点查找（对应 Go Trie.Search / Routes.Route / Routes.Exist）
    match hirust_router::search("PUT", "/api/v1/user/42") {
        Some(hit) => println!(
            "search PUT /api/v1/user/42 -> {} (tag: {}, auth: {}, params: {:?})",
            hit.route.absolute_path, hit.route.tag, hit.route.auth, hit.params
        ),
        None => println!("search PUT /api/v1/user/42 -> not found"),
    }
    println!(
        "exist GET /api/v1/user/42 -> {}",
        hirust_router::exist("GET", "/api/v1/user/42")
    );

    let state = AppState::default();
    println!("listening on http://127.0.0.1:8080");

    HttpServer::new(move || {
        let config = config.clone();
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(move |cfg| hirust_router::configure_with(cfg, &config))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
