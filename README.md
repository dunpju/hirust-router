# hirust-router

Go 版声明式路由库的 Rust 迁移版 —— 基于 actix-web 4 的**声明式路由注册**属性宏。

完整设计文档（含与 Go 版原项目逐模块对应关系表）：[DESIGN.md](DESIGN.md)

## 宏一览

| 宏 | HTTP 方法 | 对应 Go 版 |
|---|---|---|
| `#[GetMapping]` | GET | `router.Get(...)` |
| `#[PostMapping]` | POST | `router.Post(...)` |
| `#[PutMapping]` | PUT | `router.Put(...)` |
| `#[DeleteMapping]` | DELETE | `router.Delete(...)` |
| `#[HeadMapping]` | HEAD | `router.Head(...)` |
| `#[PatchMapping]` / `#[OptionsMapping]` | PATCH / OPTIONS | `router.Patch(...)` / `AddRoute("OPTIONS", ...)` |

参数：`path`（缺省 `/`，`:id` 自动改写 `{id}`）、`tag`（唯一标识，缺省为方法
完整模块路径 `crate::模块::…::函数名`，如 `demo_server::controllers::login::info`，
重复 panic）、`middleware = {a, b}`（花括号集合或裸路径）、
`desc`、`auth`（缺省继承 分组 > 全局 > false）、`group`（分组完整前缀）、
`cancel_global_prefix` / `cancel_global_api_prefix`。

## 快速开始

```rust
use actix_web::{App, HttpServer, web};
use actix_web::web::{Data, ReqData};
use actix_web::{HttpRequest, Responder};
use hirust_router::GetMapping;

#[GetMapping(
    path = "/info",
    tag = "LoginController.Info",
    middleware = { crate::middlewares::auth::auth, crate::middlewares::auth::my_auth_middleware },
    desc = "用户信息",
    auth = false,
)]
async fn info(
    req: HttpRequest,
    msg: Option<ReqData<Option<UserInfo>>>,
    data: Data<AppState>,
) -> impl Responder { /* ... */ }

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState::default()))
            .configure(hirust_router::configure) // 一行完成全部声明式路由注册
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
```

## 外部路由查找

对应 Go 版 `Trie.Search` / `Routes.Search` / `Routes.Route` / `Routes.Exist`，
`configure` 之后即可按具体 URL（参数段自动提取实际值）查询路由节点：

```rust
if let Some(hit) = hirust_router::search("PUT", "/api/v1/user/42") {
    hit.route.absolute_path;   // "/api/v1/user/{id}"（命中的注册模式）
    hit.route.tag;             // "UserController.Update"
    hit.route.auth;            // 鉴权标记
    hit.params;                // [("id", "42")]
}
hirust_router::exist("GET", "/api/v1/user/42");  // false
```

匹配规则：字面量段精确匹配且优先于参数段；`{id}` / `:id` 匹配任意单个 URL 段。

## Workspace 结构

- `crates/hirust-router-macro` — proc-macro：参数解析 + 代码生成
- `crates/hirust-router` — runtime：`configure`/分组/中间件/冲突检测/路由表
- `examples/demo-server` — 完整示例（登录/用户控制器、中间件、鉴权、分组）

## 运行示例

```
cargo run -p demo-server
```

启动时打印路由表（对应 Go 版 `GetRoutes().ForEach`）：

```
METHOD  PATH                    TAG                       AUTH   MIDDLEWARE
POST    /api/v1/login           LoginController.Login     false
GET     /api/v1/info            LoginController.Info      false  crate::middlewares::auth::auth,...
...
```
