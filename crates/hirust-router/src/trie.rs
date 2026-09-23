//! 注册期路径冲突检测 —— 对应 Go 版 `router/Trie.go` 的 insert 职责。
//!
//! Go 版用 Trie 存储 `(method, absolutePath)`，重复插入直接
//! `panic("... already exist")`；运行期路由匹配交给 actix-web，
//! 这里只保留其**注册期冲突检测**职责：
//! 按 HTTP 方法为根、`/` 分段建树，参数段（`:id` / `{id}`）归一为
//! 通配占位 `{}` 后比较 —— `/head_test/{id}` 与 `/head_test/{name}`
//! 视为同一路径（与 Go 版 ParamMap 行为一致，二者互相冲突）。

use std::collections::BTreeMap;

#[derive(Default)]
pub struct Trie {
    /// method -> (归一化段路径 -> 是否已注册)
    roots: BTreeMap<String, BTreeMap<String, ()>>,
}

impl Trie {
    pub fn new() -> Self {
        Self::default()
    }

    /// 段归一：`:id` / `{id}` / `{}` → `{}`，其余原样
    fn normalize_segment(segment: &str) -> String {
        if segment.starts_with('{') || segment.starts_with(':') {
            "{}".to_string()
        } else {
            segment.to_string()
        }
    }

    fn normalize(path: &str) -> String {
        let mut normalized = String::new();
        for segment in path.split('/') {
            if segment.is_empty() {
                continue;
            }
            normalized.push('/');
            normalized.push_str(&Self::normalize_segment(segment));
        }
        if normalized.is_empty() {
            normalized.push('/');
        }
        normalized
    }

    /// 插入；`(method, absolutePath)` 已存在时 panic（错误信息与 Go 版同款）。
    pub fn insert(&mut self, method: &str, absolute_path: &str) {
        let key = Self::normalize(absolute_path);
        let node = self.roots.entry(method.to_uppercase()).or_default();
        if node.contains_key(&key) {
            panic!(
                "hirust-router: route {}:{} already exist",
                method.to_uppercase(),
                absolute_path
            );
        }
        node.insert(key, ());
    }

    /// 是否已存在（对应 Trie.Has）
    pub fn has(&self, method: &str, absolute_path: &str) -> bool {
        self.roots
            .get(&method.to_uppercase())
            .map(|node| node.contains_key(&Self::normalize(absolute_path)))
            .unwrap_or(false)
    }
}
