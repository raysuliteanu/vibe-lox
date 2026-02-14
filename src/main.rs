use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser};

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

    /// Dump tokens and exit
    #[arg(long)]
    dump_tokens: bool,

    /// Dump AST and exit
    #[arg(long)]
    dump_ast: bool,

    /// AST output format
    #[arg(long, default_value = "sexp", value_parser = ["sexp", "json"])]
    ast_format: String,

    /// Compile to bytecode and save to a .blox file (derived from input path)
    #[arg(long)]
    compile_bytecode: bool,

    /// Compile to LLVM IR
    #[arg(long)]
    compile_llvm: bool,

    /// Suppress informational output
    #[arg(short = 'q')]
    quiet: bool,

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

fn get_filename(cli: &Cli) -> String {
    cli.file
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<input>".to_string())
}

fn compile_source(source: &str) -> Result<chunk::Chunk> {
    vibe_lox::vm::compile_to_chunk(source).map_err(|e| anyhow::anyhow!("{e}"))
}

fn run_source(source: &str, filename: &str) -> Result<()> {
    let tokens =
        scanner::scan(source).map_err(|errors| report_compile_errors(errors, filename, source))?;
    let program = LoxParser::new(tokens)
        .parse()
        .map_err(|errors| report_compile_errors(errors, filename, source))?;
    let locals = Resolver::new()
        .resolve(&program)
        .map_err(|errors| report_compile_errors(errors, filename, source))?;
    let mut interpreter = Interpreter::new();
    interpreter.set_source(source);
    interpreter
        .interpret(&program, locals)
        .map_err(|e| report_runtime_error(&e, Some(source)))?;
    Ok(())
}

/// Magic number at the start of every `.blox` file: ASCII "blox"
const BLOX_MAGIC: &[u8; 4] = b"blox";

fn save_chunk(compiled: &chunk::Chunk, path: &PathBuf) -> Result<()> {
    let payload = rmp_serde::to_vec(compiled).context("serialize bytecode to MessagePack")?;
    let mut bytes = Vec::with_capacity(BLOX_MAGIC.len() + payload.len());
    bytes.extend_from_slice(BLOX_MAGIC);
    bytes.extend_from_slice(&payload);
    std::fs::write(path, bytes).with_context(|| format!("write bytecode to '{}'", path.display()))
}

fn load_chunk(path: &PathBuf) -> Result<chunk::Chunk> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read bytecode from '{}'", path.display()))?;
    if bytes.len() < BLOX_MAGIC.len() || &bytes[..BLOX_MAGIC.len()] != BLOX_MAGIC {
        bail!(
            "'{}' is not a valid .blox file (missing magic header)",
            path.display()
        );
    }
    rmp_serde::from_slice(&bytes[BLOX_MAGIC.len()..])
        .context("deserialize bytecode from MessagePack")
}

fn is_bytecode_file(path: &PathBuf) -> Result<bool> {
    let file =
        std::fs::File::open(path).with_context(|| format!("open file '{}'", path.display()))?;
    let mut header = [0u8; 4];
    use std::io::Read;
    match file.take(4).read(&mut header) {
        Ok(4) => Ok(&header == BLOX_MAGIC),
        _ => Ok(false),
    }
}

fn report_compile_errors(
    errors: Vec<vibe_lox::error::CompileError>,
    filename: &str,
    source: &str,
) -> anyhow::Error {
    let count = errors.len();
    for error in errors {
        let error_with_src = error.with_source_code(filename, source);
        eprintln!("{:?}", miette::Report::new(error_with_src));
    }
    anyhow::anyhow!("{} compile error(s)", count)
}

fn report_runtime_error(
    error: &vibe_lox::error::RuntimeError,
    source: Option<&str>,
) -> anyhow::Error {
    // Don't report Return as an error
    if error.is_return() {
        return anyhow::anyhow!("unexpected return at top level");
    }

    match source {
        Some(src) => {
            eprintln!("{}", error.display_with_line(src));
        }
        None => {
            eprintln!("{}", error);
        }
    }

    if vibe_lox::error::backtrace_enabled() {
        let bt = vibe_lox::error::format_backtrace(error.backtrace_frames());
        if !bt.is_empty() {
            eprint!("{bt}");
        }
    }

    anyhow::anyhow!("execution failed")
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Validate that the provided file exists before doing anything else
    if let Some(ref path) = cli.file
        && !path.exists()
    {
        let mut cmd = Cli::command();
        let _ = cmd.print_help();
        eprintln!("\n");
        bail!("file not found: '{}'", path.display());
    }

    if cli.dump_tokens {
        let source = read_source(&cli)?;
        let filename = get_filename(&cli);
        let tokens =
            scanner::scan(&source).map_err(|e| report_compile_errors(e, &filename, &source))?;
        for token in &tokens {
            println!("{token}");
        }
        return Ok(());
    }

    if cli.dump_ast {
        let source = read_source(&cli)?;
        let filename = get_filename(&cli);
        let tokens =
            scanner::scan(&source).map_err(|e| report_compile_errors(e, &filename, &source))?;
        let program = LoxParser::new(tokens)
            .parse()
            .map_err(|e| report_compile_errors(e, &filename, &source))?;
        if cli.ast_format.as_str() == "json" {
            print!("{}", printer::to_json(&program))
        } else {
            print!("{}", printer::to_sexp(&program));
        }
        return Ok(());
    }

    // TODO: disassemble doesn't really make sense for source files, only for compiled code
    // what's the use case for disassembly of source code ... looking at what would be generated
    // for a source file?
    if cli.disassemble {
        // autodetect whether input is bytecode or source
        if let Some(ref path) = cli.file
            && is_bytecode_file(path)?
        {
            let compiled = load_chunk(path)?;
            print!(
                "{}",
                chunk::disassemble(&compiled, &path.display().to_string())
                    .context("while disassembling bytecode")?
            );
        } else {
            let source = read_source(&cli)?;
            let compiled = compile_source(&source)?;
            let name = cli
                .file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<script>".to_string());
            print!(
                "{}",
                chunk::disassemble(&compiled, &name).context("while disassembling bytecode")?
            );
        }

        return Ok(());
    }

    // Save bytecode to file (derived from input path: .lox -> .blox)
    if cli.compile_bytecode {
        let input_path = cli
            .file
            .as_ref()
            .context("--compile-bytecode requires an input file")?;
        let output_path = input_path.with_extension("blox");
        let source = read_source(&cli)?;
        let compiled = compile_source(&source)?;
        save_chunk(&compiled, &output_path)?;
        return Ok(());
    }

    if cli.compile_llvm {
        let input_path = cli
            .file
            .as_ref()
            .context("--compile-llvm requires an input file")?;
        let output_path = input_path.with_extension("ll");
        let source = read_source(&cli)?;
        let filename = get_filename(&cli);
        let tokens =
            scanner::scan(&source).map_err(|e| report_compile_errors(e, &filename, &source))?;
        let program = LoxParser::new(tokens)
            .parse()
            .map_err(|e| report_compile_errors(e, &filename, &source))?;
        let ir = vibe_lox::codegen::compile(&program, &source).context("compile to LLVM IR")?;
        std::fs::write(&output_path, &ir)
            .with_context(|| format!("write LLVM IR to '{}'", output_path.display()))?;
        if !cli.quiet {
            println!("Wrote LLVM IR to {}", output_path.display());
        }
        return Ok(());
    }

    match cli.file {
        Some(ref path) => {
            // Autodetect: if the file starts with the "blox" magic, run via VM
            if is_bytecode_file(path)? {
                let compiled = load_chunk(path)?;
                let mut vm = vibe_lox::vm::vm::Vm::new();
                vm.interpret(compiled)
                    .map_err(|e| report_runtime_error(&e, None))?;
            } else {
                let source = read_source(&cli)?;
                let filename = get_filename(&cli);
                run_source(&source, &filename)?;
            }
            Ok(())
        }
        None => {
            vibe_lox::repl::run_repl();
            Ok(())
        }
    }
}
