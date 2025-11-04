//! Kokia CLI - コマンドラインインターフェース
//!
//! Rustの非同期関数デバッガ kokia のREPLインターフェース

use anyhow::Result;
use clap::{Parser, Subcommand};
use kokia_core::{Command, Debugger, StopReason};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tracing_subscriber::EnvFilter;

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
    // tracing subscriberを初期化
    // 環境変数 RUST_LOG でログレベルを制御可能 (例: RUST_LOG=debug kokia run ./binary)
    // デフォルトでは info レベル以上のみ表示
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_line_number(true)
        .with_thread_ids(false)
        .init();

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

/// シンボル名をデマングルするヘルパー関数
fn demangle_name(name: &str) -> String {
    if let Ok(demangled) = rustc_demangle::try_demangle(name) {
        format!("{:#}", demangled)
    } else {
        name.to_string()
    }
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
            println!("  {}. {} @ 0x{:x} (size: {})", i + 1, sym.demangled_name, sym.address, sym.size);
        } else {
            println!("  {}. {} @ 0x{:x}", i + 1, sym.demangled_name, sym.address);
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
        Some(Command::Step) => handle_step(debugger)?,
        Some(Command::Backtrace) => handle_backtrace(debugger)?,
        Some(Command::Locals) => handle_locals(debugger)?,
        Some(Command::AsyncBacktrace) => handle_async_backtrace(debugger)?,
        Some(Command::AsyncTasks) => handle_async_tasks(debugger)?,
        Some(Command::AsyncEdges) => handle_async_edges(debugger)?,
        Some(Command::AsyncEnable) => handle_async_enable(debugger)?,
        Some(Command::AsyncLocals) => {
            handle_async_locals(debugger)?;
        }
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
    use kokia_core::parse::parse_address;

    // まずアドレスとして解釈を試みる
    if let Ok(addr) = parse_address(loc) {
        let bp_id = debugger.set_breakpoint(addr)?;
        println!("Breakpoint {} set at 0x{:x}", bp_id, addr);

        // シンボル情報があれば表示（デマングル済み）
        if let Some(symbol) = debugger.reverse_resolve(addr) {
            println!("  at {}", symbol.demangled_name);
            if let Some((file, line)) = debugger.get_line_info(addr) {
                println!("     ({}:{})", file, line);
            }
        }

        return Ok(());
    }

    // ファイル名:行番号の形式かチェック（例: "main.rs:30"）
    if let Some(colon_pos) = loc.rfind(':') {
        let file_part = &loc[..colon_pos];
        let line_part = &loc[colon_pos + 1..];

        // 行番号部分が数値かチェック
        if let Ok(line_num) = line_part.parse::<u32>() {
            // ファイル名と行番号でブレークポイントを設定
            match debugger.set_breakpoint_by_file_line(file_part, line_num) {
                Ok(bp_id) => {
                    // ブレークポイント情報を取得
                    if let Some(bp) = debugger.breakpoints().find(|b| b.id == bp_id) {
                        println!("Breakpoint {} set at {}:{}", bp_id, file_part, line_num);

                        // シンボル情報があれば表示
                        if let Some(symbol) = debugger.reverse_resolve(bp.address) {
                            println!("  in function: {}", symbol.demangled_name);
                        }

                        // 完全なファイルパスを表示
                        if let Some((full_file, actual_line)) = debugger.get_line_info(bp.address) {
                            println!("  ({}:{})", full_file, actual_line);
                        }
                    }
                    return Ok(());
                }
                Err(e) => {
                    println!("Error: {}", e);
                    return Ok(());
                }
            }
        }
    }

    // シンボル名として解釈（PIEの場合のみベースアドレスを加算）
    // まずシンボルを検索してデマングル名を取得
    let symbols = debugger.find_symbols(loc);
    let matched_symbol = symbols
        .iter()
        .find(|s| s.name == loc || s.demangled_name == loc);

    match debugger.set_breakpoint_by_symbol(loc) {
        Ok(bp_id) => {
            print!("Breakpoint {} set", bp_id);

            // マッチしたシンボルの情報を表示
            if let Some(symbol) = matched_symbol {
                println!(" at {}", symbol.demangled_name);

                // ブレークポイントを検索してアドレスと行番号を表示
                if let Some(bp) = debugger.breakpoints().find(|b| b.id == bp_id) {
                    if let Some((file, line)) = debugger.get_line_info(bp.address) {
                        println!("  ({}:{})", file, line);
                    }
                }
            } else {
                println!(" at symbol '{}'", loc);
            }

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

            // シンボルを逆引き（デマングル済み）
            if let Some(symbol) = debugger.reverse_resolve(pc) {
                println!("In function: {}", symbol.demangled_name);
                if symbol.size > 0 {
                    println!("Function address: 0x{:x}, size: {}", symbol.address, symbol.size);
                }

                // ソースファイルと行番号を表示
                if let Some((file, line)) = debugger.get_line_info(pc) {
                    println!("  at {}:{}", file, line);
                }
            }
        }
        StopReason::Step => {
            println!();
            println!("Stepped (unexpected during continue)");
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

/// Stepコマンドを処理する
fn handle_step(debugger: &mut Debugger) -> Result<()> {
    let stop_reason = debugger.step()?;

    // PCを取得
    let pc = debugger.get_pc()?;
    println!("Stepped to 0x{:x}", pc);

    // シンボルを逆引き（デマングル済み）
    if let Some(symbol) = debugger.reverse_resolve(pc) {
        println!("In function: {}", symbol.demangled_name);

        // ソースファイルと行番号を表示
        if let Some((file, line)) = debugger.get_line_info(pc) {
            println!("  at {}:{}", file, line);
        }
    }

    match stop_reason {
        StopReason::Step => {
            // 通常のステップ実行完了
        }
        StopReason::Breakpoint => {
            println!("(at breakpoint)");
        }
        StopReason::Signal(signal) => {
            println!("Received signal: {:?}", signal);
        }
        StopReason::Exited(code) => {
            println!("Process exited with code {}", code);
        }
        StopReason::Other => {}
    }

    Ok(())
}

/// Backtraceコマンドを処理する
fn handle_backtrace(debugger: &mut Debugger) -> Result<()> {
    let frames = debugger.backtrace()?;

    if frames.is_empty() {
        println!("No stack frames found");
        return Ok(());
    }

    println!("Stack backtrace:");
    for frame in &frames {
        print!("  #{:<3} ", frame.frame_number);

        // 関数名（デマングル済み）
        if let Some(ref name) = frame.function_name {
            print!("{}", name);
        } else {
            print!("<unknown>");
        }

        // ソースファイルと行番号
        if let (Some(ref file), Some(line)) = (&frame.file, frame.line) {
            print!("\n        at {}:{}", file, line);
        }

        // アドレス
        println!("\n        (0x{:x})", frame.pc);
    }

    Ok(())
}

/// Localsコマンドを処理する
fn handle_locals(debugger: &mut Debugger) -> Result<()> {
    use kokia_dwarf::VariableLocation;

    match debugger.get_local_variables() {
        Ok(variables) => {
            if variables.is_empty() {
                println!("No local variables found");
                println!("Note: Variables may be optimized out. Try compiling with -C opt-level=0");
                return Ok(());
            }

            println!("Local variables:");
            for var in &variables {
                print!("  {} : {}", var.name, var.type_name);

                // 値を表示
                if let Some(ref value) = var.value {
                    print!(" = {}", value);
                } else {
                    print!(" = <unavailable>");
                }

                // ロケーション情報（デバッグ用）
                match &var.location {
                    VariableLocation::FrameOffset(offset) => {
                        println!("  (rbp{:+})", offset);
                    }
                    VariableLocation::Address(addr) => {
                        println!("  (@0x{:x})", addr);
                    }
                    VariableLocation::Register(reg) => {
                        println!("  (reg{})", reg);
                    }
                    VariableLocation::OptimizedOut => {
                        println!("  (optimized out)");
                    }
                    VariableLocation::Unknown => {
                        println!();
                    }
                }
            }
        }
        Err(e) => {
            println!("Failed to get local variables: {}", e);
            println!("Ensure the binary was compiled with debug info (-C debuginfo=2)");
        }
    }

    Ok(())
}

/// タスク情報を整形して表示するヘルパー関数
///
/// # Arguments
/// * `task` - タスク情報
/// * `prefix` - 各行の接頭辞（インデント用）
/// * `verbose` - 詳細モード（true: 複数行、false: 1行）
fn format_task_info(task: &kokia_core::TaskInfo, prefix: &str, verbose: bool) {
    if verbose {
        // 詳細モード：複数行で表示
        print!("{}Task 0x{:x}", prefix, task.id);
        if let Some(ref type_name) = task.type_name {
            // デマングルして表示
            print!("\n{}   Type: {}", prefix, demangle_name(type_name));
        }

        let mut flags = Vec::new();
        if task.is_root {
            flags.push("root task");
        }
        if task.completed {
            flags.push("completed");
        }

        if !flags.is_empty() {
            print!("\n{}   [{}]", prefix, flags.join(", "));
        }
        println!();
    } else {
        // 簡潔モード：1行で表示
        print!("{}Task 0x{:x}", prefix, task.id);

        if let Some(ref type_name) = task.type_name {
            // デマングルして表示
            print!(" ({})", demangle_name(type_name));
        }

        let mut flags = Vec::new();
        if task.is_root {
            flags.push("root");
        }
        if task.completed {
            flags.push("completed");
        }

        if !flags.is_empty() {
            print!(" [{}]", flags.join(", "));
        }
        println!();
    }
}

/// AsyncBacktraceコマンドを処理する
fn handle_async_backtrace(debugger: &mut Debugger) -> Result<()> {
    use kokia_core::Tid;

    // TODO: 現在のスレッドIDを取得する（現時点ではプロセスIDを使用）
    let pid = debugger.pid().ok_or_else(|| anyhow::anyhow!("No process attached"))?;
    let tid = Tid(pid);

    let backtrace = debugger.async_tracker().async_backtrace(tid);

    if backtrace.is_empty() {
        println!("No async backtrace available");
        println!("Note: Async backtrace is built by observing GenFuture::poll calls");
        return Ok(());
    }

    println!("Async backtrace (logical stack):");
    for (i, task_id) in backtrace.iter().enumerate() {
        if let Some(task) = debugger.async_tracker().get_task(*task_id) {
            print!("  #{:<3} ", i);
            format_task_info(task, "     ", true);
        } else {
            println!("  #{} Task 0x{:x}", i, task_id);
        }
    }

    Ok(())
}

/// AsyncTasksコマンドを処理する
fn handle_async_tasks(debugger: &mut Debugger) -> Result<()> {
    let tasks = debugger.async_tracker().all_tasks();

    if tasks.is_empty() {
        println!("No async tasks tracked");
        println!("Note: Tasks are discovered by observing GenFuture::poll calls");
        return Ok(());
    }

    println!("Async tasks ({} total):", tasks.len());
    for task in tasks {
        format_task_info(task, "  ", false);
    }

    Ok(())
}

/// AsyncEdgesコマンドを処理する
fn handle_async_edges(debugger: &mut Debugger) -> Result<()> {
    let edges = debugger.async_tracker().all_edges();

    if edges.is_empty() {
        println!("No async edges tracked");
        println!("Note: Edges (parent-child relationships) are built by observing GenFuture::poll calls");
        return Ok(());
    }

    println!("Async edges (parent awaits child):");
    for edge in edges {
        print!("  0x{:x} -> 0x{:x}", edge.parent, edge.child);

        if let Some(callsite) = debugger.async_tracker().get_callsite(edge.callsite) {
            if let (Some(ref file), Some(line)) = (&callsite.file, callsite.line) {
                print!(" at {}:{}", file, line);
            }
            if let Some(suspend_idx) = callsite.suspend_idx {
                print!(" (suspend_idx: {})", suspend_idx);
            }
        }

        if edge.completed {
            print!(" [completed]");
        }
        println!();
    }

    Ok(())
}

/// AsyncLocalsコマンドを処理する
fn handle_async_locals(debugger: &mut Debugger) -> Result<()> {
    use kokia_dwarf::VariableLocation;

    // 現在のフレームのローカル変数を取得
    match debugger.get_async_locals() {
        Ok(variables) => {
            if variables.is_empty() {
                println!("No local variables found at current frame");
                println!("Note: Variables may be optimized out. Try compiling with -C opt-level=0");
                return Ok(());
            }

            println!("Local variables at current frame:");
            for var in &variables {
                print!("  {} : {}", var.name, var.type_name);

                if let Some(ref value) = var.value {
                    print!(" = {}", value);
                } else {
                    print!(" = <no value>");
                }

                // ロケーション情報も表示（デバッグ用）
                match &var.location {
                    VariableLocation::FrameOffset(offset) => {
                        println!("  (rbp{:+})", offset);
                    }
                    VariableLocation::Register(reg) => {
                        println!("  (reg {})", reg);
                    }
                    VariableLocation::Address(addr) => {
                        println!("  (@{:#x})", addr);
                    }
                    VariableLocation::OptimizedOut => {
                        println!("  (optimized out)");
                    }
                    VariableLocation::Unknown => {
                        println!();
                    }
                }
            }
        }
        Err(e) => {
            println!("Failed to get async local variables: {}", e);
            println!("Ensure the binary was compiled with debug info (-C debuginfo=2)");
        }
    }

    Ok(())
}

/// AsyncEnableコマンドを処理する
fn handle_async_enable(debugger: &mut Debugger) -> Result<()> {
    println!("Enabling async task tracking (runtime-independent mode)...");
    println!("Searching for async function closures...");

    let symbols = debugger.find_genfuture_poll_symbols();

    if symbols.is_empty() {
        println!("Warning: No async function closures found");
        println!("Make sure the binary was compiled with debug symbols.");
        println!();
        println!("Possible causes:");
        println!("  - The binary was compiled without debug info");
        println!("  - All async functions were inlined by the optimizer");
        println!();
        println!("Workaround:");
        println!("  Set breakpoints manually on your async functions:");
        println!("  (kokia) find <your_function_name>");
        println!("  (kokia) break <function_name>::{{{{closure}}}}");
        return Ok(());
    }

    println!("Found {} async function closure(s)", symbols.len());
    if symbols.len() <= 10 {
        for sym in &symbols {
            println!("  - {}", sym.demangled_name);
        }
    } else {
        println!("  (showing first 10)");
        for sym in symbols.iter().take(10) {
            println!("  - {}", sym.demangled_name);
        }
        println!("  ... and {} more", symbols.len() - 10);
    }

    println!();
    println!("Setting breakpoints on async function entry points...");

    let breakpoint_ids = debugger.set_genfuture_poll_breakpoints()?;

    println!("Successfully set {} breakpoint(s) for async tracking", breakpoint_ids.len());
    println!();
    println!("Note: In modern Rust, Future::poll is inlined, so we track async function");
    println!("      entry points instead. This provides basic async task tracking.");
    println!();
    println!("Async tracking is now enabled (runtime-independent).");
    println!("Use 'continue' to run the program and observe async tasks.");
    println!("Use 'async tasks' to see tracked tasks.");
    println!("Use 'async edges' to see parent-child relationships.");
    println!("Use 'async bt' to see the async backtrace.");

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
    } else if cmd.starts_with("locals") {
        handle_async_locals(debugger)?;
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
    println!("  step (s)       - Execute one instruction (step into)");
    println!("  backtrace (bt) - Show stack backtrace");
    println!("  locals (l)     - Show local variables");
    println!("  find <pattern> - Find symbols matching pattern");
    println!();
    println!("Async commands:");
    println!("  async enable   - Enable async tracking (set GenFuture::poll breakpoints)");
    println!("  async list     - List all async-related symbols");
    println!("  async bt       - Show async backtrace (logical stack)");
    println!("  async tasks    - Show all tracked async tasks");
    println!("  async edges    - Show async task parent-child relationships");
    println!("  async locals <TaskId> - Show local variables for an async task");
    println!();
    println!("Examples:");
    println!("  break main");
    println!("  break 0x1234");
    println!("  step");
    println!("  backtrace");
    println!("  find double");
    println!("  async tasks");
}
