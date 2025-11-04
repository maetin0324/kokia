//! Async関数の検出ロジック

/// Async関数検出器
pub struct AsyncDetector {
    excluded_prefixes: Vec<&'static str>,
    excluded_contains: Vec<&'static str>,
}

impl AsyncDetector {
    /// 新しいAsync関数検出器を作成
    pub fn new() -> Self {
        Self {
            excluded_prefixes: vec![
                // ランタイム内部
                "tokio::",
                "async_std::",
                "futures::",
                "mio::",
                // 標準ライブラリ
                "std::",
                "core::",
                "alloc::",
                // よく使われる依存ライブラリ
                "parking_lot",
                "hashbrown::",
                "tracing::",
                "serde::",
                "log::",
                "bytes::",
                "hyper::",
                "h2::",
            ],
            excluded_contains: vec![
                // 特殊なclosure
                "{{constant}}",
                // ランタイム内部
                "::runtime::",
                "::executor::",
                "::task::",
                // システム関数
                "drop_in_place",
                "::fmt::",
                "::clone::",
                "::drop::",
            ],
        }
    }

    /// ユーザー定義のasync closureかどうか判定
    ///
    /// # Arguments
    /// * `name` - シンボル名
    ///
    /// # Returns
    /// ユーザー定義のasync closureの場合はtrue
    pub fn is_user_async_closure(&self, name: &str) -> bool {
        // closure シンボルかチェック
        if !name.contains("{{closure}}") {
            return false;
        }

        // 除外プレフィックスをチェック
        if self
            .excluded_prefixes
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            return false;
        }

        // 除外パターンをチェック
        if self
            .excluded_contains
            .iter()
            .any(|pattern| name.contains(pattern))
        {
            return false;
        }

        true
    }

    /// 除外プレフィックスを追加
    pub fn add_excluded_prefix(&mut self, prefix: &'static str) {
        self.excluded_prefixes.push(prefix);
    }

    /// 除外パターンを追加
    pub fn add_excluded_pattern(&mut self, pattern: &'static str) {
        self.excluded_contains.push(pattern);
    }
}

impl Default for AsyncDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_user_async_closure() {
        let detector = AsyncDetector::new();

        // ユーザー定義のasync関数
        assert!(detector.is_user_async_closure("my_app::compute::{{closure}}"));
        assert!(detector.is_user_async_closure("simple_async::main::{{closure}}"));

        // 除外すべきもの
        assert!(!detector.is_user_async_closure("tokio::runtime::task::{{closure}}"));
        assert!(!detector.is_user_async_closure("std::future::{{closure}}"));
        assert!(!detector.is_user_async_closure("core::drop::drop_in_place::{{closure}}"));
        assert!(!detector.is_user_async_closure("some_function"));  // closure ではない
        assert!(!detector.is_user_async_closure("test::{{constant}}"));  // constant
    }
}
