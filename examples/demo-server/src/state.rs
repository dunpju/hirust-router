use crate::models::UserInfo;

/// 应用全局状态（handler 以 web::Data<AppState> 提取）
#[derive(Debug, Clone)]
pub struct AppState {
    pub app_name: String,
    pub version: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            app_name: "hirust-router-demo".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

impl AppState {
    /// 模拟从存储读取用户信息
    pub fn find_user(&self, id: u64) -> Option<UserInfo> {
        match id {
            1 => Some(UserInfo { id: 1, username: "admin".to_string() }),
            2 => Some(UserInfo { id: 2, username: "dunpju".to_string() }),
            _ => None,
        }
    }
}
