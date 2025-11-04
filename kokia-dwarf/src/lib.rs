//! Kokia DWARF デバッグ情報解析
//!
//! このクレートは、ELFファイルとDWARFデバッグ情報の解析機能を提供します。
//! シンボル名の解決、アドレスからソース行への変換、変数のロケーション評価などを行います。

pub mod loader;
pub mod symbols;
pub mod lines;
pub mod variables;
pub mod utils;
pub mod generator_layout;
pub mod loc_eval;
pub mod decode;
pub mod type_info;

pub use loader::DwarfLoader;
pub use symbols::{Symbol, SymbolResolver};
pub use lines::{LineInfo, LineInfoProvider};
pub use variables::{LocalVariable, Variable, VariableLocator, VariableLocation, VariableValue};
pub use utils::FunctionFinder;
pub use generator_layout::{
    DiscriminantLayout, GeneratorLayoutAnalyzer, VariantInfo, FieldInfo,
};
pub use loc_eval::{Loc, LocPiece, LocPieceLocation, LocationEvaluator};
pub use decode::{DisplayValue, ValueDecoder, DecodeConfig};
pub use type_info::{TypeInfo, TypeInfoExtractor, FieldInfo as TypeFieldInfo, VariantInfo as TypeVariantInfo};

/// DWARF解析の結果型
pub type Result<T> = anyhow::Result<T>;
