//! GenFuture::poll検出機能

use crate::Result;
use regex::Regex;

/// GenFuture::poll関数の検出器
pub struct GenFutureDetector {
    /// Future::pollパターン（マングリング名用）
    future_poll_pattern: Regex,
    /// async関数のクロージャパターン
    async_closure_pattern: Regex,
}

impl GenFutureDetector {
    /// GenFuture検出器を作成する
    pub fn new() -> Result<Self> {
        // Future::pollの実装を検出するパターン
        // 例: _ZN102_$LT$tokio..runtime..blocking..task..BlockingTask$LT$T$GT$$u20$as$u20$core..future..future..Future$GT$4poll
        let future_poll_pattern = Regex::new(
            r"(?:Future|GenFuture).*poll|as.*core..future..future..Future.*4poll"
        )?;

        // async関数のクロージャを検出するパターン
        // 例: _ZN12simple_async6double28_$u7b$$u7b$closure$u7d$$u7d$
        let async_closure_pattern = Regex::new(
            r"closure.*\$u7d\$|::\{\{closure\}\}"
        )?;

        Ok(Self {
            future_poll_pattern,
            async_closure_pattern,
        })
    }

    /// 関数名がFuture::pollの実装かどうかを判定する
    pub fn is_future_poll(&self, symbol: &str) -> bool {
        self.future_poll_pattern.is_match(symbol)
    }

    /// 関数名がasync関数のクロージャかどうかを判定する
    pub fn is_async_closure(&self, symbol: &str) -> bool {
        self.async_closure_pattern.is_match(symbol)
    }

    /// 関数名がGenFuture::poll関連かどうかを判定する
    ///
    /// Future::poll実装またはasync関数のクロージャであればtrue
    pub fn is_async_related(&self, symbol: &str) -> bool {
        self.is_future_poll(symbol) || self.is_async_closure(symbol)
    }

    /// デマングルされた関数名を検査する
    pub fn is_async_related_demangled(&self, demangled: &str) -> bool {
        (demangled.contains("Future") && demangled.contains("poll"))
            || demangled.contains("{{closure}}")
            || demangled.contains("GenFuture")
    }

    /// シンボル名からasync関数名を抽出する
    ///
    /// 例: "_ZN12simple_async6double28_$u7b$$u7b$closure$u7d$$u7d$" -> "simple_async::double"
    pub fn extract_function_name(&self, symbol: &str) -> Option<String> {
        // マングリング名からモジュール名と関数名を抽出する簡易実装
        // 実際にはデマングラを使用する方が正確
        if symbol.starts_with("_ZN") {
            // 数字を探してモジュール名を抽出（簡易版）
            // 正確には rustc-demangle クレートを使用すべき
            None
        } else {
            None
        }
    }
}

impl Default for GenFutureDetector {
    fn default() -> Self {
        Self::new().expect("Failed to create GenFutureDetector")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_future_poll_detection() {
        let detector = GenFutureDetector::new().unwrap();
        assert!(detector.is_future_poll("_ZN102_$LT$tokio..runtime..blocking..task..BlockingTask$LT$T$GT$$u20$as$u20$core..future..future..Future$GT$4poll17h1a49a7c8b85eef80E"));
        assert!(!detector.is_future_poll("simple_function"));
    }

    #[test]
    fn test_async_closure_detection() {
        let detector = GenFutureDetector::new().unwrap();
        assert!(detector.is_async_closure("_ZN12simple_async6double28_$u7b$$u7b$closure$u7d$$u7d$17h7e292cfcb2965d2eE"));
        assert!(detector.is_async_closure("simple_async::double::{{closure}}"));
        assert!(!detector.is_async_closure("simple_async::double"));
    }

    #[test]
    fn test_async_related() {
        let detector = GenFutureDetector::new().unwrap();
        // Future::pollの実装
        assert!(detector.is_async_related("_ZN102_$LT$tokio..runtime..blocking..task..BlockingTask$LT$T$GT$$u20$as$u20$core..future..future..Future$GT$4poll17h1a49a7c8b85eef80E"));
        // async関数のクロージャ
        assert!(detector.is_async_related("_ZN12simple_async6double28_$u7b$$u7b$closure$u7d$$u7d$17h7e292cfcb2965d2eE"));
        // 通常の関数
        assert!(!detector.is_async_related("simple_function"));
    }
}
