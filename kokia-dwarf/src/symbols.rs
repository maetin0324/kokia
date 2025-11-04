//! シンボル解決機能

use crate::{DwarfLoader, Result};
use std::collections::HashMap;
use object::{Object, ObjectSymbol};

/// シンボル情報
#[derive(Debug, Clone)]
pub struct Symbol {
    /// マングルされたシンボル名
    pub name: String,
    /// デマングルされたシンボル名（可読な形式）
    pub demangled_name: String,
    pub address: u64,
    pub size: u64,
}

impl Symbol {
    /// シンボルを作成し、デマングルされた名前を設定する
    pub fn new(name: String, address: u64, size: u64) -> Self {
        let demangled_name = demangle_symbol(&name);
        Self {
            name,
            demangled_name,
            address,
            size,
        }
    }

    /// 表示用の名前を取得（デマングル可能ならデマングル後、できなければマングル名）
    pub fn display_name(&self) -> &str {
        &self.demangled_name
    }
}

/// シンボル名をデマングルする
fn demangle_symbol(name: &str) -> String {
    // Rustのシンボルをデマングル
    if let Ok(demangled) = rustc_demangle::try_demangle(name) {
        return format!("{:#}", demangled);
    }

    // C++のシンボルをデマングルする場合は、cpp_demangleクレートを使用
    // 現時点ではRustのみサポート
    name.to_string()
}

/// シンボル解決
pub struct SymbolResolver {
    /// シンボル名 -> シンボル情報のマップ
    symbols_by_name: HashMap<String, Symbol>,
    /// アドレス -> シンボル情報のマップ（ソート済み）
    symbols_by_address: Vec<Symbol>,
    /// PIE（Position Independent Executable）かどうか
    is_pie: bool,
}

impl SymbolResolver {
    /// DWARFローダーからシンボル解決を作成する
    pub fn new(loader: &DwarfLoader) -> Result<Self> {
        let mut symbols_by_name = HashMap::new();
        let mut symbols_by_address = Vec::new();

        // objectファイルからシンボルテーブルを読み取る
        for symbol in loader.object_file().symbols() {
            if let Ok(name) = symbol.name() {
                if !name.is_empty() {
                    let address = symbol.address();
                    let size = symbol.size();

                    let sym = Symbol::new(name.to_string(), address, size);

                    symbols_by_name.insert(name.to_string(), sym.clone());
                    symbols_by_address.push(sym);
                }
            }
        }

        // アドレスでソート
        symbols_by_address.sort_by_key(|s| s.address);

        // PIE判定
        let is_pie = loader.is_pie();

        Ok(Self {
            symbols_by_name,
            symbols_by_address,
            is_pie,
        })
    }

    /// PIE（Position Independent Executable）かどうかを取得する
    pub fn is_pie(&self) -> bool {
        self.is_pie
    }

    /// シンボル名からアドレスを解決する
    pub fn resolve(&self, symbol: &str) -> Option<u64> {
        self.symbols_by_name.get(symbol).map(|s| s.address)
    }

    /// アドレスからシンボル名を解決する（最も近いシンボルを返す）
    pub fn reverse_resolve(&self, addr: u64) -> Option<Symbol> {
        // バイナリサーチで最も近いシンボルを見つける
        match self.symbols_by_address.binary_search_by_key(&addr, |s| s.address) {
            Ok(idx) => Some(self.symbols_by_address[idx].clone()),
            Err(idx) => {
                if idx > 0 {
                    let sym = &self.symbols_by_address[idx - 1];
                    // シンボルのサイズ範囲内かチェック
                    if sym.size > 0 && addr < sym.address + sym.size {
                        Some(sym.clone())
                    } else if addr >= sym.address {
                        // サイズ情報がない場合は単純に最も近いシンボルを返す
                        Some(sym.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    /// すべてのシンボルを取得する
    pub fn all_symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols_by_address.iter()
    }

    /// パターンにマッチするシンボルを検索する
    /// マングル名とデマングル名の両方で検索する
    pub fn find_symbols(&self, pattern: &str) -> Vec<Symbol> {
        self.symbols_by_name
            .values()
            .filter(|s| s.name.contains(pattern) || s.demangled_name.contains(pattern))
            .cloned()
            .collect()
    }
}
