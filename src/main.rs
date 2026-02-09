use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;

use vibe_lox::ast::printer;
use vibe_lox::interpreter::Interpreter;
use vibe_lox::interpreter::resolver::Resolver;
use vibe_lox::parser::Parser as LoxParser;
use vibe_lox::scanner;
use vibe_lox::vm::chunk;

#[derive(Parser, Debug)]
#[command(name = "vibe-lox", about = "A Lox language interpreter and compiler")]
struct Cli {
    /// Lox source file to run (omit for REPL)
    file: Option<PathBuf>,

    /// Use bytecode VM backend
    #[arg(long)]
    vm: bool,

    /// Compile to LLVM IR
    #[arg(long)]
    compile: bool,

    /// Dump tokens and exit
    #[arg(long)]
    dump_tokens: bool,

    /// Dump AST and exit
    #[arg(long)]
    dump_ast: bool,

    /// AST output format
    #[arg(long, default_value = "sexp", value_parser = ["sexp", "json"])]
    ast_format: String,

    /// Save compiled bytecode to a file
    #[arg(long, value_name = "FILE")]
    save_bytecode: Option<PathBuf>,

    /// Load and execute bytecode from a file
    #[arg(long, value_name = "FILE")]
    load_bytecode: Option<PathBuf>,

    /// Disassemble bytecode (from source or saved file) and print
    #[arg(long)]
    disassemble: bool,
}

fn read_source(cli: &Cli) -> Result<String> {
    match &cli.file {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("read source file '{}'", path.display())),
        None => bail!("source file required for this operation"),
    }
}

fn compile_source(source: &str) -> Result<chunk::Chunk> {
    vibe_lox::vm::compile_to_chunk(source).map_err(|e| anyhow::anyhow!("{e}"))
}

fn run_source(source: &str) -> Result<()> {
    let tokens = scanner::scan(source).map_err(|e| report_lox_errors(&e))?;
    let program = LoxParser::new(tokens)
        .parse()
        .map_err(|e| report_lox_errors(&e))?;
    let locals = Resolver::new()
        .resolve(&program)
        .map_err(|e| report_lox_errors(&e))?;
    let mut interpreter = Interpreter::new();
    interpreter
        .interpret(&program, locals)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

fn run_vm(source: &str) -> Result<()> {
    vibe_lox::vm::interpret_vm(source).map_err(|e| anyhow::anyhow!("{e}"))
}

fn save_chunk(compiled: &chunk::Chunk, path: &PathBuf) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(compiled).context("serialize bytecode to JSON")?;
    std::fs::write(path, bytes).with_context(|| format!("write bytecode to '{}'", path.display()))
}

fn load_chunk(path: &PathBuf) -> Result<chunk::Chunk> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read bytecode from '{}'", path.display()))?;
    serde_json::from_slice(&bytes).context("deserialize bytecode from JSON")
}

fn report_lox_errors(errors: &[vibe_lox::error::LoxError]) -> anyhow::Error {
    for e in errors {
        eprintln!("{e}");
    }
    anyhow::anyhow!("{} error(s)", errors.len())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.dump_tokens {
        let source = read_source(&cli)?;
        let tokens = scanner::scan(&source).map_err(|e| report_lox_errors(&e))?;
        for token in &tokens {
            println!("{token}");
        }
        return Ok(());
    }

    if cli.dump_ast {
        let source = read_source(&cli)?;
        let tokens = scanner::scan(&source).map_err(|e| report_lox_errors(&e))?;
        let program = LoxParser::new(tokens)
            .parse()
            .map_err(|e| report_lox_errors(&e))?;
        match cli.ast_format.as_str() {
            "json" => print!("{}", printer::to_json(&program)),
            _ => print!("{}", printer::to_sexp(&program)),
        }
        return Ok(());
    }

    // Load bytecode from file and execute or disassemble
    if let Some(ref path) = cli.load_bytecode {
        let compiled = load_chunk(path)?;
        if cli.disassemble {
            print!("{}", chunk::disassemble(&compiled, "loaded"));
            return Ok(());
        }
        let mut vm = vibe_lox::vm::vm::Vm::new();
        vm.interpret(compiled).map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(());
    }

    // Disassemble source to bytecode listing
    if cli.disassemble {
        let source = read_source(&cli)?;
        let compiled = compile_source(&source)?;
        let name = cli
            .file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<script>".to_string());
        print!("{}", chunk::disassemble(&compiled, &name));
        return Ok(());
    }

    // Save bytecode to file (optionally also run)
    if let Some(ref path) = cli.save_bytecode {
        let source = read_source(&cli)?;
        let compiled = compile_source(&source)?;
        save_chunk(&compiled, path)?;
        eprintln!("bytecode saved to '{}'", path.display());
        return Ok(());
    }

    if cli.compile {
        bail!("--compile not yet implemented");
    }

    if cli.vm {
        let source = read_source(&cli)?;
        run_vm(&source)?;
        return Ok(());
    }

    match cli.file {
        Some(_) => {
            let source = read_source(&cli)?;
            run_source(&source)?;
            Ok(())
        }
        None => {
            vibe_lox::repl::run_repl();
            Ok(())
        }
    }
}
