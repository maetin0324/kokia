//! Kokia CLI - コマンドラインインターフェース
//!
//! Rustの非同期関数デバッガ kokia のREPLインターフェース

use anyhow::Result;
use clap::{Parser, Subcommand};
use kokia_core::{Command, Debugger, StopReason};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

/// Kokia - Rust Async Debugger
#[derive(Parser)]
#[command(name = "kokia")]
#[command(version = "0.1.0")]
#[command(about = "Runtime-independent debugger for Rust async functions", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: DebugCommand,
}

#[derive(Subcommand)]
enum DebugCommand {
    /// Launch and debug an executable
    Run {
        /// Path to the executable binary
        binary: String,

        /// Arguments to pass to the program
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Attach to an existing process
    Attach {
        /// Path to the executable binary
        binary: String,

        /// Process ID to attach to
        #[arg(short, long)]
        pid: i32,
    },
}

fn main() -> Result<()> {
    println!("Kokia - Rust Async Debugger");
    println!("Version 0.1.0");
    println!();

    let cli = Cli::parse();
    let mut debugger = init_debugger(cli.command)?;
    run_repl(&mut debugger)?;

    Ok(())
}

/// デバッガを初期化してプロセスにアタッチまたは起動する
fn init_debugger(command: DebugCommand) -> Result<Debugger> {
    let mut debugger = Debugger::new();

    match command {
        DebugCommand::Run { binary, args } => {
            println!("Loading binary: {}", binary);
            println!();

            // バイナリからDWARF情報を読み込む
            debugger.load_binary(&binary)?;
            println!("Loaded DWARF information from {}", binary);

            // プロセスを起動
            debugger.spawn(&binary, &args)?;
            println!("Process spawned and stopped at first instruction");
            println!("Memory mappings are now initialized");
            println!("Set breakpoints and use 'continue' to continue execution");
            println!();
        }
        DebugCommand::Attach { binary, pid } => {
            println!("Loading binary: {}", binary);
            println!("Attaching to process: {}", pid);
            println!();

            // バイナリからDWARF情報を読み込む
            debugger.load_binary(&binary)?;
            println!("Loaded DWARF information from {}", binary);

            // プロセスにアタッチ
            debugger.attach(pid)?;
            println!("Attached to process {}", pid);
            println!();
        }
    }

    Ok(debugger)
}

/// REPLループを実行する
fn run_repl(debugger: &mut Debugger) -> Result<()> {
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

                if let Err(e) = handle_command(debugger, line) {
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

/// シンボルリストを表示するヘルパー関数
fn print_symbol_list(title: &str, symbols: &[kokia_core::Symbol], limit: Option<usize>) {
    if symbols.is_empty() {
        println!("No {} found", title);
        return;
    }

    let display_limit = limit.unwrap_or(symbols.len());
    println!("{} ({} found):", title, symbols.len());

    for (i, sym) in symbols.iter().take(display_limit).enumerate() {
        if sym.size > 0 {
            println!("  {}. {} @ 0x{:x} (size: {})", i + 1, sym.name, sym.address, sym.size);
        } else {
            println!("  {}. {} @ 0x{:x}", i + 1, sym.name, sym.address);
        }
    }

    if symbols.len() > display_limit {
        println!("  ... and {} more", symbols.len() - display_limit);
    }
}

fn handle_command(debugger: &mut Debugger, line: &str) -> Result<()> {
    let parsed_command = Command::parse(line);

    match parsed_command {
        Some(Command::Help) => print_help(),
        Some(Command::Quit) => handle_quit(),
        Some(Command::Break(loc)) => handle_break(debugger, &loc)?,
        Some(Command::Continue) => handle_continue(debugger)?,
        Some(Command::AsyncBacktrace) => handle_async_backtrace(debugger)?,
        None => handle_custom_command(debugger, line)?,
        _ => println!("Command not yet implemented: {}", line),
    }

    Ok(())
}

/// Quitコマンドを処理する
fn handle_quit() {
    println!("Goodbye!");
    std::process::exit(0);
}

/// Breakコマンドを処理する
fn handle_break(debugger: &mut Debugger, loc: &str) -> Result<()> {
    // まず16進数アドレスとして解釈を試みる
    if loc.starts_with("0x") || loc.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(addr) = u64::from_str_radix(&loc.trim_start_matches("0x"), 16) {
            let bp_id = debugger.set_breakpoint(addr)?;
            println!("Breakpoint {} set at 0x{:x}", bp_id, addr);
            return Ok(());
        }
    }

    // シンボル名として解釈（PIEの場合のみベースアドレスを加算）
    match debugger.set_breakpoint_by_symbol(loc) {
        Ok(bp_id) => {
            println!("Breakpoint {} set at symbol '{}'", bp_id, loc);
            Ok(())
        }
        Err(e) => {
            println!("Error: {}", e);
            Ok(())
        }
    }
}

/// Continueコマンドを処理する
fn handle_continue(debugger: &mut Debugger) -> Result<()> {
    println!("Continuing execution...");

    let stop_reason = debugger.continue_and_wait()?;

    match stop_reason {
        StopReason::Breakpoint => {
            println!();
            println!("Breakpoint hit!");

            // PCを取得
            let pc = debugger.get_pc()?;
            println!("Stopped at 0x{:x}", pc);

            // シンボルを逆引き
            if let Some(symbol) = debugger.reverse_resolve(pc) {
                println!("In function: {}", symbol.name);
                if symbol.size > 0 {
                    println!("Function address: 0x{:x}, size: {}", symbol.address, symbol.size);
                }
            }
        }
        StopReason::Signal(signal) => {
            println!();
            println!("Received signal: {:?}", signal);
        }
        StopReason::Exited(code) => {
            println!();
            println!("Process exited with code {}", code);
        }
        StopReason::Other => {
            println!();
            println!("Process stopped (unknown reason)");
        }
    }

    Ok(())
}

/// AsyncBacktraceコマンドを処理する
fn handle_async_backtrace(debugger: &mut Debugger) -> Result<()> {
    let async_symbols = debugger.find_async_symbols();
    print_symbol_list("Async functions", &async_symbols, Some(10));
    Ok(())
}

/// カスタムコマンドを処理する
fn handle_custom_command(debugger: &mut Debugger, line: &str) -> Result<()> {
    if line.starts_with("find ") {
        let pattern = &line[5..];
        let symbols = debugger.find_symbols(pattern);
        let title = format!("Symbols matching '{}'", pattern);
        print_symbol_list(&title, &symbols, Some(10));
    } else if line.starts_with("async ") {
        // async関連のコマンド
        handle_async_command(debugger, &line[6..])?;
    } else {
        println!("Unknown command: {}", line);
        println!("Type 'help' for available commands.");
    }
    Ok(())
}

fn handle_async_command(debugger: &mut Debugger, cmd: &str) -> Result<()> {
    if cmd == "list" || cmd == "ls" {
        let async_symbols = debugger.find_async_symbols();
        print_symbol_list("Async-related symbols", &async_symbols, None);
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
