//! DWARF ロケーション式評価
//!
//! DWARFのロケーション式を評価して、変数のメモリ上の位置を特定します。

use crate::Result;
use gimli::{Reader, Evaluation, EvaluationResult, Location, Piece, Value};

/// ロケーション評価の結果
#[derive(Debug, Clone)]
pub enum Loc {
    /// レジスタに格納されている
    Reg { reg: u16 },
    /// メモリアドレス
    Addr { addr: u64, size: usize },
    /// 複数のピースから構成される（構造体の一部など）
    Pieces(Vec<LocPiece>),
    /// 最適化により削除された
    Empty,
}

/// ロケーションのピース
#[derive(Debug, Clone)]
pub struct LocPiece {
    /// サイズ（バイト）
    pub size_in_bits: u64,
    /// ビットオフセット
    pub bit_offset: Option<u64>,
    /// 実際のロケーション
    pub location: LocPieceLocation,
}

/// ピースのロケーション
#[derive(Debug, Clone)]
pub enum LocPieceLocation {
    /// レジスタ
    Reg(u16),
    /// メモリアドレス
    Addr(u64),
    /// 値そのもの
    Value(Vec<u8>),
}

/// ロケーション評価器
pub struct LocationEvaluator<'a, R: Reader> {
    eval: Option<Evaluation<R>>,
    frame_base: Option<u64>,
    encoding: gimli::Encoding,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a, R: Reader<Offset = usize>> LocationEvaluator<'a, R> {
    /// 新しいロケーション評価器を作成する
    ///
    /// # Arguments
    /// * `expr` - DWARF ロケーション式
    /// * `frame_base` - フレームベースアドレス（RBP等）
    /// * `encoding` - DWARF エンコーディング情報
    pub fn new(expr: gimli::Expression<R>, frame_base: Option<u64>, encoding: gimli::Encoding) -> Self {
        let eval = expr.evaluation(encoding);
        Self {
            eval: Some(eval),
            frame_base,
            encoding,
            _marker: std::marker::PhantomData,
        }
    }

    /// ロケーション式を評価する
    ///
    /// # Arguments
    /// * `get_reg` - レジスタ値を取得するコールバック
    /// * `read_mem` - メモリを読み取るコールバック
    pub fn evaluate<F, G>(
        &mut self,
        mut get_reg: F,
        mut read_mem: G,
    ) -> Result<Loc>
    where
        F: FnMut(u16) -> Result<u64>,
        G: FnMut(u64, usize) -> Result<Vec<u8>>,
    {
        let mut eval = self.eval.take().ok_or_else(|| anyhow::anyhow!("Evaluation already consumed"))?;

        loop {
            match eval.evaluate()? {
                EvaluationResult::Complete => {
                    break;
                }
                EvaluationResult::RequiresRegister { register, .. } => {
                    let reg_num = register.0 as u16;
                    let value = get_reg(reg_num)?;
                    eval.resume_with_register(Value::Generic(value))?;
                }
                EvaluationResult::RequiresFrameBase => {
                    if let Some(fb) = self.frame_base {
                        eval.resume_with_frame_base(fb)?;
                    } else {
                        return Err(anyhow::anyhow!("Frame base required but not provided"));
                    }
                }
                EvaluationResult::RequiresMemory { address, size, .. } => {
                    let bytes = read_mem(address, size as usize)?;
                    // gimliは結果をu64として期待する場合が多い
                    let mut value_bytes = [0u8; 8];
                    let copy_size = bytes.len().min(8);
                    value_bytes[..copy_size].copy_from_slice(&bytes[..copy_size]);
                    let value = u64::from_le_bytes(value_bytes);
                    eval.resume_with_memory(Value::Generic(value))?;
                }
                other => {
                    return Err(anyhow::anyhow!("Unsupported evaluation result: {:?}", other));
                }
            }
        }

        // 評価結果を取得
        let result = eval.result();

        match result.len() {
            0 => Ok(Loc::Empty),
            1 => {
                // 単一のピース
                let piece = &result[0];
                Self::convert_piece(piece)
            }
            _ => {
                // 複数のピース
                let pieces: Result<Vec<_>> = result
                    .iter()
                    .map(|p| Self::convert_piece_to_loc_piece(p))
                    .collect();
                Ok(Loc::Pieces(pieces?))
            }
        }
    }

    /// Pieceを Loc に変換する（単一ピース用）
    fn convert_piece(piece: &Piece<R>) -> Result<Loc> {
        match piece.location {
            Location::Empty => Ok(Loc::Empty),
            Location::Register { register } => Ok(Loc::Reg {
                reg: register.0 as u16,
            }),
            Location::Address { address } => {
                let size = piece.size_in_bits.map(|b| (b / 8) as usize).unwrap_or(8);
                Ok(Loc::Addr { addr: address, size })
            }
            Location::Value { value: _ } => {
                // 値そのものが格納されている場合、アドレスとしては扱えない
                // Empty として返すか、エラーにする
                Ok(Loc::Empty)
            }
            _ => Ok(Loc::Empty),
        }
    }

    /// Pieceを LocPiece に変換する（複数ピース用）
    fn convert_piece_to_loc_piece(piece: &Piece<R>) -> Result<LocPiece> {
        let size_in_bits = piece.size_in_bits.unwrap_or(0);
        let bit_offset = piece.bit_offset;

        let location = match piece.location {
            Location::Empty => return Err(anyhow::anyhow!("Empty piece location")),
            Location::Register { register } => LocPieceLocation::Reg(register.0 as u16),
            Location::Address { address } => LocPieceLocation::Addr(address),
            Location::Value { value } => {
                // 値を直接取得
                let bytes = match value {
                    Value::Generic(v) => v.to_le_bytes().to_vec(),
                    Value::I8(v) => vec![v as u8],
                    Value::U8(v) => vec![v],
                    Value::I16(v) => v.to_le_bytes().to_vec(),
                    Value::U16(v) => v.to_le_bytes().to_vec(),
                    Value::I32(v) => v.to_le_bytes().to_vec(),
                    Value::U32(v) => v.to_le_bytes().to_vec(),
                    Value::I64(v) => v.to_le_bytes().to_vec(),
                    Value::U64(v) => v.to_le_bytes().to_vec(),
                    Value::F32(v) => v.to_le_bytes().to_vec(),
                    Value::F64(v) => v.to_le_bytes().to_vec(),
                };
                LocPieceLocation::Value(bytes)
            }
            _ => return Err(anyhow::anyhow!("Unsupported piece location")),
        };

        Ok(LocPiece {
            size_in_bits,
            bit_offset,
            location,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_location_types() {
        // テスト用のロケーションタイプの確認
        let loc = Loc::Reg { reg: 5 };
        match loc {
            Loc::Reg { reg } => assert_eq!(reg, 5),
            _ => panic!("Expected Reg"),
        }

        let loc = Loc::Addr { addr: 0x1000, size: 8 };
        match loc {
            Loc::Addr { addr, size } => {
                assert_eq!(addr, 0x1000);
                assert_eq!(size, 8);
            }
            _ => panic!("Expected Addr"),
        }
    }
}
