//! メモリアクセス機能

use crate::Result;
use nix::unistd::Pid;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read as _, Seek, SeekFrom, Write as _};

/// メモリから読み取り可能な型
pub trait MemoryReadable: Sized {
    /// バイト配列から値を構築
    fn from_le_bytes(bytes: &[u8]) -> Result<Self>;

    /// リトルエンディアンバイト配列に変換
    fn to_le_bytes(&self) -> Vec<u8>;

    /// 型のサイズ（バイト数）
    fn size() -> usize;
}

impl MemoryReadable for u64 {
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let array: [u8; 8] = bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to convert {} bytes to u64 array (expected 8 bytes)", bytes.len()))?;
        Ok(u64::from_le_bytes(array))
    }

    fn to_le_bytes(&self) -> Vec<u8> {
        (*self).to_le_bytes().to_vec()
    }

    fn size() -> usize { 8 }
}

impl MemoryReadable for u32 {
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let array: [u8; 4] = bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to convert {} bytes to u32 array (expected 4 bytes)", bytes.len()))?;
        Ok(u32::from_le_bytes(array))
    }

    fn to_le_bytes(&self) -> Vec<u8> {
        (*self).to_le_bytes().to_vec()
    }

    fn size() -> usize { 4 }
}

impl MemoryReadable for u16 {
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let array: [u8; 2] = bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to convert {} bytes to u16 array (expected 2 bytes)", bytes.len()))?;
        Ok(u16::from_le_bytes(array))
    }

    fn to_le_bytes(&self) -> Vec<u8> {
        (*self).to_le_bytes().to_vec()
    }

    fn size() -> usize { 2 }
}

impl MemoryReadable for u8 {
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(anyhow::anyhow!("Cannot read u8 from empty bytes"));
        }
        Ok(bytes[0])
    }

    fn to_le_bytes(&self) -> Vec<u8> {
        vec![*self]
    }

    fn size() -> usize { 1 }
}

/// メモリマッピング情報
#[derive(Debug, Clone)]
pub struct MemoryMapping {
    pub start: usize,
    pub end: usize,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

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

    /// /proc/pid/mem のパスを取得する
    fn mem_path(&self) -> String {
        format!("/proc/{}/mem", self.pid)
    }

    /// メモリからデータを読み取る
    ///
    /// /proc/pid/memを使用してターゲットプロセスのメモリを読み取ります。
    /// /proc/pid/memが使用できない場合（EIOエラー）、PTRACE_PEEKDATAにフォールバックします。
    pub fn read(&self, addr: usize, size: usize) -> Result<Vec<u8>> {
        // まず /proc/pid/mem で試す
        match self.read_via_proc_mem(addr, size) {
            Ok(data) => Ok(data),
            Err(e) => {
                // EIOエラー（未マッピング領域）の場合、ptraceにフォールバック
                if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == std::io::ErrorKind::Other
                        && io_err.raw_os_error() == Some(5)
                    {
                        // EIO (errno 5): ptraceにフォールバック
                        return self.read_via_ptrace(addr, size);
                    }
                }
                Err(e)
            }
        }
    }

    /// /proc/pid/mem経由でメモリを読み取る（内部実装）
    fn read_via_proc_mem(&self, addr: usize, size: usize) -> Result<Vec<u8>> {
        let mem_path = self.mem_path();
        let mut file = File::open(&mem_path)
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", mem_path, e))?;

        // 指定されたアドレスにシーク
        file.seek(SeekFrom::Start(addr as u64))?;

        // データを読み取る
        let mut buffer = vec![0u8; size];
        file.read_exact(&mut buffer)?;

        Ok(buffer)
    }

    /// メモリにデータを書き込む
    ///
    /// /proc/pid/memを使用してターゲットプロセスのメモリに書き込みます。
    pub fn write(&self, addr: usize, data: &[u8]) -> Result<()> {
        let mem_path = self.mem_path();
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

    /// 型付き値を読み取る（ジェネリック版）
    ///
    /// # Examples
    /// ```ignore
    /// let value: u64 = memory.read_typed(addr)?;
    /// let value: u32 = memory.read_typed(addr)?;
    /// ```
    pub fn read_typed<T: MemoryReadable>(&self, addr: usize) -> Result<T> {
        let bytes = self.read(addr, T::size())?;
        T::from_le_bytes(&bytes)
    }

    /// 型付き値を書き込む（ジェネリック版）
    pub fn write_typed<T: MemoryReadable>(&self, addr: usize, value: &T) -> Result<()> {
        self.write(addr, &value.to_le_bytes())
    }

    /// u64値を読み取る（リトルエンディアン）
    pub fn read_u64(&self, addr: usize) -> Result<u64> {
        self.read_typed(addr)
    }

    /// u64値を書き込む（リトルエンディアン）
    pub fn write_u64(&self, addr: usize, value: u64) -> Result<()> {
        self.write_typed(addr, &value)
    }

    /// u32値を読み取る（リトルエンディアン）
    pub fn read_u32(&self, addr: usize) -> Result<u32> {
        self.read_typed(addr)
    }

    /// u32値を書き込む（リトルエンディアン）
    pub fn write_u32(&self, addr: usize, value: u32) -> Result<()> {
        self.write_typed(addr, &value)
    }

    /// u16値を読み取る（リトルエンディアン）
    pub fn read_u16(&self, addr: usize) -> Result<u16> {
        self.read_typed(addr)
    }

    /// u16値を書き込む（リトルエンディアン）
    pub fn write_u16(&self, addr: usize, value: u16) -> Result<()> {
        self.write_typed(addr, &value)
    }

    /// u8値を読み取る
    pub fn read_u8(&self, addr: usize) -> Result<u8> {
        self.read_typed(addr)
    }

    /// u8値を書き込む
    pub fn write_u8(&self, addr: usize, value: u8) -> Result<()> {
        self.write_typed(addr, &value)
    }

    /// /proc/pid/maps を解析してメモリマッピング情報を取得する
    pub fn get_mappings(&self) -> Result<Vec<MemoryMapping>> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let file = File::open(&maps_path)
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", maps_path, e))?;
        let reader = BufReader::new(file);

        let mut mappings = Vec::new();

        for line in reader.lines() {
            let line = line?;
            // フォーマット: "address perms offset dev inode pathname"
            // 例: "7f1234567000-7f1234568000 r-xp 00000000 08:01 123456 /lib/libc.so"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            // アドレス範囲をパース
            let addr_parts: Vec<&str> = parts[0].split('-').collect();
            if addr_parts.len() != 2 {
                continue;
            }

            let start = usize::from_str_radix(addr_parts[0], 16)
                .map_err(|e| anyhow::anyhow!("Failed to parse start address: {}", e))?;
            let end = usize::from_str_radix(addr_parts[1], 16)
                .map_err(|e| anyhow::anyhow!("Failed to parse end address: {}", e))?;

            // パーミッションをパース
            let perms = parts[1];
            let readable = perms.chars().next() == Some('r');
            let writable = perms.chars().nth(1) == Some('w');
            let executable = perms.chars().nth(2) == Some('x');

            mappings.push(MemoryMapping {
                start,
                end,
                readable,
                writable,
                executable,
            });
        }

        Ok(mappings)
    }

    /// 指定されたアドレスが有効なメモリマッピング内にあるかチェックする
    pub fn is_mapped(&self, addr: usize) -> Result<bool> {
        let mappings = self.get_mappings()?;
        Ok(mappings.iter().any(|m| addr >= m.start && addr < m.end))
    }

    /// 実行可能ファイルのベースアドレスを取得する
    ///
    /// PIE（Position Independent Executable）の場合、実行時にランダムなアドレスにロードされます。
    /// このメソッドは、実行可能ファイルの最初の実行可能セグメントのベースアドレスを返します。
    pub fn get_base_address(&self) -> Result<usize> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let file = File::open(&maps_path)
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", maps_path, e))?;
        let reader = BufReader::new(file);

        // 最初の実行可能セグメントを見つけて、ファイルオフセットを引く
        for line in reader.lines() {
            let line = line?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 6 {
                continue;
            }

            // アドレス範囲、パーミッション、オフセットをパース
            let addr_parts: Vec<&str> = parts[0].split('-').collect();
            if addr_parts.len() != 2 {
                continue;
            }

            let perms = parts[1];
            let offset_str = parts[2];

            // 実行可能（x）フラグがあるセグメントを探す
            if perms.chars().nth(2) == Some('x') {
                let start = usize::from_str_radix(addr_parts[0], 16)
                    .map_err(|e| anyhow::anyhow!("Failed to parse base address: {}", e))?;
                let offset = usize::from_str_radix(offset_str, 16)
                    .map_err(|e| anyhow::anyhow!("Failed to parse segment offset: {}", e))?;

                // PIEの場合、シンボルオフセットはファイル内のオフセットなので、
                // セグメントのファイルオフセットを引いた値を返す
                let base = start - offset;
                return Ok(base);
            }
        }

        Err(anyhow::anyhow!("Could not find executable segment in memory mappings"))
    }

    /// PTRACE_PEEKDATAを使用してメモリからデータを読み取る
    ///
    /// /proc/pid/memが使用できない場合のフォールバック。
    /// 小さなデータ読み取り（1-8バイト）に適しています。
    pub fn read_via_ptrace(&self, addr: usize, size: usize) -> Result<Vec<u8>> {
        use nix::sys::ptrace;

        let mut data = Vec::with_capacity(size);
        let word_size = std::mem::size_of::<usize>();

        // word単位で読み取り
        for offset in (0..size).step_by(word_size) {
            let word_addr = (addr + offset) as *mut std::ffi::c_void;
            let word = ptrace::read(self.pid, word_addr)
                .map_err(|e| anyhow::anyhow!("Failed to read via ptrace at 0x{:x}: {}", addr + offset, e))?;

            // wordをバイト列に変換
            let bytes = word.to_ne_bytes();
            let remaining = size - offset;
            let copy_size = remaining.min(word_size);

            data.extend_from_slice(&bytes[..copy_size]);
        }

        data.truncate(size);
        Ok(data)
    }
}

/// kokia_dwarfのMemoryReaderトレイトを実装
impl kokia_dwarf::MemoryReader for Memory {
    fn read_u8(&self, addr: usize) -> Result<u8> {
        self.read_u8(addr)
    }

    fn read_u16(&self, addr: usize) -> Result<u16> {
        self.read_u16(addr)
    }

    fn read_u32(&self, addr: usize) -> Result<u32> {
        self.read_u32(addr)
    }

    fn read_u64(&self, addr: usize) -> Result<u64> {
        self.read_u64(addr)
    }

    fn read(&self, addr: usize, size: usize) -> Result<Vec<u8>> {
        self.read(addr, size)
    }
}
