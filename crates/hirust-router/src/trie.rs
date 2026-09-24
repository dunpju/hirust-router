//! 前缀树 —— 对应 Go 版 `router/Trie.go` + `router/Node.go`。
//!
//! 双重职责（与 Go 版一致）：
//! 1. **注册期冲突检测**：`insert` 时 `(method, absolutePath)` 终端已存在
//!    直接 `panic("... already exist")`；
//! 2. **运行期路由查找**：`search(method, url)` 按注册模式匹配具体 URL，
//!    参数段（`:id` / `{id}`）匹配任意单个段并提取实际值 —— 对应 Go
//!    `Trie.Search` / `Routes.Search` / `Routes.Route` / `Routes.Exist`。
//!
//! 匹配规则：
//! - 字面量段精确匹配；参数段匹配任意**单个** URL 段（不支持跨段通配）；
//! - 同一位置同时存在字面量子节点与参数子节点时，**字面量优先**
//!   （`/user/list` 优先命中字面量，`/user/42` 走参数段）；
//! - `:id` 与 `{id}` 等价；同一位置的参数段共享节点，
//!   参数名以首次注册为准（与 Go 版 ParamMap 行为一致）。

use std::collections::BTreeMap;

/// 查找结果：命中节点携带的负载 + 按出现顺序提取的路径参数。
pub struct Match<'a, T> {
    pub value: &'a T,
    /// `(参数名, 实际值)`，如注册 `/user/{id}` 匹配 `/user/42` → `[("id", "42")]`
    pub params: Vec<(String, String)>,
}

#[derive(Debug)]
enum Segment {
    Literal(String),
    Param(String),
}

/// 按 `/` 切分路径段，`:x` / `{x}` 解析为参数段。
fn parse(path: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        if let Some(name) = segment.strip_prefix(':') {
            segments.push(Segment::Param(name.to_string()));
        } else if let Some(inner) = segment.strip_prefix('{') {
            let name = inner.strip_suffix('}').unwrap_or(inner);
            segments.push(Segment::Param(name.to_string()));
        } else {
            segments.push(Segment::Literal(segment.to_string()));
        }
    }
    segments
}

struct Node<T> {
    /// 字面量子段 -> 子节点
    children: BTreeMap<String, Node<T>>,
    /// 参数段子节点：`(参数名, 子节点)`
    param_child: Option<(String, Box<Node<T>>)>,
    /// 终端负载：有路由在此节点结束（对应 Go Node.Route + isEnd）
    terminal: Option<T>,
}

impl<T> Default for Node<T> {
    fn default() -> Self {
        Self { children: BTreeMap::new(), param_child: None, terminal: None }
    }
}

/// 以 HTTP 方法为根的前缀树，泛型 `T` 为节点负载
/// （registry 中承载路由表索引 `usize`，对应 Go Node.Route 指向的 Route）。
pub struct Trie<T> {
    roots: BTreeMap<String, Node<T>>,
}

impl<T> Default for Trie<T> {
    fn default() -> Self {
        Self { roots: BTreeMap::new() }
    }
}

impl<T> Trie<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// 插入注册模式（参数段以 `:x` / `{x}` 书写）。
    /// `(method, path)` 终端已存在时 panic —— 错误信息与 Go 版同款。
    pub fn insert(&mut self, method: &str, path: &str, value: T) {
        let method = method.to_uppercase();
        let mut node = self.roots.entry(method.clone()).or_default();
        for segment in parse(path) {
            node = match segment {
                Segment::Literal(literal) => node.children.entry(literal).or_default(),
                Segment::Param(name) => {
                    let child = node
                        .param_child
                        .get_or_insert_with(|| (name, Box::default()));
                    &mut child.1
                }
            };
        }
        if node.terminal.is_some() {
            panic!("hirust-router: route {}:{} already exist", method, path);
        }
        node.terminal = Some(value);
    }

    /// 注册模式是否已存在（按模式本身比对，非 URL 匹配；对应 Go Trie.Has）。
    pub fn has(&self, method: &str, path: &str) -> bool {
        let Some(mut node) = self.roots.get(&method.to_uppercase()) else {
            return false;
        };
        for segment in parse(path) {
            node = match segment {
                Segment::Literal(literal) => match node.children.get(&literal) {
                    Some(child) => child,
                    None => return false,
                },
                Segment::Param(_) => match &node.param_child {
                    Some((_, child)) => child,
                    None => return false,
                },
            };
        }
        node.terminal.is_some()
    }

    /// 按具体 URL 查找（参数段提取实际值；对应 Go Trie.Search）。
    pub fn search<'a>(&'a self, method: &str, url: &str) -> Option<Match<'a, T>> {
        let root = self.roots.get(&method.to_uppercase())?;
        let segments: Vec<&str> = url.split('/').filter(|segment| !segment.is_empty()).collect();
        let (value, params) = match_node(root, &segments)?;
        Some(Match { value, params })
    }
}

/// 递归匹配：字面量优先，参数段兜底；段耗尽时命中终端。
fn match_node<'a, T>(
    node: &'a Node<T>,
    segments: &[&str],
) -> Option<(&'a T, Vec<(String, String)>)> {
    if segments.is_empty() {
        return node.terminal.as_ref().map(|value| (value, Vec::new()));
    }
    let (head, rest) = (segments[0], &segments[1..]);
    // ① 字面量子节点优先
    if let Some(child) = node.children.get(head) {
        if let Some(found) = match_node(child, rest) {
            return Some(found);
        }
    }
    // ② 参数段子节点兜底：当前段提取为参数值
    if let Some((name, child)) = &node.param_child {
        if let Some((value, mut params)) = match_node(child, rest) {
            params.insert(0, (name.clone(), head.to_string()));
            return Some((value, params));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_with_params() {
        let mut trie = Trie::new();
        trie.insert("HEAD", "/y1/y2/y3/head_test/{id}/{name}", "two");
        trie.insert("HEAD", "/y1/y2/y3/head_test/{id}/{name}/tt", "three");

        let hit = trie.search("HEAD", "/y1/y2/y3/head_test/1/gg").unwrap();
        assert_eq!(*hit.value, "two");
        assert_eq!(
            hit.params,
            vec![("id".to_string(), "1".to_string()), ("name".to_string(), "gg".to_string())]
        );

        let hit = trie.search("HEAD", "/y1/y2/y3/head_test/1/gg/tt").unwrap();
        assert_eq!(*hit.value, "three");

        // 参数个数不足 / 方法不匹配 → 未命中
        assert!(trie.search("HEAD", "/y1/y2/y3/head_test/1").is_none());
        assert!(trie.search("GET", "/y1/y2/y3/head_test/1/gg").is_none());
        // 超出模式长度 → 未命中
        assert!(trie.search("HEAD", "/y1/y2/y3/head_test/1/gg/tt/extra").is_none());
    }

    #[test]
    fn colon_and_brace_equivalent() {
        let mut trie = Trie::new();
        trie.insert("GET", "/user/:id", 1);
        assert!(trie.has("GET", "/user/{id}"));
        assert!(trie.has("GET", "/user/:uid"), "同位置参数段共享节点（名字首个注册者生效）");
        let hit = trie.search("GET", "/user/42").unwrap();
        assert_eq!(hit.params, vec![("id".to_string(), "42".to_string())]);
    }

    #[test]
    fn literal_preferred_over_param() {
        let mut trie = Trie::new();
        trie.insert("GET", "/user/list", "literal");
        trie.insert("GET", "/user/{id}", "param");
        assert_eq!(*trie.search("GET", "/user/list").unwrap().value, "literal");
        assert_eq!(*trie.search("GET", "/user/42").unwrap().value, "param");
    }

    #[test]
    fn root_path_and_trailing_slash() {
        let mut trie = Trie::new();
        trie.insert("GET", "/", "root");
        assert!(trie.search("GET", "/").is_some());
        assert!(trie.search("GET", "").is_some());
        trie.insert("GET", "/ping", "ping");
        assert_eq!(*trie.search("GET", "/ping/").unwrap().value, "ping");
    }

    #[test]
    #[should_panic(expected = "already exist")]
    fn duplicate_route_panics() {
        let mut trie = Trie::new();
        trie.insert("GET", "/a", 1);
        trie.insert("GET", "/a", 2);
    }

    #[test]
    #[should_panic(expected = "already exist")]
    fn duplicate_same_shape_params_panic() {
        let mut trie = Trie::new();
        trie.insert("GET", "/user/{id}", 1);
        // 同位置参数段共享节点 → 终端冲突（与 Go 版 ParamMap 行为一致）
        trie.insert("GET", "/user/{name}", 2);
    }
}
