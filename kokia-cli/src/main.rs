//! Kokia CLI - コマンドラインインターフェース
//!
//! Rustの非同期関数デバッガ kokia のREPLインターフェース

use anyhow::Result;
use kokia_core::{Command, Debugger};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::env;

fn main() -> Result<()> {
    println!("Kokia - Rust Async Debugger");
    println!("Version 0.1.0");
    println!();

    // コマンドライン引数をパース
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <binary-path> <pid>", args[0]);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  {} ./target/debug/simple_async 12345", args[0]);
        std::process::exit(1);
    }

    let binary_path = &args[1];
    let pid: i32 = args[2].parse()
        .map_err(|_| anyhow::anyhow!("Invalid PID: {}", args[2]))?;

    println!("Loading binary: {}", binary_path);
    println!("Attaching to process: {}", pid);
    println!();

    // デバッガを初期化
    let mut debugger = Debugger::new();

    // バイナリからDWARF情報を読み込む
    debugger.load_binary(binary_path)?;
    println!("Loaded DWARF information from {}", binary_path);

    // プロセスにアタッチ
    debugger.attach(pid)?;
    println!("Attached to process {}", pid);
    println!();

    println!("Type 'help' for available commands, 'quit' to exit.");
    println!();

    let mut rl = DefaultEditor::new()?;

    loop {
        let readline = rl.readline("(kokia) ");
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(line)?;

                if let Err(e) = handle_command(&mut debugger, line) {
                    eprintln!("Error: {}", e);
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}

fn handle_command(debugger: &mut Debugger, line: &str) -> Result<()> {
    let parsed_command = Command::parse(line);

    match parsed_command {
        Some(Command::Help) => {
            print_help();
        }
        Some(Command::Quit) => {
            println!("Goodbye!");
            std::process::exit(0);
        }
        Some(Command::Break(loc)) => {
            // シンボル名からアドレスを解決
            if let Some(addr) = debugger.resolve_symbol(&loc) {
                let bp_id = debugger.set_breakpoint(addr)?;
                println!("Breakpoint {} set at {} (0x{:x})", bp_id, loc, addr);
            } else {
                // 16進数アドレスとして解釈を試みる
                if let Ok(addr) = u64::from_str_radix(&loc.trim_start_matches("0x"), 16) {
                    let bp_id = debugger.set_breakpoint(addr)?;
                    println!("Breakpoint {} set at 0x{:x}", bp_id, addr);
                } else {
                    println!("Symbol not found: {}", loc);
                }
            }
        }
        Some(Command::Continue) => {
            println!("Continuing execution...");
            debugger.continue_execution()?;
        }
        Some(Command::AsyncBacktrace) => {
            let async_symbols = debugger.find_async_symbols();
            if async_symbols.is_empty() {
                println!("No async functions found");
            } else {
                println!("Async functions ({} found):", async_symbols.len());
                for (i, sym) in async_symbols.iter().take(10).enumerate() {
                    println!("  {}. {} @ 0x{:x}", i + 1, sym.name, sym.address);
                }
                if async_symbols.len() > 10 {
                    println!("  ... and {} more", async_symbols.len() - 10);
                }
            }
        }
        None => {
            // カスタムコマンド処理
            if line.starts_with("find ") {
                let pattern = &line[5..];
                let symbols = debugger.find_symbols(pattern);
                println!("Found {} symbols matching '{}':", symbols.len(), pattern);
                for (i, sym) in symbols.iter().take(10).enumerate() {
                    println!("  {}. {} @ 0x{:x}", i + 1, sym.name, sym.address);
                }
                if symbols.len() > 10 {
                    println!("  ... and {} more", symbols.len() - 10);
                }
            } else if line.starts_with("async ") {
                // async関連のコマンド
                handle_async_command(debugger, &line[6..])?;
            } else {
                println!("Unknown command: {}", line);
                println!("Type 'help' for available commands.");
            }
        }
        _ => {
            println!("Command not yet implemented: {}", line);
        }
    }

    Ok(())
}

fn handle_async_command(debugger: &mut Debugger, cmd: &str) -> Result<()> {
    if cmd == "list" || cmd == "ls" {
        let async_symbols = debugger.find_async_symbols();
        println!("Async-related symbols ({} found):", async_symbols.len());
        for (i, sym) in async_symbols.iter().enumerate() {
            println!("  {}. {} @ 0x{:x} (size: {})", i + 1, sym.name, sym.address, sym.size);
        }
    } else {
        println!("Unknown async command: {}", cmd);
    }
    Ok(())
}

fn print_help() {
    println!("Available commands:");
    println!();
    println!("  help           - Show this help message");
    println!("  quit/exit/q    - Exit the debugger");
    println!();
    println!("Debug commands:");
    println!("  break <loc>    - Set breakpoint at symbol or address");
    println!("  continue (c)   - Continue execution");
    println!("  find <pattern> - Find symbols matching pattern");
    println!();
    println!("Async commands:");
    println!("  async list     - List all async-related symbols");
    println!("  async bt       - Show async functions (same as 'async list')");
    println!();
    println!("Examples:");
    println!("  break main");
    println!("  break 0x1234");
    println!("  find double");
    println!("  async list");
}
