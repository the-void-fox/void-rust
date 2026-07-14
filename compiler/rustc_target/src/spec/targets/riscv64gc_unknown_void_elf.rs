use crate::spec::{Arch, CodeModel, LlvmAbi, Target, TargetMetadata, base};

/// Userspace VOID на RISC-V — те же ISA/ABI, что у ядра (riscv64gc-unknown-none-elf):
/// RV64GC, lp64d, static, medium code model (программы линкуются на 0x4000_0000).
pub(crate) fn target() -> Target {
    let mut options = base::void::opts();
    options.cpu = "generic-rv64".into();
    options.features = "+m,+a,+f,+d,+c,+zicsr,+zifencei".into();
    options.llvm_abiname = LlvmAbi::Lp64d;
    options.max_atomic_width = Some(64);
    options.code_model = Some(CodeModel::Medium);

    Target {
        data_layout: "e-m:e-p:64:64-i64:64-i128:128-n32:64-S128".into(),
        llvm_target: "riscv64".into(),
        metadata: TargetMetadata {
            description: Some("VOID OS RISC-V (RV64GC ISA)".into()),
            tier: Some(3),
            host_tools: Some(false),
            std: Some(true),
        },
        pointer_width: 64,
        arch: Arch::RiscV64,
        options,
    }
}
