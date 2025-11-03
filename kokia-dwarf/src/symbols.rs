//! シンボル解決機能

use crate::{DwarfLoader, Result};
use std::collections::HashMap;
use object::{Object, ObjectSymbol};

/// シンボル情報
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub address: u64,
    pub size: u64,
}

/// シンボル解決
pub struct SymbolResolver {
    /// シンボル名 -> シンボル情報のマップ
    symbols_by_name: HashMap<String, Symbol>,
    /// アドレス -> シンボル情報のマップ（ソート済み）
    symbols_by_address: Vec<Symbol>,
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

                    let sym = Symbol {
                        name: name.to_string(),
                        address,
                        size,
                    };

                    symbols_by_name.insert(name.to_string(), sym.clone());
                    symbols_by_address.push(sym);
                }
            }
        }

        // アドレスでソート
        symbols_by_address.sort_by_key(|s| s.address);

        Ok(Self {
            symbols_by_name,
            symbols_by_address,
        })
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
    pub fn find_symbols(&self, pattern: &str) -> Vec<Symbol> {
        self.symbols_by_name
            .values()
            .filter(|s| s.name.contains(pattern))
            .cloned()
            .collect()
    }
}
