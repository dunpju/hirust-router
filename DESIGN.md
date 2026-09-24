# hirust-router 设计方案（Go 版路由库 → Rust/actix-web 迁移）

> 将 Go 版声明式路由库的完整能力迁移到 Rust，
> 基于 actix-web 4，提供 `#[GetMapping]`、`#[PostMapping]`、`#[PutMapping]`、
> `#[DeleteMapping]`、`#[HeadMapping]` 五个属性宏，实现"标注即注册"的声明式路由。

---

## 1. Go 版原项目源码逐模块梳理

| Go 文件 | 职责 | 核心机制 |
|---|---|---|
| `Init.go` | 常量与全局单例 | HTTP 方法常量（GET/POST/PUT/DELETE/PATCH/OPTIONS/HEAD）、属性名常量、`serve` 全局单例、`onlySupportMethods` 白名单（`sync.Once` 初始化） |
| `Attribute.go` | 路由属性 | `Flag/FrontPath/IsStatic/Title/Desc/IsAuth/IsDataAuth/CancelGlobalGroupPrefix/CancelGlobalApiGroupPrefix/IsWs/Middleware/GroupMiddle/GlobalMiddle/SetServe/SetHeader` 构造器；`RouteAttributes.Find` 按 Name 查找 |
| `Collector.go` | 路由收集器（核心） | 全局状态：`currentServe/currentGroupPrefix/currentGroupIsAuth/currentGroupMiddleware/globalGroupPrefix/globalApiGroupPrefix`；`addRoute` 组装 Route 并按 **globalMiddle → groupMiddle → middleware → handler** 生成 handlers 链；`addGroup` 嵌套时保存/恢复外层状态（前缀累加、isAuth 可覆盖继承、组中间件叠加）；`flag` 缺省取函数名 |
| `Route.go` | 路由实体 | serve/method/groupPrefix/relativePath/absolutePath/handle/flag/desc/isAuth/isDataAuth/isWs/middleware/groupMiddle/globalMiddle/handlers/header；唯一键 `Unique(method, absolutePath) = method + "@" + absolutePath`（及 UniMd5） |
| `Routes.go` | 路由集（每 Serve 一份） | `CollectRoute` 入口：方法白名单校验（不合法 panic）→ `serve.AddRoute`；`ForEach/Search/Route/Exist`；链式 `AddRoute/AddGroup/Get/Post/...` |
| `Trie.go` + `Node.go` | 前缀树路由存储 | 按 `/` 分段插入，`:param` 段记入 `ParamMap`；`insert` 时 `(method, absolutePath)` 已存在直接 **panic "already exist"**；`Search` 支持参数段匹配 |
| `Serve.go` | 多服务管理 | 一个 `Serve` 管理多个命名 `Routes`（默认 `"http"`，可 `AddServe("https")`），加锁保序（`Sort` 记录插入顺序） |
| `Sort.go` / `Unique.go` / `Type.go` | 辅助结构 | 有序去重集合、保序遍历（保证路由表输出顺序 = 注册顺序） |

**Go 版关键语义**（迁移必须保留）：

1. 声明式注册：调用 `router.Get(path, handler, attrs...)` 即完成收集，无中心清单；
2. 中间件是**函数链**，不是框架 wrap：`handlers = globalMiddle ++ groupMiddle ++ middleware ++ [handle]`；
3. `(method, absolutePath)` 唯一，重复注册 panic；`flag`(tag) 作用户侧唯一标识；
4. 组嵌套：前缀字符串拼接、`isAuth` 默认继承外层（可覆盖）、组中间件沿嵌套叠加；
5. 全局前缀/鉴权：`GlobalGroupPrefix / GlobalApiGroupPrefix / GlobalGroupIsAuth`；
6. `isAuth` 只是元数据标记，鉴权动作由中间件链承担。

---

## 2. Rust 版总体架构

Rust 与 Go 的根本差异：Go 靠包初始化副作用 + 全局可变状态收集路由；Rust proc-macro 只能做**编译期代码变换**，没有跨文件的全局副作用。因此采用 **`inventory` 分布式注册**（基于 ctor/link_section，把每个宏展开点的注册项收集进全局链表），这是 Rust 生态"标注即注册"的标准做法（serde、actix 生态同源思路）。

```
hirust-router/                        # cargo workspace
├── Cargo.toml                        # workspace 根
├── DESIGN.md                         # 本文档
├── crates/
│   ├── hirust-router-macro/            # proc-macro crate（仅编译期）
│   │   └── src/lib.rs                # 五个属性宏：参数解析 + 代码生成
│   └── hirust-router/                  # runtime crate（re-export 宏，供用户依赖）
│       └── src/
│           ├── lib.rs                # configure()/全局配置/路由表导出
│           ├── route.rs              # RouteEntry 元数据  ←→ Go Route.go
│           ├── registry.rs           # 收集/冲突检测/排序  ←→ Go Routes.go/Serve.go/Unique.go
│           ├── group.rs              # GroupBuilder 分组   ←→ Go Collector.addGroup
│           ├── middleware.rs         # 中间件约定 + auth   ←→ Go Handler.go/中间件链
│           └── trie.rs               # (method,path) 冲突检测 ←→ Go Trie.go
└── examples/
    └── demo-server/                  # 示例：登录控制器 + 中间件 + AppState
        └── src/
            ├── main.rs               # HttpServer::new + hirust_router::configure
            ├── state.rs              # AppState
            ├── middlewares/auth.rs   # auth / my_auth_middleware
            └── controllers/login_controller.rs  # 五个宏的使用示例
```

依赖关系：`demo-server → hirust-router → { hirust-router-macro, actix-web, inventory }`。

---

## 3. proc-macro 解析思路（hirust-router-macro）

### 3.1 属性文法

五个宏共用同一套参数解析器，文法：

```ebnf
mapping_args   := arg ( "," arg )* ","?
arg            := "path" "=" string_lit
                | "tag" "=" string_lit
                | "desc" "=" string_lit
                | "auth" "=" bool_lit
                | "middleware" "=" middleware_list
middleware_list:= "{" path_expr ( "," path_expr )* ","? "}"    // 花括号集合（用户指定语法）
                | path_expr                                    // 单个裸路径也接受
path_expr      := Rust 限定路径，如 middlewares::auth::auth
```

要点：

- `middleware = {middlewares::auth::auth, middlewares::auth::my_auth_middleware}`
  中的 `{...}` 不是合法 Rust 表达式，但 proc-macro 层面它是一个
  `Group(Delimiter::Brace)` token，完全可解析：取 Brace group 内 TokenStream，
  按**顶层逗号**切分（忽略 `<>` 泛型参数中的逗号），每段 `syn::parse::<syn::Path>()`
  得到中间件路径；
- 未知参数名 → 编译错误（列出合法参数），拼写错误早失败；
- `path` 缺省默认 `"/"`；`tag` 缺省默认**方法的完整模块路径**：
  `module_path!() + "::" + 函数名`，即 `crate名::模块::…::函数名`
  （如 `demo_server::controllers::login::info`；对应 Go 的
  `runtime.FuncForPC` 函数名兜底）；`auth` 缺省 `false`（对应 Go `isAuth` 默认）；
- 兼容 Go 版原项目路径风格：path 中的 `:id` 参数段自动改写为 actix 的 `{id}`。

### 3.2 处理函数签名约定

```rust
#[GetMapping(path = "/info", tag = "...", middleware = {...}, desc = "...", auth = false)]
async fn info(req: HttpRequest, msg: Option<ReqData<Option<UserInfo>>>, data: web::Data<AppState>) -> impl Responder
```

宏用 `syn` 解析 `ItemFn` 的每个参数（名称 + 类型）；包装闭包本身只声明
`HttpRequest + Payload` 两个参数，其余参数在**中间件链执行完毕后**以
`FromRequest::from_request(&req, &mut payload)` 手动提取再转发给原函数。
参数类型只要是 actix `FromRequest` 即可（HttpRequest / web::Data / web::Json /
Option<ReqData<T>> / web::Path ... 均满足），宏不限制具体类型组合。

### 3.3 代码生成（展开示例）

对上面示例，宏展开为三部分（属性宏可返回多个 item）：

```rust
// ① 原函数原样保留
async fn info(req: HttpRequest, msg: Option<ReqData<Option<UserInfo>>>,
              data: web::Data<AppState>) -> impl Responder { ... }

// ② inventory 分布式注册项：编译期把路由元数据提交到全局链表
//    （经 hirust_router::__inventory 再导出，用户无需直接依赖 inventory）
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "GET",
        path: "/info",                       // 相对路径，全局前缀在 configure 时拼接
        tag: "LoginController.Info",
        desc: "用户信息",
        auth: ::core::option::Option::Some(false),  // 未声明则为 None（继承组/全局）
        group: "",
        middleware_names: &["middlewares::auth::auth", "middlewares::auth::my_auth_middleware"],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),  // 保序键，对应 Go Sort
        attach: __hirust_attach_info,          // 类型化的路由挂载函数
    }
}

// ③ 类型化挂载函数：中间件链 + handler 包装成单个 actix route
fn __hirust_attach_info(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::get().to(
        move |__hirust_req: HttpRequest, __hirust_payload_w: web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            // —— 链首：全局中间件 → 分组中间件（运行期注册表，对应 Go globalMiddle → groupMiddle）——
            let __hirust_req = hirust_router::middleware::run_global_and_group_middleware("", __hirust_req).await?;
            // —— 鉴权（显式声明 → 编译期常量；未声明 → 运行期继承组/全局）——
            let __hirust_req = if false {
                hirust_router::middleware::default_auth(__hirust_req).await?
            } else { __hirust_req };
            // —— 路由级中间件（编译期内联，按书写顺序）——
            let __hirust_req = ::middlewares::auth::auth(__hirust_req).await?;
            let __hirust_req = ::middlewares::auth::my_auth_middleware(__hirust_req).await?;
            // —— 中间件之后再提取 handler 参数（ReqData<T> 才能读到中间件注入的数据）——
            let msg = <Option<ReqData<Option<UserInfo>>> as FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload).await.map_err(Into::into)?;
            let data = <web::Data<AppState> as FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload).await.map_err(Into::into)?;
            // —— handler（对应 Go: handlers 链尾的 handle）——
            let __hirust_res = info(__hirust_req.clone(), msg, data).await;
            Ok(Responder::respond_to(__hirust_res, &__hirust_req).map_into_boxed_body())
        }
    ))
}
```

关键取舍：

1. **中间件在生成的 handler 包装闭包内顺序执行**，而不是
   `resource.wrap(Transform)` —— 与 Go 版语义完全一致（Go 版原项目的中间件
   本来就是"handler 前置函数链"，auth 拦截 = 链上函数返回 Err 提前短路）；
2. **参数在中间件链之后手动 `FromRequest` 提取** —— 只有这样，中间件注入的
   `req.extensions` 数据才能被后续 `ReqData<T>` 参数读到，完整复刻 Go 版
   "中间件改写请求 → handler 消费"的链路；
3. **每条路由都生成包装闭包**（无直通快路径）—— 保证运行期注册的
   全局/分组中间件与鉴权继承在任何路由上都生效；
4. 同一路径不同 HTTP 方法可携带**不同中间件链**。actix 的 `wrap` 是
   resource 级，若 GET/POST 同 path 而中间件不同就必须拆成两个 resource，
   而 actix 对同 path 多 resource 直接 panic；把链放进 handler 内，
   同 path 各方法可安全合并进一个 `web::Resource`（见 §4.2）。

### 3.4 中间件函数约定（hirust_router::middleware）

与 Go 版 handler 链中的前置函数同构，用户侧写普通 async fn 即可：

```rust
/// 通过校验 → 返回（可能注入了 extensions 的）请求，继续链；
/// 拦截      → 返回 Err(actix_web::Error)，链短路，Error 经 ResponseError 转 HTTP 响应
pub async fn my_middleware(req: HttpRequest) -> Result<HttpRequest, actix_web::Error> { ... }
```

- 路由级（`middleware = {...}`）：宏编译期直接内联调用上述 async fn，零装箱开销；
- 全局/分组级（运行期注册表，函数指针存储）：
  `type SimpleMiddleware = fn(HttpRequest) -> Pin<Box<dyn Future<...> + Send>>`，
  用 `hirust_router::boxed_middleware!(async_fn)` 把普通 async fn 适配过去。

---

## 4. 路由收集与注册机制

### 4.1 收集：inventory（对应 Go 全局 serve/Trie 的注册期角色）

- 每个宏展开点 `inventory::submit!` 一条 `RouteEntry`（零手工清单，任意模块/文件均可）；
- 启动时 `inventory::iter::<RouteEntry>` 遍历全部条目 —— 对应 Go `GetRoutes().ForEach`。

### 4.2 注册：`hirust_router::configure(cfg: &mut web::ServiceConfig)`

在 `HttpServer::new(|| App::new().configure(hirust_router::configure))` 中调用，流程：

```
inventory::iter::<RouteEntry>                    // ① 收集全部路由元数据
  → Registry::collect()                          // ② tag 唯一性检查（重复 panic，对应 flag 语义）
  → method 白名单校验（对应 onlySupportMethods panic）
  → path 归一化（`:id` → `{id}`）、拼接 global_prefix + global_api_prefix
  → (method, absolute_path) 冲突检测（trie.insert，重复 panic "already exist"）
  → ③ 按 absolute_path 分组合并：同 path 的各方法挂到同一个 web::Resource
      res = web::resource(full_path)
      for entry in group { res = (entry.attach)(res); }   // 各自的 method + 中间件链
  → cfg.service(res)
  → ④ 分组解析冻结（freeze_resolved：分组链中间件/鉴权快照，请求期免锁读取）
  → ⑤ 路由表缓存（供 route_table()/print_route_table() 输出，对应 Go ForEach 打印）
```

幂等性说明：`HttpServer::new(|| App::new().configure(...))` 的工厂闭包
**每个 worker 线程执行一次**，因此"计划构建 + 全部校验（含 panic）"只做一次
（`OnceLock`），注册动作（attach 静态函数指针）可安全重复执行。
```

排序：inventory 跨编译单元顺序不稳定，与 Go 的注册顺序保序不同；
`RouteEntry.order = line!()` + 模块名做稳定排序键，路由表输出按
`(模块, 行号)` 排序，保证可复现（对应 `Sort` 的保序职责）。

### 4.3 Trie：注册期冲突检测 + 运行期查找（hirust_router::trie）

完整迁移 Go 版 Trie 的双重职责，泛型 `Trie<T>`（节点负载 `T`，
registry 中承载路由表索引，对应 Go `Node.Route` 指向的 `*Route`）：

1. **注册期冲突检测**：按 method 为根、`/` 分段建树，`insert` 时终端
   已存在直接 `panic!("route {}:{} already exist")`；同一位置的参数段共享节点
   —— `/head_test/{id}` 与 `/head_test/{name}` 视为同一路径而冲突
   （与 Go 版 ParamMap 行为一致）；
2. **运行期路由查找** `search(method, url)`（对应 Go `Trie.Search`）：
   按注册模式匹配具体 URL，字面量段精确匹配、**字面量优先于参数段**、
   参数段 `{id}`/`:id` 匹配任意单个段并提取实际值。

对外查找 API（registry 层，对应 Go `Routes.Search/Route/Exist`）：

```rust
// configure 之后即可调用（configure 时构建查找索引）
if let Some(hit) = hirust_router::search("PUT", "/api/v1/user/42") {
    hit.route.absolute_path;   // "/api/v1/user/{id}"（命中的注册模式）
    hit.route.tag;             // "UserController.Update"
    hit.params;                // [("id", "42")]
}
hirust_router::exist("GET", "/api/v1/user/42");  // false（该路径无 GET 路由）
```

actix 内部的请求分发仍由框架自身完成；`search` 供业务侧主动查询路由节点
（权限校验、灰度判断、API 网关元数据等场景）。

---

## 5. 中间件链组装方式

| 层级 | Go 版原项目 | Rust (本方案) | 挂载时机 |
|---|---|---|---|
| 全局中间件 globalMiddle | `GlobalMiddle(...)` 属性 | `hirust_router::set_global_middleware(&[boxed_middleware!(f), ...])` | `configure()` 前/运行期，包装闭包链首统一执行 |
| 组中间件 groupMiddle | `AddGroup(..., GroupMiddle(...))` 嵌套叠加 | `hirust_router::add_group(segment, parent, &[...], auth)`，嵌套链叠加并继承 auth | 运行期注册表，紧跟全局中间件执行 |
| 路由中间件 middleware | `Middleware(...)` 属性 | `#[..Mapping(middleware = {a, b})]` | **宏展开编译期**，内联进 handler 包装闭包 |
| 鉴权 auth | `IsAuth(bool)` 元数据标记 | `#[..Mapping(auth = true)]` → 包装闭包内执行 `default_auth`；同时写入元数据 | 编译期常量分支 + 运行时继承 |

执行顺序（与 Go 完全一致）：

```
globalMiddle → groupMiddle → auth(默认鉴权) → route middleware(按书写顺序) → handler
```

默认鉴权中间件可用 `hirust_router::set_auth_validator(fn(&HttpRequest) -> Result<(), Error>)`
全局替换；`auth` 仅作为"是否执行鉴权函数"的开关与元数据，鉴权逻辑本身在校验器里 —— 与
Go 版 `isAuth` 标记语义一致。auth 未显式声明时按 `路由 > 链上最内层组 > 全局 > false`
继承（对应 Go addGroup 的覆盖继承）。

---

## 6. 分组与多服务

| Go 能力 | Rust 对应 |
|---|---|
| `GlobalGroupPrefix("/api")` / `GlobalApiGroupPrefix("/v1")` | `RouterConfig { global_prefix, global_api_prefix }`，configure 时统一拼接 |
| `GlobalGroupIsAuth(true)` | `RouterConfig.global_auth`，所有未显式声明 `auth` 的路由继承 |
| `AddGroup(prefix, fn, IsAuth, GroupMiddle)` 嵌套 | `hirust_router::add_group(segment, parent, &[中间件], auth)`：`parent` 传父分组完整前缀即嵌套（前缀累加、中间件叠加、isAuth 覆盖继承 —— 对应 addGroup 的保存-恢复语义） |
| `AddServe("https")` 多服务 | `configure_named("https", cfg, &config)`：具名标签（单 App 单端口下多服务退化为路由表标签） |
| `CancelGlobalGroupPrefix/CancelGlobalApiGroupPrefix` | `#[..Mapping(cancel_global_prefix = true)]` / `cancel_global_api_prefix = true`，configure 时跳过对应前缀 |
| 路由表 `ForEach` 打印 | `hirust_router::route_table()` / `print_route_table()`：输出 method/path/tag/desc/auth/middleware 列表 |

宏路由通过可选参数 `group = "/admin/user"`（分组**完整前缀**）声明归属分组，
缺省进根；分组须在启动前 `add_group` 注册（configure 时校验，未注册即 panic）。
组的中间件/auth 在运行期组装，路由级中间件在编译期内联，
两层拼接顺序与 Go 一致。分组采用两阶段存储：注册期 `Mutex<Vec<GroupDef>>`，
configure 时冻结为免锁快照（对应 Go"启动时一次性收集"）。

---

## 7. 功能对应关系总表

| Go 版原项目 | hirust-router (Rust/actix-web) | 说明 |
|---|---|---|
| `router.Get(path, handler, attrs...)` | `#[GetMapping(path = "...", ...)] async fn ...` | Post/Put/Delete/Head 同理五宏（另附 Patch/Options） |
| `router.AddRoute(method, ...)` | 宏内 method 固定；泛型 method 不单独提供（新增宏入口即可） | |
| `router.Flag("x")` | `tag = "x"` | 唯一标识，重复 panic |
| `router.Desc("x")` | `desc = "x"` | 接口描述 |
| `router.Title("x")` | `title = "x"` | 标题（元数据，供 API 文档/菜单生成） |
| `router.FrontPath("/login")` | `front_path = "/login"` | 前端菜单路由（元数据） |
| `router.IsDataAuth(true)` | `is_data_auth = true`（缺省 false） | 数据权限标记（元数据；Go 组级数据鉴权继承未迁移） |
| `router.IsAuth(true/false)` | `auth = true/false` | 鉴权开关 + 元数据 |
| `router.Middleware(f1, f2)` | `middleware = {f1, f2}` | 编译期内联进 handler 链 |
| `router.GroupMiddle(...)` | `add_group(..., middleware, ...)` | 组级中间件 |
| `router.GlobalMiddle(...)` | `set_global_middleware(&[...])` | 全局中间件 |
| `addGroup` 嵌套前缀/继承 | `add_group(segment, parent, ...)` | 保存-恢复语义 → parent 链 |
| `GlobalGroupPrefix/ApiPrefix/IsAuth` | `RouterConfig` | 全局前缀与鉴权 |
| `Trie.insert` 冲突 panic | `trie.rs` 注册期冲突检测 panic | 同款错误信息 |
| `Trie.Search` / `Routes.Search` / `Routes.Route` | `hirust_router::search(method, url)` 返回命中路由 + 参数实际值 | 供外部查找路由节点 |
| `Routes.Exist` | `hirust_router::exist(method, url)` | URL 可含实际参数值 |
| `onlySupportMethods` 校验 | 宏层面 method 固定 + 运行期白名单 | |
| `Route.absolutePath` | configure 时 `global_prefix + global_api_prefix + group + path` | |
| `Unique(method, path)` | `unique(method, path)` / `RouteInfo::unique()` = `"METHOD@path"` | |
| `UniMd5(method, path)` | `uni_md5(method, path)`（RFC 1321 内置实现，含标准测试向量） | 前端权限码常用 |
| `Route.serve` / `Serve` 多服务 | `RouteInfo.service` 标签 + `configure_named(service, cfg)` | 单 App 下退化为标签（见 §8） |
| `Routes.ForEach` 路由表 | `route_table()` / `print_route_table()` | 按 order 排序输出 |
| `:id` 参数段 | 自动改写 `{id}` | 风格兼容 |
| `IsWs` / `Ws()` | 不迁移（actix 有独立 WebSocket 生态） | 明确列为范围外 |
| `IsStatic` / `SetHeader` | 不迁移（actix 静态文件服务/响应头有原生方案） | 列为范围外 |
| `Routes.Get(...)` 等运行期链式手动注册 | 不提供：attach 需编译期类型化，无法运行期构造 | 用户可在自己的 configure 闭包内添加原生 actix 路由 |

## 8. 已知差异与限制

1. **注册时机的顺序**：Go 靠代码执行顺序保序；Rust inventory 跨 crate 顺序不保证，
   用 `line!()` + 模块名排序，路由表输出顺序稳定但不等于声明顺序；
2. **请求分发由 actix 完成**；`hirust_router::search()/exist()` 提供业务侧
   主动查找（对应 Go `Trie.Search`），但不参与请求分发；
3. **中间件链在 handler 内执行**（非 `wrap` Transform），语义对应 Go 版函数链；
   需要 Transform 级能力（如请求日志计时包裹整个 service）时，可直接在
   `App::new().wrap(...)` 使用 actix 原生中间件，两者不冲突；
4. **多 Serve 独立路由集退化**：Go 的 `AddServe("https")` 可持有独立路由集；
   Rust 版单 App 单端口，`configure_named` 仅作为 `RouteInfo.service` 标签，
   所有声明式路由进入同一张路由表；
5. **运行期手动注册不提供**：`Routes.Get(...)` 等链式运行期注册需要编译期
   类型化 attach 函数，无法运行期构造；替代方案是在自己的 configure 闭包内
   直接添加原生 actix 路由（与 `hirust_router::configure` 并存不冲突）；
6. **组级数据鉴权继承未迁移**：`is_data_auth` 仅路由级元数据
   （Go 版 `GlobalGroupIsDataAuth` / `addGroup(IsDataAuth)` 的继承链未迁移）；
7. WebSocket、静态文件路由不迁移（actix 生态有专门方案）。

## 9. 示例用法（速览）

```rust
use actix_web::{HttpRequest, web, Responder};
use actix_web::web::{Data, ReqData};
use hirust_router::{GetMapping, PostMapping, PutMapping, DeleteMapping, HeadMapping};

#[GetMapping(path = "/info", tag = "LoginController.Info",
             middleware = {crate::middlewares::auth::auth, crate::middlewares::auth::my_auth_middleware},
             desc = "用户信息", auth = false)]
async fn info(req: HttpRequest, msg: Option<ReqData<Option<String>>>,
              data: Data<AppState>) -> impl Responder { ... }

// main.rs —— 一行完成全部声明式路由的注册
HttpServer::new(move || {
    App::new()
        .app_data(web::Data::new(state.clone()))
        .configure(hirust_router::configure)
}).bind(("127.0.0.1", 8080))?.run().await
```
