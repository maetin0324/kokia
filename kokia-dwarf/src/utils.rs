//! DWARF解析のユーティリティ関数

use crate::Result;
use gimli::Reader;

/// 関数DIE検索ユーティリティ
pub struct FunctionFinder;

impl FunctionFinder {
    /// PCを含む関数DIEを検索
    ///
    /// # Arguments
    /// * `unit` - DWARFコンパイルユニット
    /// * `pc` - プログラムカウンタ
    ///
    /// # Returns
    /// 関数DIEのオフセット、見つからない場合はNone
    pub fn find_at_pc<R: Reader>(
        unit: &gimli::Unit<R>,
        pc: u64,
    ) -> Result<Option<gimli::UnitOffset<R::Offset>>> {
        let mut entries = unit.entries();

        while let Some((_, entry)) = entries.next_dfs()? {
            if entry.tag() == gimli::DW_TAG_subprogram {
                if let Some(offset) = Self::check_pc_in_function(entry, pc)? {
                    return Ok(Some(offset));
                }
            }
        }
        Ok(None)
    }

    /// 関数DIEの範囲チェック
    fn check_pc_in_function<R: Reader>(
        entry: &gimli::DebuggingInformationEntry<R>,
        pc: u64,
    ) -> Result<Option<gimli::UnitOffset<R::Offset>>> {
        let (start_addr, end_addr) = match Self::get_function_range(entry) {
            Ok(range) => range,
            Err(_) => return Ok(None),
        };

        if pc >= start_addr && pc < end_addr {
            Ok(Some(entry.offset()))
        } else {
            Ok(None)
        }
    }

    /// 関数のアドレス範囲を取得
    fn get_function_range<R: Reader>(
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<(u64, u64)> {
        let low_pc = entry.attr_value(gimli::DW_AT_low_pc)?;
        let high_pc = entry.attr_value(gimli::DW_AT_high_pc)?;

        let (low_pc_val, high_pc_val) = match (low_pc, high_pc) {
            (Some(l), Some(h)) => (l, h),
            _ => return Err(anyhow::anyhow!("Missing address attributes")),
        };

        let start_addr = match low_pc_val {
            gimli::AttributeValue::Addr(addr) => addr,
            _ => return Err(anyhow::anyhow!("Invalid low_pc")),
        };

        let end_addr = match high_pc_val {
            gimli::AttributeValue::Addr(addr) => addr,
            gimli::AttributeValue::Udata(offset) => start_addr + offset,
            _ => return Err(anyhow::anyhow!("Invalid high_pc")),
        };

        Ok((start_addr, end_addr))
    }
}
