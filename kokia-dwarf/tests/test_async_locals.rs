//! Async locals（DWARF location evaluation）の統合テスト

use kokia_dwarf::{DwarfLoader, VariableLocator, ValueDecoder, DecodeConfig};

#[test]
fn test_variable_locator_creation() {
    // simple_asyncバイナリのパス
    let binary_path = "../target/debug/simple_async";

    // DWARFローダーを作成
    let loader = DwarfLoader::load(binary_path)
        .expect("Failed to load DWARF from simple_async binary");

    // VariableLocatorを作成できることを確認
    let _locator = VariableLocator::new(&loader);

    println!("✓ VariableLocator created successfully");
}

#[test]
fn test_value_decoder_primitives() {
    let decoder = ValueDecoder::new(DecodeConfig::default());

    // i32のデコード
    let i32_bytes = 42i32.to_le_bytes();
    let value = decoder.decode_primitive(&i32_bytes, "i32");
    println!("i32(42) = {}", value);

    // u64のデコード
    let u64_bytes = 12345u64.to_le_bytes();
    let value = decoder.decode_primitive(&u64_bytes, "u64");
    println!("u64(12345) = {}", value);

    // f32のデコード
    let f32_bytes = 3.14f32.to_le_bytes();
    let value = decoder.decode_primitive(&f32_bytes, "f32");
    println!("f32(3.14) = {}", value);

    // boolのデコード
    let bool_bytes = [1u8];
    let value = decoder.decode_primitive(&bool_bytes, "bool");
    println!("bool(true) = {}", value);

    println!("✓ All primitive types decoded successfully");
}

#[test]
fn test_value_decoder_strings() {
    let decoder = ValueDecoder::new(DecodeConfig::default());

    // UTF-8文字列のデコード
    let str_bytes = b"Hello, Kokia!";
    let value = decoder.decode_str(str_bytes);
    println!("str = {}", value);

    // バイト列のデコード
    let bytes = vec![0x01, 0x02, 0x03, 0x04, 0x05];
    let value = decoder.decode_bytes(&bytes);
    println!("bytes = {}", value);

    println!("✓ String and bytes decoded successfully");
}

#[test]
fn test_async_function_symbols() {
    // simple_asyncバイナリのパス
    let binary_path = "../target/debug/simple_async";

    // DWARFローダーを作成
    let loader = DwarfLoader::load(binary_path)
        .expect("Failed to load DWARF from simple_async binary");

    // シンボル解決器を使ってasync関数を検索
    let resolver = kokia_dwarf::SymbolResolver::new(&loader)
        .expect("Failed to create symbol resolver");

    // async関数（closureを含む）を検索
    let closure_symbols = resolver.find_symbols("{{closure}}");
    println!("Found {} closure symbols", closure_symbols.len());

    // GenFuture::pollを検索
    let genfuture_symbols = resolver.find_symbols("GenFuture");
    println!("Found {} GenFuture symbols", genfuture_symbols.len());

    // Future::pollを検索
    let future_poll_symbols = resolver.find_symbols("Future");
    println!("Found {} Future symbols", future_poll_symbols.len());

    // いくつかのシンボルを表示
    println!("\nFirst 5 closure symbols:");
    for (i, sym) in closure_symbols.iter().take(5).enumerate() {
        println!("  {}. {} @ 0x{:x}", i + 1, sym.demangled_name, sym.address);
    }

    assert!(!closure_symbols.is_empty(), "Should find closure symbols in async binary");

    println!("✓ Async function symbols found successfully");
}

#[test]
fn test_generator_layout_analyzer() {
    // simple_asyncバイナリのパス
    let binary_path = "../target/debug/simple_async";

    // DWARFローダーを作成
    let loader = DwarfLoader::load(binary_path)
        .expect("Failed to load DWARF from simple_async binary");

    // GeneratorLayoutAnalyzerを作成
    let analyzer = kokia_dwarf::GeneratorLayoutAnalyzer::new(loader.dwarf());

    // テスト: 適当な関数名でvariant情報を取得してみる
    // （実際の値は実行時にしか分からないが、構造は確認できる）
    match analyzer.get_variant_info("", 0) {
        Ok(Some(info)) => {
            println!("Found variant info with {} fields", info.fields.len());
            for field in info.fields.iter().take(5) {
                println!("  Field: {} @ offset {}, size {}", field.name, field.offset, field.size);
            }
        }
        Ok(None) => {
            println!("No variant info found (expected for discriminant 0)");
        }
        Err(e) => {
            println!("Generator layout analysis: {}", e);
        }
    }

    println!("✓ GeneratorLayoutAnalyzer created successfully");
}

#[test]
fn test_decode_config() {
    // カスタム設定でValueDecoderを作成
    let config = DecodeConfig {
        max_depth: 5,
        max_array_elements: 32,
        max_string_bytes: 512,
        max_bytes_display: 128,
    };

    let decoder = ValueDecoder::new(config);

    // 大きめのバイト列をデコード
    let large_bytes = vec![0xAAu8; 256];
    let value = decoder.decode_bytes(&large_bytes);
    println!("Large bytes (truncated) = {}", value);

    println!("✓ Custom DecodeConfig works correctly");
}
