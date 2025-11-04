# Kokia

A runtime-independent debugger for Rust async functions.

## Project Structure

```
kokia-core      # Debugger core logic
kokia-async     # Async task tracking and logical stack
kokia-target    # Process control via ptrace
kokia-dwarf     # DWARF debug information
kokia-cli       # Command-line interface
```

## Build

```bash
cargo build --release
```

## Usage

Build your program with debug info and frame pointers:

```bash
RUSTFLAGS="-C debuginfo=2 -C force-frame-pointers=yes" cargo build
```

Run the debugger:

```bash
./target/release/kokia run ./your-program
```

Available commands:

```
find <pattern>     # Search symbols
async funcs        # List async functions
async track        # Set tracking breakpoints
async tasks        # Show tracked tasks
async edges        # Show task relationships
async bt           # Show async backtrace
break <symbol>     # Set breakpoint
continue           # Continue execution
step               # Step instruction
backtrace          # Show call stack
quit               # Exit
```

## How It Works

Kokia detects async functions by identifying closure symbols (`::{{closure}}`) in the binary. It sets breakpoints at function entry and exit points (ret instructions) to track Poll::Ready/Pending states and build the task dependency graph.

The tracker maintains:
- Task list with function names and states
- Parent-child relationships via await points
- Discriminant values (suspend points)
- Logical async call stack

## License

MIT OR Apache-2.0
