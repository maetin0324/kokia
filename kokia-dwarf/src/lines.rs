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
        _unit: &gimli::Unit<EndianSlice<'static, RunTimeEndian>>,
        row: &gimli::LineRow,
    ) -> Result<LineInfo> {
        let address = row.address();
        let line = row.line().map(|l| l.get());
        let column = match row.column() {
            gimli::ColumnType::LeftEdge => None,
            gimli::ColumnType::Column(col) => Some(col.get()),
        };

        // ファイル名を取得（簡略化版）
        let file = None; // TODO: ファイル名の取得は複雑なので一旦スキップ

        Ok(LineInfo {
            address,
            file,
            line,
            column,
        })
    }
}
