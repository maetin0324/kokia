//! ソース行情報

use crate::{DwarfLoader, Result};
use gimli::{EndianSlice, RunTimeEndian};

/// ソース行情報
#[derive(Debug, Clone)]
pub struct LineInfo {
    pub address: u64,
    pub file: Option<String>,
    pub line: Option<u64>,
    pub column: Option<u64>,
}

/// ソース行情報の取得
pub struct LineInfoProvider<'a> {
    loader: &'a DwarfLoader,
}

impl<'a> LineInfoProvider<'a> {
    /// ソース行情報プロバイダを作成する
    pub fn new(loader: &'a DwarfLoader) -> Self {
        Self { loader }
    }

    /// 指定されたアドレス範囲内の最初の有効な行番号のアドレスを取得する
    ///
    /// 関数プロローグをスキップして、最初の実際のソースコード行のアドレスを返します。
    /// これはgdbがブレークポイントを設定する位置と同じです。
    pub fn find_first_line_in_range(&self, start_addr: u64, end_addr: u64) -> Result<Option<u64>> {
        let dwarf = self.loader.dwarf();
        let mut units = dwarf.units();

        while let Some(header) = units.next()? {
            let unit = dwarf.unit(header)?;

            // 行番号プログラムを取得
            if let Some(line_program) = unit.line_program.clone() {
                let mut rows = line_program.rows();
                let mut first_line_addr: Option<u64> = None;

                while let Some((_, row)) = rows.next_row()? {
                    let addr = row.address();

                    // 指定された範囲内かチェック
                    if addr >= start_addr && addr < end_addr {
                        // end_sequenceフラグがセットされている行は無視
                        if row.end_sequence() {
                            continue;
                        }

                        // 行番号情報があるかチェック
                        if let Some(line) = row.line() {
                            if line.get() > 0 && row.is_stmt() {
                                // 関数の先頭アドレスと同じ場合はスキップ（関数宣言行）
                                // 最初の実行可能ステートメントを選ぶ
                                if addr == start_addr {
                                    // 関数の先頭はスキップして次を探す
                                    continue;
                                }

                                // 先頭以外の最初のis_stmt行を返す
                                first_line_addr = Some(addr);
                                break;
                            }
                        }
                    }
                }

                // 見つからなかった場合、start_addrを使用（フォールバック）
                if first_line_addr.is_none() {
                    first_line_addr = Some(start_addr);
                }

                if first_line_addr.is_some() {
                    return Ok(first_line_addr);
                }
            }
        }

        Ok(None)
    }

    /// アドレスからソース行情報を取得する
    pub fn lookup(&self, addr: u64) -> Result<Option<LineInfo>> {
        let dwarf = self.loader.dwarf();
        let mut units = dwarf.units();

        while let Some(header) = units.next()? {
            let unit = dwarf.unit(header)?;

            if let Some(line_program) = unit.line_program.clone() {
                let mut rows = line_program.rows();
                let mut prev_row: Option<gimli::LineRow> = None;

                while let Some((_, row)) = rows.next_row()? {
                    let row_addr = row.address();

                    if row_addr > addr {
                        // アドレスを超えた場合、前の行が該当
                        if let Some(prev) = prev_row {
                            return Ok(Some(self.extract_line_info(&unit, &prev)?));
                        }
                        break;
                    }

                    if row_addr == addr {
                        return Ok(Some(self.extract_line_info(&unit, &row)?));
                    }

                    prev_row = Some(row.clone());
                }
            }
        }

        Ok(None)
    }

    /// LineRowから行番号情報を抽出する
    fn extract_line_info(
        &self,
        unit: &gimli::Unit<EndianSlice<'static, RunTimeEndian>>,
        row: &gimli::LineRow,
    ) -> Result<LineInfo> {
        let address = row.address();
        let line = row.line().map(|l| l.get());
        let column = match row.column() {
            gimli::ColumnType::LeftEdge => None,
            gimli::ColumnType::Column(col) => Some(col.get()),
        };

        // ファイル名を取得
        let file = self.get_file_name(unit, row);

        Ok(LineInfo {
            address,
            file,
            line,
            column,
        })
    }

    /// LineRowからファイル名を取得する
    fn get_file_name(
        &self,
        unit: &gimli::Unit<EndianSlice<'static, RunTimeEndian>>,
        row: &gimli::LineRow,
    ) -> Option<String> {
        let dwarf = self.loader.dwarf();
        let file_index = row.file_index();

        // line_programからファイル名を取得
        if let Some(line_program) = &unit.line_program {
            if let Some(file_entry) = line_program.header().file(file_index) {
                // ファイルパスを構築
                let mut path_buf = std::path::PathBuf::new();

                // ディレクトリを取得
                if let Some(dir) = file_entry.directory(line_program.header()) {
                    if let Ok(dir_str) = dwarf.attr_string(unit, dir) {
                        path_buf.push(dir_str.to_string_lossy().as_ref());
                    }
                }

                // ファイル名を追加
                if let Ok(name_str) = dwarf.attr_string(unit, file_entry.path_name()) {
                    path_buf.push(name_str.to_string_lossy().as_ref());
                }

                return Some(path_buf.to_string_lossy().to_string());
            }
        }

        None
    }

    /// ファイル名と行番号からアドレスを検索する
    ///
    /// 指定されたファイル名と行番号に該当するアドレスを検索します。
    /// ファイル名は部分一致で検索されます（例: "main.rs" で "examples/simple_async/src/main.rs" にマッチ）。
    pub fn find_address_by_file_line(&self, file_pattern: &str, target_line: u32) -> Result<Option<u64>> {
        let dwarf = self.loader.dwarf();
        let mut units = dwarf.units();

        while let Some(header) = units.next()? {
            let unit = dwarf.unit(header)?;

            if let Some(line_program) = unit.line_program.clone() {
                let mut rows = line_program.rows();

                while let Some((_, row)) = rows.next_row()? {
                    // 行番号をチェック
                    if let Some(line) = row.line() {
                        if line.get() == target_line as u64 {
                            // ファイル名をチェック
                            if let Some(file_name) = self.get_file_name(&unit, &row) {
                                // 部分一致でファイル名を検索（末尾一致も許容）
                                if file_name.ends_with(file_pattern) || file_name.contains(file_pattern) {
                                    // is_stmt（ステートメント開始位置）を優先
                                    if row.is_stmt() {
                                        return Ok(Some(row.address()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// 現在の行の次の行のアドレスを検索する
    ///
    /// `next`コマンド（ステップオーバー）のために使用。
    /// 現在のアドレスの行番号を取得し、その次の行のアドレスを返す。
    ///
    /// # Arguments
    /// * `current_addr` - 現在のプログラムカウンタ
    ///
    /// # Returns
    /// 次の行の開始アドレス、見つからない場合はNone
    pub fn find_next_line(&self, current_addr: u64) -> Result<Option<u64>> {
        // 現在のアドレスの行情報を取得
        let current_line_info = match self.lookup(current_addr)? {
            Some(info) => info,
            None => return Ok(None),
        };

        let current_line = match current_line_info.line {
            Some(line) => line,
            None => return Ok(None),
        };

        let current_file = match &current_line_info.file {
            Some(file) => file.clone(),
            None => return Ok(None),
        };

        // 同じファイル内で次の行を検索
        let dwarf = self.loader.dwarf();
        let mut units = dwarf.units();

        while let Some(header) = units.next()? {
            let unit = dwarf.unit(header)?;

            if let Some(line_program) = unit.line_program.clone() {
                let mut rows = line_program.rows();

                while let Some((_, row)) = rows.next_row()? {
                    let addr = row.address();

                    // 現在のアドレスより後ろをチェック
                    if addr <= current_addr {
                        continue;
                    }

                    // end_sequenceフラグがセットされている行は無視
                    if row.end_sequence() {
                        continue;
                    }

                    // 行番号情報を取得
                    if let Some(line) = row.line() {
                        let line_num = line.get();

                        // 現在の行より大きい行番号をチェック
                        if line_num > current_line && row.is_stmt() {
                            // ファイル名が一致するかチェック
                            if let Some(file_name) = self.get_file_name(&unit, &row) {
                                if file_name == current_file {
                                    return Ok(Some(addr));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}
