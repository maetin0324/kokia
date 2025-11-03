//! DWARFローダーとシンボル解決のテスト

use kokia_dwarf::{DwarfLoader, SymbolResolver};

#[test]
fn test_load_simple_async() {
    // simple_asyncバイナリのパス
    let binary_path = "../target/debug/simple_async";

    // DWARFローダーを作成
    let loader = DwarfLoader::load(binary_path)
        .expect("Failed to load DWARF from simple_async binary");

    // シンボル解決器を作成
    let resolver = SymbolResolver::new(&loader)
        .expect("Failed to create symbol resolver");

    // simple_asyncの関数シンボルを検索
    let simple_async_symbols = resolver.find_symbols("simple_async");

    println!("Found {} symbols containing 'simple_async'", simple_async_symbols.len());

    // 最初の10個のシンボルを表示
    for (i, sym) in simple_async_symbols.iter().take(10).enumerate() {
        println!("  {}. {} @ 0x{:x} (size: {})", i + 1, sym.name, sym.address, sym.size);
    }

    assert!(!simple_async_symbols.is_empty(), "Should find simple_async symbols");

    // double関数を探す
    let double_symbols = resolver.find_symbols("double");
    println!("\nFound {} symbols containing 'double'", double_symbols.len());
    for sym in double_symbols.iter().take(5) {
        println!("  {} @ 0x{:x}", sym.name, sym.address);
    }

    assert!(!double_symbols.is_empty(), "Should find double function");

    // main関数を探す
    if let Some(main_addr) = resolver.resolve("main") {
        println!("\nmain function address: 0x{:x}", main_addr);
    }

    // アドレスからシンボルを逆引き
    if let Some(first_sym) = simple_async_symbols.first() {
        if let Some(sym) = resolver.reverse_resolve(first_sym.address) {
            println!("\nReverse resolve 0x{:x} -> {}", first_sym.address, sym.name);
            assert_eq!(sym.address, first_sym.address);
        }
    }
}

#[test]
fn test_find_poll_functions() {
    let binary_path = "../target/debug/simple_async";

    let loader = DwarfLoader::load(binary_path)
        .expect("Failed to load DWARF from simple_async binary");

    let resolver = SymbolResolver::new(&loader)
        .expect("Failed to create symbol resolver");

    // pollを含むシンボルを検索
    let poll_symbols = resolver.find_symbols("poll");

    println!("Found {} symbols containing 'poll'", poll_symbols.len());
    for (i, sym) in poll_symbols.iter().take(20).enumerate() {
        println!("  {}. {} @ 0x{:x}", i + 1, sym.name, sym.address);
    }

    assert!(!poll_symbols.is_empty(), "Should find poll-related symbols");
}
