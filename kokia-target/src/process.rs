//! プロセス制御機能

use crate::Result;
use nix::sys::signal::Signal;
use std::ffi::CString;
use std::path::Path;

/// 停止イベントの種類
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// ブレークポイントヒット（SIGTRAP）
    Breakpoint,
    /// ステップ実行完了（SIGTRAP）
    Step,
    /// シグナル受信
    Signal(Signal),
    /// プロセス終了
    Exited(i32),
    /// その他の停止
    Other,
}

/// デバッグ対象のプロセス
pub struct Process {
    pid: nix::unistd::Pid,
}

impl Process {
    /// 実行可能ファイルを起動してデバッグ対象プロセスを開始する
    ///
    /// 新しいプロセスをforkして起動し、PTRACE_TRACEMEを設定してから
    /// 指定された実行可能ファイルをexecveで実行します。
    /// プロセスは最初の命令で停止状態で返されます。
    /// これにより、メモリマッピングが完全に初期化され、ブレークポイントを安全に設定できます。
    pub fn spawn<P: AsRef<Path>>(program: P, args: &[String]) -> Result<Self> {
        use nix::sys::ptrace;
        use nix::sys::wait::{waitpid, WaitStatus};
        use nix::unistd::{execve, fork, ForkResult};

        // プログラムパスをCStringに変換
        let program_path = program.as_ref().to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid program path"))?;
        let program_cstring = CString::new(program_path)?;

        // 引数をCStringに変換
        let mut cstring_args = vec![program_cstring.clone()];
        for arg in args {
            cstring_args.push(CString::new(arg.as_str())?);
        }

        // 環境変数は親プロセスから継承
        let env: Vec<CString> = std::env::vars()
            .map(|(key, val)| CString::new(format!("{}={}", key, val)).map_err(anyhow::Error::from))
            .collect::<Result<Vec<_>>>()?;

        // forkしてプロセスを生成
        match unsafe { fork()? } {
            ForkResult::Parent { child } => {
                // 親プロセス: 子プロセスが停止するまで待機
                match waitpid(child, None)? {
                    WaitStatus::Stopped(_, _) => {
                        // 子プロセスがexecve後に停止した
                        // メモリマッピングを初期化するために1ステップ実行
                        ptrace::step(child, None)?;

                        // 次の停止を待つ
                        match waitpid(child, None)? {
                            WaitStatus::Stopped(_, _) => {
                                // メモリマッピングが初期化された
                                Ok(Self { pid: child })
                            }
                            status => {
                                Err(anyhow::anyhow!(
                                    "Unexpected wait status after step: {:?}",
                                    status
                                ))
                            }
                        }
                    }
                    status => {
                        Err(anyhow::anyhow!("Unexpected wait status after execve: {:?}", status))
                    }
                }
            }
            ForkResult::Child => {
                // 子プロセス: PTRACE_TRACEMEを設定してexecve
                ptrace::traceme()?;

                // execveを実行（成功すると戻ってこない）
                execve(&program_cstring, &cstring_args, &env)?;

                // execveが失敗した場合はここに到達
                unreachable!("execve failed");
            }
        }
    }

    /// 既存のプロセスにアタッチする
    pub fn attach(pid: i32) -> Result<Self> {
        let pid = nix::unistd::Pid::from_raw(pid);
        nix::sys::ptrace::attach(pid)?;
        Ok(Self { pid })
    }

    /// プロセスIDを取得する
    pub fn pid(&self) -> i32 {
        self.pid.as_raw()
    }

    /// プロセスを実行継続する
    pub fn continue_execution(&self) -> Result<()> {
        nix::sys::ptrace::cont(self.pid, None)?;
        Ok(())
    }

    /// プロセスを実行継続して停止イベントを待機する
    ///
    /// プロセスを実行継続し、次の停止イベント（ブレークポイント、シグナル、終了など）まで待機します。
    pub fn continue_and_wait(&self) -> Result<StopReason> {
        use nix::sys::ptrace;
        use nix::sys::wait::{waitpid, WaitStatus};

        // プロセスを実行継続
        ptrace::cont(self.pid, None)?;

        // 停止イベントを待機
        let status = waitpid(self.pid, None)?;

        match status {
            WaitStatus::Stopped(_, signal) => {
                // SIGTRAPはブレークポイントヒット
                if signal == Signal::SIGTRAP {
                    Ok(StopReason::Breakpoint)
                } else {
                    Ok(StopReason::Signal(signal))
                }
            }
            WaitStatus::Exited(_, code) => Ok(StopReason::Exited(code)),
            WaitStatus::Signaled(_, signal, _) => {
                Ok(StopReason::Signal(signal))
            }
            _ => Ok(StopReason::Other),
        }
    }

    /// 1命令だけ実行して停止する（ステップ実行）
    ///
    /// プロセスの1命令だけを実行し、次の停止イベントまで待機します。
    /// 関数呼び出しの中にも入ります（ステップイン）。
    pub fn step(&self) -> Result<StopReason> {
        use nix::sys::ptrace;
        use nix::sys::wait::{waitpid, WaitStatus};

        // 1命令だけ実行
        ptrace::step(self.pid, None)?;

        // 停止イベントを待機
        let status = waitpid(self.pid, None)?;

        match status {
            WaitStatus::Stopped(_, signal) => {
                // SIGTRAPはステップ実行完了
                // （ブレークポイントヒットの場合は、呼び出し元で判定する）
                if signal == Signal::SIGTRAP {
                    Ok(StopReason::Step)
                } else {
                    Ok(StopReason::Signal(signal))
                }
            }
            WaitStatus::Exited(_, code) => Ok(StopReason::Exited(code)),
            WaitStatus::Signaled(_, signal, _) => {
                Ok(StopReason::Signal(signal))
            }
            _ => Ok(StopReason::Other),
        }
    }

    /// プロセスを停止する（シグナルを送信）
    pub fn stop(&self) -> Result<()> {
        nix::sys::signal::kill(self.pid, nix::sys::signal::Signal::SIGSTOP)?;
        Ok(())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = nix::sys::ptrace::detach(self.pid, None);
    }
}
