//! hirust-router-macro
//!
//! 声明式路由注册属性宏（Go 版声明式路由库的 Rust/actix-web 迁移版）。
//!
//! 提供 `#[GetMapping]` / `#[PostMapping]` / `#[PutMapping]` / `#[DeleteMapping]` /
//! `#[HeadMapping]`（及 Go 版同源的 `#[PatchMapping]` / `#[OptionsMapping]`）。
//!
//! 每个宏支持参数：
//! - `path = "/info"`        请求相对路径（缺省 `"/"`；`:id` 参数段自动改写为 actix 的 `{id}`）
//! - `tag = "LoginController.Info"` 唯一标识（缺省为方法完整模块路径 `crate::模块::…::函数名`，
//!   即 `module_path!() + "::" + 函数名`，如 `demo_server::controllers::login::info`；重复注册 panic）
//! - `middleware = {a::m1, a::m2}`  中间件（花括号集合，也可写单个裸路径）
//! - `desc = "用户信息"`      接口描述
//! - `title = "用户信息"`     标题（元数据，对应 Go Route.title）
//! - `front_path = "/login"` 前端菜单路由（元数据，对应 Go Route.frontPath）
//! - `is_data_auth = true`   数据权限标记（元数据，对应 Go Route.isDataAuth；缺省 false）
//! - `auth = true|false`     是否鉴权（缺省继承组/全局配置，最终默认 false）
//! - `group = "/admin"`      归属分组前缀（可选，对应 Go AddGroup）
//! - `cancel_global_prefix = true`  拼接路径时跳过全局组前缀（对应 CancelGlobalGroupPrefix）
//! - `cancel_global_api_prefix = true` 跳过全局 API 组前缀（对应 CancelGlobalApiGroupPrefix）
//!
//! 标注在 `async fn` 处理函数上；函数参数只需满足 actix-web `FromRequest`，
//! 返回值只需满足 `Responder`，例如：
//!
//! ```ignore
//! #[GetMapping(path = "/info", tag = "LoginController.Info",
//!              middleware = {middlewares::auth::auth, middlewares::auth::my_auth_middleware},
//!              desc = "用户信息", auth = false)]
//! async fn info(req: HttpRequest, msg: Option<ReqData<Option<UserInfo>>>,
//!               data: web::Data<AppState>) -> impl Responder { ... }
//! ```

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    braced,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    FnArg, Ident, ItemFn, LitBool, LitStr, Pat, Path, Token,
};

// ---------------------------------------------------------------------------
// 属性参数解析
// ---------------------------------------------------------------------------

/// `#[xxxMapping(...)]` 的参数集合。
struct MappingArgs {
    path: String,
    tag: Option<String>,
    desc: String,
    /// 标题（元数据，对应 Go Route.title）
    title: String,
    /// 前端菜单路由（元数据，对应 Go Route.frontPath）
    front_path: String,
    /// 数据权限标记（元数据，对应 Go Route.isDataAuth；None = 缺省 false）
    is_data_auth: Option<bool>,
    /// None = 未显式声明（继承组/全局，最终默认 false）；Some(v) = 显式声明
    auth: Option<bool>,
    group: String,
    cancel_global_prefix: bool,
    cancel_global_api_prefix: bool,
    middleware: Vec<Path>,
}

impl MappingArgs {
    fn new() -> Self {
        Self {
            path: String::new(),
            tag: None,
            desc: String::new(),
            title: String::new(),
            front_path: String::new(),
            is_data_auth: None,
            auth: None,
            group: String::new(),
            cancel_global_prefix: false,
            cancel_global_api_prefix: false,
            middleware: Vec::new(),
        }
    }
}

impl Parse for MappingArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = MappingArgs::new();
        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "path" => {
                    input.parse::<Token![=]>()?;
                    args.path = input.parse::<LitStr>()?.value();
                }
                "tag" => {
                    input.parse::<Token![=]>()?;
                    args.tag = Some(input.parse::<LitStr>()?.value());
                }
                "desc" => {
                    input.parse::<Token![=]>()?;
                    args.desc = input.parse::<LitStr>()?.value();
                }
                "title" => {
                    input.parse::<Token![=]>()?;
                    args.title = input.parse::<LitStr>()?.value();
                }
                "front_path" => {
                    input.parse::<Token![=]>()?;
                    args.front_path = input.parse::<LitStr>()?.value();
                }
                "is_data_auth" => {
                    input.parse::<Token![=]>()?;
                    args.is_data_auth = Some(input.parse::<LitBool>()?.value);
                }
                "auth" => {
                    input.parse::<Token![=]>()?;
                    args.auth = Some(input.parse::<LitBool>()?.value);
                }
                "group" => {
                    input.parse::<Token![=]>()?;
                    args.group = input.parse::<LitStr>()?.value();
                }
                "cancel_global_prefix" => {
                    input.parse::<Token![=]>()?;
                    args.cancel_global_prefix = input.parse::<LitBool>()?.value;
                }
                "cancel_global_api_prefix" => {
                    input.parse::<Token![=]>()?;
                    args.cancel_global_api_prefix = input.parse::<LitBool>()?.value;
                }
                "middleware" => {
                    input.parse::<Token![=]>()?;
                    // 两种形式：{a::m1, a::m2} 花括号集合，或单个裸路径 a::m1
                    if input.peek(syn::token::Brace) {
                        let content;
                        braced!(content in input);
                        let list = Punctuated::<Path, Token![,]>::parse_terminated(&content)?;
                        args.middleware.extend(list);
                    } else {
                        args.middleware.push(input.parse::<Path>()?);
                    }
                }
                other => {
                    return Err(input.error(format!(
                        "unknown argument `{}`; expected one of: path, tag, middleware, \
                         desc, title, front_path, is_data_auth, auth, group, \
                         cancel_global_prefix, cancel_global_api_prefix",
                        other
                    )));
                }
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else if !input.is_empty() {
                return Err(input.error("expected `,` between mapping arguments"));
            }
        }
        Ok(args)
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// `:id` 参数段改写为 actix 的 `{id}`，并保证以 `/` 开头。
fn normalize_path(path: &str) -> String {
    let mut normalized = String::new();
    if !path.starts_with('/') {
        normalized.push('/');
    }
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        normalized.push('/');
        if let Some(param) = segment.strip_prefix(':') {
            normalized.push('{');
            normalized.push_str(param);
            normalized.push('}');
        } else {
            normalized.push_str(segment);
        }
    }
    if normalized.is_empty() {
        normalized.push('/');
    }
    normalized
}

/// 类型路径最后一个 segment 是否为 `HttpRequest`（覆盖 `HttpRequest`、
/// `actix_web::HttpRequest`、`::actix_web::HttpRequest` 等写法）。
fn is_http_request_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty {
        if type_path.qself.is_none() {
            if let Some(last) = type_path.path.segments.last() {
                return last.ident == "HttpRequest";
            }
        }
    }
    false
}

/// 从函数签名提取 `(参数名, 类型)` 列表；`self` 参数直接报错。
fn extract_params(sig: &syn::Signature) -> syn::Result<Vec<(Pat, syn::Type)>> {
    let mut params = Vec::new();
    for arg in &sig.inputs {
        match arg {
            FnArg::Typed(pat_type) => {
                // 去掉参数上的属性（如 #[allow(...)]）后转发
                params.push(((*pat_type.pat).clone(), (*pat_type.ty).clone()));
            }
            FnArg::Receiver(_) => {
                return Err(syn::Error::new_spanned(
                    sig.fn_token,
                    "mapping macros cannot be applied to methods with `self` receiver; \
                     annotate a free async fn instead",
                ));
            }
        }
    }
    Ok(params)
}

// ---------------------------------------------------------------------------
// 代码生成
// ---------------------------------------------------------------------------

fn expand(method: &str, args: MappingArgs, item: ItemFn) -> TokenStream2 {
    let sig = &item.sig;
    let fn_ident = &sig.ident;

    // 校验：必须是 async fn、非泛型
    if sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            sig.fn_token,
            format!(
                "#{}Mapping handler must be `async fn`",
                method.to_lowercase()
            ),
        )
        .to_compile_error();
    }
    if !sig.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &sig.generics,
            "mapping macros do not support generic handlers",
        )
        .to_compile_error();
    }

    let params = match extract_params(sig) {
        Ok(p) => p,
        Err(err) => return err.to_compile_error(),
    };

    let attach_ident = format_ident!("__hirust_attach_{}", fn_ident);

    // tag 缺省：完整模块路径::函数名 —— module_path!() 在属性展开处取值，
    // 得到 crate名::模块::…::函数名（如 demo_server::controllers::login::info），
    // 对应 Go 版 flag 缺省取 runtime.FuncForPC 函数名的兜底语义
    let tag_expr = match &args.tag {
        Some(tag) => quote! { #tag },
        None => {
            quote! { ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#fn_ident)) }
        }
    };

    let path_normalized = normalize_path(if args.path.is_empty() {
        "/"
    } else {
        &args.path
    });
    let group = &args.group;
    let desc = &args.desc;
    let title = &args.title;
    let front_path = &args.front_path;
    let auth_flag = args.auth;
    let is_data_auth_opt_tokens = match args.is_data_auth {
        Some(v) => quote! { ::core::option::Option::Some(#v) },
        None => quote! { ::core::option::Option::None },
    };

    // 中间件名列表（仅用于路由表展示）
    let mw_names: Vec<String> = args
        .middleware
        .iter()
        .map(|p| quote! { #p }.to_string().replace(' ', ""))
        .collect();

    // actix route 工厂：GET → web::get() ...
    let route_factory = format_ident!("{}", method.to_lowercase());

    // 包装闭包：`|req, payload| async move { 全链执行 }`。
    // 每条路由都生成包装闭包，以保证运行期注册的全局/分组中间件与鉴权继承
    // 在任何路由上都生效。链顺序与 Go 版 handlers 链完全一致：
    //   全局中间件 → 分组中间件 → 鉴权 → 路由级中间件 → handler。
    // 中间件先于参数提取执行（对应 Go 版语义：中间件可注入 req.extensions，
    // 随后手动 FromRequest 提取的 ReqData<T> 才能读到注入数据）。
    //
    // 鉴权表达式：显式 auth = v → 编译期常量；未声明 → 运行期继承
    //（分组链最内层显式 auth > 全局 auth > false，对应 Go 的覆盖继承）。
    let auth_expr = match auth_flag {
        Some(v) => quote! { #v },
        None => quote! { ::hirust_router::group::inherited_auth(#group) },
    };
    // Option<bool> 的显式 token（quote 对 Option 的 ToTokens 会丢失 Some 包装）
    let auth_opt_tokens = match auth_flag {
        Some(v) => quote! { ::core::option::Option::Some(#v) },
        None => quote! { ::core::option::Option::None },
    };
    // 路由级中间件：按书写顺序
    let mw_calls = args.middleware.iter().map(|mw| {
        quote! {
            let __hirust_req = #mw(__hirust_req).await?;
        }
    });
    // 原 fn 中除 HttpRequest 外的参数：在中间件链之后手动提取
    let extractions = params
        .iter()
        .filter(|(_, ty)| !is_http_request_type(ty))
        .map(|(pat, ty)| {
            quote! {
                #[allow(unused_variables)]
                let #pat = <#ty as ::actix_web::FromRequest>::from_request(
                    &__hirust_req, &mut __hirust_payload,
                ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            }
        });
    // 调用参数：HttpRequest 参数传 clone（携带中间件注入的 extensions），其余按名转发
    let call_args = params.iter().map(|(pat, ty)| {
        if is_http_request_type(ty) {
            quote! { __hirust_req.clone() }
        } else {
            quote! { #pat }
        }
    });
    let attach_body = quote! {
        __hirust_res.route(::actix_web::web::#route_factory().to(
            move |mut __hirust_req: ::actix_web::HttpRequest,
                  __hirust_payload_w: ::actix_web::web::Payload| async move {
                // web::Payload(FromRequest 提取器) → dev::Payload(手动提取所需)
                let mut __hirust_payload = __hirust_payload_w.into_inner();
                // 全局中间件 → 分组中间件（运行期注册表，对应 Go globalMiddle → groupMiddle）
                let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                    #group, __hirust_req,
                ).await?;
                // 鉴权（auth = true 时执行默认鉴权中间件）
                let __hirust_req = if #auth_expr {
                    ::hirust_router::middleware::default_auth(__hirust_req).await?
                } else {
                    __hirust_req
                };
                #(#mw_calls)*
                #(#extractions)*
                let __hirust_res = #fn_ident(#(#call_args),*).await;
                // respond_to 返回 HttpResponse<具体Responder的Body>，归一化为 BoxBody
                ::core::result::Result::<
                    ::actix_web::HttpResponse, ::actix_web::Error,
                >::Ok(
                    ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                        .map_into_boxed_body(),
                )
            }
        ))
    };

    let cgp = args.cancel_global_prefix;
    let cgap = args.cancel_global_api_prefix;

    quote! {
        // ① 原函数原样保留
        #item

        // ② 类型化挂载函数：把处理函数（含编译期内联的中间件链）挂到 Resource 上
        #[doc(hidden)]
        #[allow(non_snake_case, unused_variables)]
        fn #attach_ident(
            mut __hirust_res: ::actix_web::Resource,
        ) -> ::actix_web::Resource {
            #attach_body
        }

        // ③ inventory 分布式注册：编译期把路由元数据提交到全局链表，
        //    由 hirust_router::configure 在启动时统一收集注册（对应 Go CollectRoute）
        ::hirust_router::__inventory::submit! {
            ::hirust_router::route::RouteEntry {
                method: #method,
                path: #path_normalized,
                tag: #tag_expr,
                desc: #desc,
                title: #title,
                front_path: #front_path,
                is_data_auth: #is_data_auth_opt_tokens,
                auth: #auth_opt_tokens,
                group: #group,
                middleware_names: &[#(#mw_names),*],
                cancel_global_prefix: #cgp,
                cancel_global_api_prefix: #cgap,
                order: (::core::line!(), ::core::module_path!()),
                attach: #attach_ident,
                is_ws: false,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 宏入口
// ---------------------------------------------------------------------------

macro_rules! mapping_macro {
    ($name:ident, $method:literal, $doc:expr) => {
        #[proc_macro_attribute]
        #[doc = $doc]
        #[allow(non_snake_case)]
        pub fn $name(attr: TokenStream, item: TokenStream) -> TokenStream {
            let args = parse_macro_input!(attr as MappingArgs);
            let item = parse_macro_input!(item as ItemFn);
            expand($method, args, item).into()
        }
    };
}

mapping_macro!(
    GetMapping,
    "GET",
    r##"Register a GET route（Go 版 `router.Get`）.

# 用法

标注在 `async fn` 处理函数上（参数只需满足 `FromRequest`，返回值满足 `Responder`）：

```ignore
#[GetMapping(path = "/info", tag = "LoginController.Info",
             middleware = {middlewares::auth::auth, middlewares::auth::my_auth_middleware},
             desc = "用户信息", auth = false)]
async fn info(req: HttpRequest, msg: Option<ReqData<Option<UserInfo>>>,
              data: web::Data<AppState>) -> impl Responder {
    web::Json(UserInfo { id: 1, username: "admin".to_string() })
}
```

# 宏展开后（等价代码）

```ignore
// ① 原函数原样保留
async fn info(req: HttpRequest, msg: Option<ReqData<Option<UserInfo>>>,
              data: web::Data<AppState>) -> impl Responder {
    web::Json(UserInfo { id: 1, username: "admin".to_string() })
}

// ② 类型化挂载函数：中间件链内联进 handler 包装闭包，
//    执行顺序 = 全局/分组中间件 → 鉴权 → 路由级中间件（书写序）→ handler；
//    handler 参数在中间件链之后手动 FromRequest 提取，
//    因此中间件注入 req.extensions 的数据能被 ReqData<T> 读到
#[doc(hidden)]
fn __hirust_attach_info(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::get().to(
        move |mut __hirust_req: ::actix_web::HttpRequest,
              __hirust_payload_w: ::actix_web::web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                "", __hirust_req,
            ).await?;
            let __hirust_req = if false {   // auth = false → 编译期常量，死代码被消除
                ::hirust_router::middleware::default_auth(__hirust_req).await?
            } else {
                __hirust_req
            };
            let __hirust_req = ::middlewares::auth::auth(__hirust_req).await?;
            let __hirust_req = ::middlewares::auth::my_auth_middleware(__hirust_req).await?;
            let msg = <Option<ReqData<Option<UserInfo>>> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let data = <web::Data<AppState> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let __hirust_res = info(__hirust_req.clone(), msg, data).await;
            ::core::result::Result::<::actix_web::HttpResponse, ::actix_web::Error>::Ok(
                ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                    .map_into_boxed_body(),
            )
        }))
}

// ③ inventory 分布式注册：启动时 hirust_router::configure 统一收集全部路由，
//    校验 tag 唯一 / (method, path) 冲突后，与同 path 的其他方法合并进
//    同一个 web::Resource，最后 cfg.service() 挂载到 actix
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "GET",
        path: "/info",                 // :id 参数段会被自动改写为 {id}
        tag: "LoginController.Info",   // 缺省时为 module_path!() + "::" + 函数名
        desc: "用户信息",
        auth: ::core::option::Option::Some(false),
        group: "",
        middleware_names: &["middlewares::auth::auth",
                            "middlewares::auth::my_auth_middleware"],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),
        attach: __hirust_attach_info,
        is_ws: false,
    }
}
```
"##
);

mapping_macro!(
    PostMapping,
    "POST",
    r##"Register a POST route（Go 版 `router.Post`）.

# 用法

```ignore
#[PostMapping(path = "/login", tag = "LoginController.Login", desc = "用户登录", auth = false)]
async fn login(payload: web::Json<LoginRequest>,
               data: web::Data<AppState>) -> impl Responder {
    web::Json(LoginResponse { token: "secret-token".to_string() })
}
```

# 宏展开后（等价代码）

```ignore
// ① 原函数原样保留
async fn login(payload: web::Json<LoginRequest>,
               data: web::Data<AppState>) -> impl Responder {
    web::Json(LoginResponse { token: "secret-token".to_string() })
}

// ② 类型化挂载函数（本例无中间件，仅保留全局/分组钩子与鉴权分支）
#[doc(hidden)]
fn __hirust_attach_login(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::post().to(
        move |mut __hirust_req: ::actix_web::HttpRequest,
              __hirust_payload_w: ::actix_web::web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                "", __hirust_req,
            ).await?;
            let __hirust_req = if false {
                ::hirust_router::middleware::default_auth(__hirust_req).await?
            } else {
                __hirust_req
            };
            let payload = <web::Json<LoginRequest> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let data = <web::Data<AppState> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let __hirust_res = login(payload, data).await;
            ::core::result::Result::<::actix_web::HttpResponse, ::actix_web::Error>::Ok(
                ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                    .map_into_boxed_body(),
            )
        }))
}

// ③ inventory 注册项（启动时与同 path 的其他方法合并进同一个 web::Resource）
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "POST",
        path: "/login",
        tag: "LoginController.Login",
        desc: "用户登录",
        auth: ::core::option::Option::Some(false),
        group: "",
        middleware_names: &[],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),
        attach: __hirust_attach_login,
        is_ws: false,
    }
}
```
"##
);

mapping_macro!(
    PutMapping,
    "PUT",
    r##"Register a PUT route（Go 版 `router.Put`）.

# 用法

路径中的 `:id` 参数段会自动改写为 actix 的 `{id}`：

```ignore
#[PutMapping(path = "/user/:id", tag = "UserController.Update", desc = "更新用户")]
async fn update(path: web::Path<(u64,)>,
                payload: web::Json<UpdateUser>) -> impl Responder {
    web::Json(UserInfo { id: path.0, username: payload.username.clone() })
}
```

# 宏展开后（等价代码）

```ignore
// ① 原函数原样保留
async fn update(path: web::Path<(u64,)>,
                payload: web::Json<UpdateUser>) -> impl Responder {
    web::Json(UserInfo { id: path.0, username: payload.username.clone() })
}

// ② 类型化挂载函数
#[doc(hidden)]
fn __hirust_attach_update(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::put().to(
        move |mut __hirust_req: ::actix_web::HttpRequest,
              __hirust_payload_w: ::actix_web::web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                "", __hirust_req,
            ).await?;
            let __hirust_req = if false {
                ::hirust_router::middleware::default_auth(__hirust_req).await?
            } else {
                __hirust_req
            };
            let path = <web::Path<(u64,)> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let payload = <web::Json<UpdateUser> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let __hirust_res = update(path, payload).await;
            ::core::result::Result::<::actix_web::HttpResponse, ::actix_web::Error>::Ok(
                ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                    .map_into_boxed_body(),
            )
        }))
}

// ③ inventory 注册项（注意 path 已改写为 {id}）
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "PUT",
        path: "/user/{id}",
        tag: "UserController.Update",
        desc: "更新用户",
        auth: ::core::option::Option::None,   // 未声明 → 运行期继承 分组 > 全局 > false
        group: "",
        middleware_names: &[],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),
        attach: __hirust_attach_update,
        is_ws: false,
    }
}
```
"##
);

mapping_macro!(
    DeleteMapping,
    "DELETE",
    r##"Register a DELETE route（Go 版 `router.Delete`）.

# 用法

```ignore
#[DeleteMapping(path = "/user/:id", tag = "UserController.Delete", desc = "删除用户", auth = true)]
async fn delete(path: web::Path<(u64,)>) -> impl Responder {
    web::Json(format!("deleted {}", path.0))
}
```

# 宏展开后（等价代码）

```ignore
// ① 原函数原样保留
async fn delete(path: web::Path<(u64,)>) -> impl Responder {
    web::Json(format!("deleted {}", path.0))
}

// ② 类型化挂载函数（auth = true → 链首执行默认鉴权，可用
//    hirust_router::set_auth_validator 全局替换校验逻辑）
#[doc(hidden)]
fn __hirust_attach_delete(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::delete().to(
        move |mut __hirust_req: ::actix_web::HttpRequest,
              __hirust_payload_w: ::actix_web::web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                "", __hirust_req,
            ).await?;
            let __hirust_req = if true {
                ::hirust_router::middleware::default_auth(__hirust_req).await?
            } else {
                __hirust_req
            };
            let path = <web::Path<(u64,)> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let __hirust_res = delete(path).await;
            ::core::result::Result::<::actix_web::HttpResponse, ::actix_web::Error>::Ok(
                ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                    .map_into_boxed_body(),
            )
        }))
}

// ③ inventory 注册项
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "DELETE",
        path: "/user/{id}",
        tag: "UserController.Delete",
        desc: "删除用户",
        auth: ::core::option::Option::Some(true),
        group: "",
        middleware_names: &[],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),
        attach: __hirust_attach_delete,
        is_ws: false,
    }
}
```
"##
);

mapping_macro!(
    HeadMapping,
    "HEAD",
    r##"Register a HEAD route（Go 版 `router.Head`）.

# 用法

```ignore
#[HeadMapping(path = "/ping", tag = "LoginController.Ping", desc = "探活", auth = false)]
async fn ping() -> impl Responder {
    actix_web::HttpResponse::Ok().finish()
}
```

# 宏展开后（等价代码）

```ignore
// ① 原函数原样保留
async fn ping() -> impl Responder {
    actix_web::HttpResponse::Ok().finish()
}

// ② 类型化挂载函数（无参数 handler 的最简形态）
#[doc(hidden)]
fn __hirust_attach_ping(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::head().to(
        move |mut __hirust_req: ::actix_web::HttpRequest,
              __hirust_payload_w: ::actix_web::web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                "", __hirust_req,
            ).await?;
            let __hirust_req = if false {
                ::hirust_router::middleware::default_auth(__hirust_req).await?
            } else {
                __hirust_req
            };
            let __hirust_res = ping().await;
            ::core::result::Result::<::actix_web::HttpResponse, ::actix_web::Error>::Ok(
                ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                    .map_into_boxed_body(),
            )
        }))
}

// ③ inventory 注册项
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "HEAD",
        path: "/ping",
        tag: "LoginController.Ping",
        desc: "探活",
        auth: ::core::option::Option::Some(false),
        group: "",
        middleware_names: &[],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),
        attach: __hirust_attach_ping,
        is_ws: false,
    }
}
```
"##
);

mapping_macro!(
    PatchMapping,
    "PATCH",
    r##"Register a PATCH route（Go 版 `router.Patch`）.

# 用法

```ignore
#[PatchMapping(path = "/user/:id", tag = "UserController.Patch", desc = "局部更新用户")]
async fn patch(path: web::Path<(u64,)>) -> impl Responder {
    web::Json(format!("patched {}", path.0))
}
```

# 宏展开后（等价代码）

```ignore
// ① 原函数原样保留
async fn patch(path: web::Path<(u64,)>) -> impl Responder {
    web::Json(format!("patched {}", path.0))
}

// ② 类型化挂载函数（auth 未声明 → 运行期继承分组/全局配置）
#[doc(hidden)]
fn __hirust_attach_patch(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::patch().to(
        move |mut __hirust_req: ::actix_web::HttpRequest,
              __hirust_payload_w: ::actix_web::web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                "", __hirust_req,
            ).await?;
            let __hirust_req = if ::hirust_router::group::inherited_auth("") {
                ::hirust_router::middleware::default_auth(__hirust_req).await?
            } else {
                __hirust_req
            };
            let path = <web::Path<(u64,)> as ::actix_web::FromRequest>::from_request(
                &__hirust_req, &mut __hirust_payload,
            ).await.map_err(::core::convert::Into::<::actix_web::Error>::into)?;
            let __hirust_res = patch(path).await;
            ::core::result::Result::<::actix_web::HttpResponse, ::actix_web::Error>::Ok(
                ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                    .map_into_boxed_body(),
            )
        }))
}

// ③ inventory 注册项
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "PATCH",
        path: "/user/{id}",
        tag: "UserController.Patch",
        desc: "局部更新用户",
        auth: ::core::option::Option::None,
        group: "",
        middleware_names: &[],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),
        attach: __hirust_attach_patch,
        is_ws: false,
    }
}
```
"##
);

mapping_macro!(
    OptionsMapping,
    "OPTIONS",
    r##"Register an OPTIONS route（Go 版 `router.AddRoute("OPTIONS", ...)`）.

# 用法

```ignore
#[OptionsMapping(path = "/preflight", tag = "LoginController.Preflight", desc = "跨域预检")]
async fn preflight() -> impl Responder {
    actix_web::HttpResponse::Ok().finish()
}
```

# 宏展开后（等价代码）

```ignore
// ① 原函数原样保留
async fn preflight() -> impl Responder {
    actix_web::HttpResponse::Ok().finish()
}

// ② 类型化挂载函数
#[doc(hidden)]
fn __hirust_attach_preflight(res: ::actix_web::Resource) -> ::actix_web::Resource {
    res.route(::actix_web::web::options().to(
        move |mut __hirust_req: ::actix_web::HttpRequest,
              __hirust_payload_w: ::actix_web::web::Payload| async move {
            let mut __hirust_payload = __hirust_payload_w.into_inner();
            let __hirust_req = ::hirust_router::middleware::run_global_and_group_middleware(
                "", __hirust_req,
            ).await?;
            let __hirust_req = if ::hirust_router::group::inherited_auth("") {
                ::hirust_router::middleware::default_auth(__hirust_req).await?
            } else {
                __hirust_req
            };
            let __hirust_res = preflight().await;
            ::core::result::Result::<::actix_web::HttpResponse, ::actix_web::Error>::Ok(
                ::actix_web::Responder::respond_to(__hirust_res, &__hirust_req)
                    .map_into_boxed_body(),
            )
        }))
}

// ③ inventory 注册项
::hirust_router::__inventory::submit! {
    ::hirust_router::route::RouteEntry {
        method: "OPTIONS",
        path: "/preflight",
        tag: "LoginController.Preflight",
        desc: "跨域预检",
        auth: ::core::option::Option::None,
        group: "",
        middleware_names: &[],
        cancel_global_prefix: false,
        cancel_global_api_prefix: false,
        order: (::core::line!(), ::core::module_path!()),
        attach: __hirust_attach_preflight,
        is_ws: false,
    }
}
```
"##
);
