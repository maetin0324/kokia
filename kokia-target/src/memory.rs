//! メモリアクセス機能

use crate::Result;
use nix::unistd::Pid;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek, SeekFrom, Write as _};

/// メモリアクセス
pub struct Memory {
    pid: Pid,
}

impl Memory {
    /// メモリアクセスを作成する
    pub fn new(pid: i32) -> Self {
        Self {
            pid: Pid::from_raw(pid),
        }
    }

    /// メモリからデータを読み取る
    ///
    /// /proc/pid/memを使用してターゲットプロセスのメモリを読み取ります。
    pub fn read(&self, addr: usize, size: usize) -> Result<Vec<u8>> {
        let mem_path = format!("/proc/{}/mem", self.pid);
        let mut file = File::open(&mem_path)
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", mem_path, e))?;

        // 指定されたアドレスにシーク
        file.seek(SeekFrom::Start(addr as u64))
            .map_err(|e| anyhow::anyhow!("Failed to seek to address 0x{:x}: {}", addr, e))?;

        // データを読み取る
        let mut buffer = vec![0u8; size];
        file.read_exact(&mut buffer)
            .map_err(|e| anyhow::anyhow!("Failed to read {} bytes from 0x{:x}: {}", size, addr, e))?;

        Ok(buffer)
    }

    /// メモリにデータを書き込む
    ///
    /// /proc/pid/memを使用してターゲットプロセスのメモリに書き込みます。
    pub fn write(&self, addr: usize, data: &[u8]) -> Result<()> {
        let mem_path = format!("/proc/{}/mem", self.pid);
        let mut file = OpenOptions::new()
            .write(true)
            .open(&mem_path)
            .map_err(|e| anyhow::anyhow!("Failed to open {} for writing: {}", mem_path, e))?;

        // 指定されたアドレスにシーク
        file.seek(SeekFrom::Start(addr as u64))
            .map_err(|e| anyhow::anyhow!("Failed to seek to address 0x{:x}: {}", addr, e))?;

        // データを書き込む
        file.write_all(data)
            .map_err(|e| anyhow::anyhow!("Failed to write {} bytes to 0x{:x}: {}", data.len(), addr, e))?;

        Ok(())
    }

    /// u64値を読み取る（リトルエンディアン）
    pub fn read_u64(&self, addr: usize) -> Result<u64> {
        let bytes = self.read(addr, 8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    /// u64値を書き込む（リトルエンディアン）
    pub fn write_u64(&self, addr: usize, value: u64) -> Result<()> {
        self.write(addr, &value.to_le_bytes())
    }

    /// u8値を読み取る
    pub fn read_u8(&self, addr: usize) -> Result<u8> {
        let bytes = self.read(addr, 1)?;
        Ok(bytes[0])
    }

    /// u8値を書き込む
    pub fn write_u8(&self, addr: usize, value: u8) -> Result<()> {
        self.write(addr, &[value])
    }
}
