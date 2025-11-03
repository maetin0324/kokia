//! プロセス制御機能

use crate::Result;

/// デバッグ対象のプロセス
pub struct Process {
    pid: nix::unistd::Pid,
}

impl Process {
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
