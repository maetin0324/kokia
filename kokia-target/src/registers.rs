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

    /// ベースポインタ（RBP）を取得する
    pub fn get_rbp(&self) -> Result<u64> {
        let regs = self.read()?;
        Ok(regs.rbp)
    }

    /// スタックポインタ（RSP）を取得する
    pub fn get_rsp(&self) -> Result<u64> {
        let regs = self.read()?;
        Ok(regs.rsp)
    }

    /// RDIレジスタを取得する（x86_64 System V ABI 第1引数）
    pub fn get_rdi(&self) -> Result<u64> {
        let regs = self.read()?;
        Ok(regs.rdi)
    }

    /// RAXレジスタを取得する（x86_64 System V ABI 戻り値）
    pub fn get_rax(&self) -> Result<u64> {
        let regs = self.read()?;
        Ok(regs.rax)
    }
}
