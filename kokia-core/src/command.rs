//! デバッガコマンド

/// デバッガコマンド
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// ブレークポイントを設定
    Break(String),
    /// 実行継続
    Continue,
    /// ステップ実行
    Step,
    /// 次の行へ
    Next,
    /// 現在の関数から抜けるまで実行
    Finish,
    /// バックトレース表示
    Backtrace,
    /// ローカル変数表示
    Locals,
    /// 論理スタック（awaitチェーン）表示
    AsyncBacktrace,
    /// async関数のローカル変数表示
    AsyncLocals,
    /// asyncタスク一覧表示
    AsyncTasks,
    /// asyncエッジ（親子関係）表示
    AsyncEdges,
    /// asyncトラッキングを有効化（GenFuture::pollにブレークポイント設定）
    AsyncEnable,
    /// ヘルプ表示
    Help,
    /// 終了
    Quit,
}

impl Command {
    /// コマンド文字列をパースする
    pub fn parse(input: &str) -> Option<Self> {
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "break" | "b" => {
                if parts.len() > 1 {
                    Some(Command::Break(parts[1..].join(" ")))
                } else {
                    None
                }
            }
            "continue" | "c" => Some(Command::Continue),
            "step" | "s" => Some(Command::Step),
            "next" | "n" => Some(Command::Next),
            "finish" | "f" => Some(Command::Finish),
            "backtrace" | "bt" => Some(Command::Backtrace),
            "locals" | "l" => Some(Command::Locals),
            "async" => {
                if parts.len() > 1 {
                    match parts[1] {
                        "bt" | "backtrace" => Some(Command::AsyncBacktrace),
                        "locals" | "l" => Some(Command::AsyncLocals),
                        "tasks" => Some(Command::AsyncTasks),
                        "edges" => Some(Command::AsyncEdges),
                        "enable" => Some(Command::AsyncEnable),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            "help" | "h" | "?" => Some(Command::Help),
            "quit" | "q" | "exit" => Some(Command::Quit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_commands() {
        assert_eq!(Command::parse("continue"), Some(Command::Continue));
        assert_eq!(Command::parse("c"), Some(Command::Continue));
        assert_eq!(Command::parse("step"), Some(Command::Step));
        assert_eq!(Command::parse("async bt"), Some(Command::AsyncBacktrace));
        assert_eq!(Command::parse("quit"), Some(Command::Quit));
    }
}
