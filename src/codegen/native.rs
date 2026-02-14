use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use inkwell::OptimizationLevel;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};

/// Compile an LLVM module to a native ELF executable.
///
/// Emits an object file from the module, links it with the Lox C runtime,
/// and produces a self-contained executable.
pub fn compile_to_executable(module: &Module, output_path: &Path) -> Result<()> {
    let obj_path = output_path.with_extension("o");
    emit_object_file(module, &obj_path)?;

    let link_result = link_executable(&obj_path, output_path);

    // Clean up the intermediate object file regardless of link success
    let _ = std::fs::remove_file(&obj_path);

    link_result
}

/// Emit an object file from an LLVM module using the host target.
fn emit_object_file(module: &Module, obj_path: &Path) -> Result<()> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|msg| anyhow::anyhow!("initialize native target: {msg}"))?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple)
        .map_err(|msg| anyhow::anyhow!("get target from triple: {msg}"))?;

    let cpu = TargetMachine::get_host_cpu_name();
    let features = TargetMachine::get_host_cpu_features();

    let machine = target
        .create_target_machine(
            &triple,
            cpu.to_str().expect("host CPU name is valid UTF-8"),
            features
                .to_str()
                .expect("host CPU features are valid UTF-8"),
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| anyhow::anyhow!("create target machine for {}", triple))?;

    module.set_triple(&triple);
    module.set_data_layout(&machine.get_target_data().get_data_layout());

    machine
        .write_to_file(module, FileType::Object, obj_path)
        .map_err(|msg| anyhow::anyhow!("write object file: {msg}"))
        .context("emit object file")?;

    Ok(())
}

/// Link an object file with the Lox runtime to produce an executable.
fn link_executable(obj_path: &Path, output_path: &Path) -> Result<()> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "gcc".to_string());
    let runtime_obj = env!("LOX_RUNTIME_OBJ");

    let output = Command::new(&cc)
        .arg(obj_path)
        .arg(runtime_obj)
        .arg("-o")
        .arg(output_path)
        .arg("-lm")
        .output()
        .with_context(|| format!("run linker `{cc}`"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("linker `{cc}` failed: {stderr}");
    }

    Ok(())
}
