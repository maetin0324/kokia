//! レジスタアクセス機能

use crate::Result;
use nix::unistd::Pid;

/// レジスタ情報
pub struct Registers {
    pid: Pid,
}

impl Registers {
    /// レジスタアクセスを作成する
    pub fn new(pid: i32) -> Self {
        Self {
            pid: Pid::from_raw(pid),
        }
    }

    /// レジスタを読み取る
    pub fn read(&self) -> Result<nix::libc::user_regs_struct> {
        let regs = nix::sys::ptrace::getregs(self.pid)?;
        Ok(regs)
    }

    /// レジスタに書き込む
    pub fn write(&self, regs: nix::libc::user_regs_struct) -> Result<()> {
        nix::sys::ptrace::setregs(self.pid, regs)?;
        Ok(())
    }

    /// プログラムカウンタ（RIP）を取得する
    pub fn get_pc(&self) -> Result<u64> {
        let regs = self.read()?;
        Ok(regs.rip)
    }

    /// プログラムカウンタ（RIP）を設定する
    pub fn set_pc(&self, pc: u64) -> Result<()> {
        let mut regs = self.read()?;
        regs.rip = pc;
        self.write(regs)
    }
}
