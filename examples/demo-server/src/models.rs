use serde::{Deserialize, Serialize};

/// 登录请求
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// 登录响应
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

/// 用户信息（由中间件注入 req.extensions，handler 以 ReqData<Option<UserInfo>> 提取）
#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub id: u64,
    pub username: String,
}

/// 通用响应包裹
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self { code: 0, message: "ok".into(), data: Some(data) }
    }

    pub fn err(message: &str) -> Self {
        Self { code: 1, message: message.into(), data: None }
    }
}
