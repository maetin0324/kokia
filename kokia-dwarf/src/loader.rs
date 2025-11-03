//! ELFとDWARFの読み込み機能

use crate::Result;
use std::path::Path;
use std::fs;
use std::rc::Rc;
use object::{Object, ObjectSection};

/// DWARFローダー
pub struct DwarfLoader {
    /// オブジェクトファイル
    object_file: Rc<object::File<'static>>,
    /// DWARFコンテキスト
    dwarf: gimli::Dwarf<gimli::EndianSlice<'static, gimli::RunTimeEndian>>,
}

impl DwarfLoader {
    /// ELFファイルからDWARF情報を読み込む
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        // ファイルを読み込む
        let file_data = fs::read(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file {:?}: {}", path, e))?;

        // メモリリークを防ぐため、Box::leakを使用して'staticライフタイムを得る
        let file_data: &'static [u8] = Box::leak(file_data.into_boxed_slice());

        // objectクレートでELFファイルをパース
        let object_file = object::File::parse(file_data)
            .map_err(|e| anyhow::anyhow!("Failed to parse ELF file {:?}: {}", path, e))?;

        // エンディアンを取得
        let endian = if object_file.is_little_endian() {
            gimli::RunTimeEndian::Little
        } else {
            gimli::RunTimeEndian::Big
        };

        // DWARFセクションを読み込む
        let load_section = |id: gimli::SectionId| -> Result<gimli::EndianSlice<'static, gimli::RunTimeEndian>> {
            let data = object_file
                .section_by_name(id.name())
                .and_then(|section| section.data().ok())
                .unwrap_or(&[]);
            Ok(gimli::EndianSlice::new(data, endian))
        };

        // DWARFコンテキストを構築
        let dwarf = gimli::Dwarf::load(load_section)
            .map_err(|e| anyhow::anyhow!("Failed to load DWARF sections: {}", e))?;

        Ok(Self {
            object_file: Rc::new(object_file),
            dwarf,
        })
    }

    /// DWARFコンテキストへの参照を取得
    pub fn dwarf(&self) -> &gimli::Dwarf<gimli::EndianSlice<'static, gimli::RunTimeEndian>> {
        &self.dwarf
    }

    /// オブジェクトファイルへの参照を取得
    pub fn object_file(&self) -> &object::File<'static> {
        &self.object_file
    }

    /// PIE（Position Independent Executable）かどうかを判定する
    ///
    /// PIE実行ファイルの場合、シンボルアドレスはオフセットであり、
    /// 実行時ベースアドレスを加算する必要があります。
    /// 非PIE実行ファイルの場合、シンボルアドレスは絶対アドレスです。
    pub fn is_pie(&self) -> bool {
        use object::ObjectKind;

        // ELFのタイプを確認
        // ET_DYN (Dynamic/Shared Object) = PIE実行ファイルまたは共有ライブラリ
        // ET_EXEC (Executable) = 非PIE実行ファイル
        matches!(self.object_file.kind(), ObjectKind::Dynamic)
    }
}
