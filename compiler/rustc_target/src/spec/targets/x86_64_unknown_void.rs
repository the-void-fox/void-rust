use crate::spec::{Arch, CodeModel, RustcAbi, StackProbeType, Target, TargetMetadata, base};

/// Userspace VOID на x86_64. Soft-float и без SSE — как x86_64-unknown-none:
/// ядро VOID не сохраняет FPU/SSE-состояние при переключении процессов, поэтому
/// векторные регистры userspace запрещены честно, на уровне таргета. Red zone
/// выключена и code model = small: программы линкуются на 0x4000_0000 (< 2 ГиБ).
pub(crate) fn target() -> Target {
    let mut options = base::void::opts();
    options.cpu = "x86-64".into();
    options.max_atomic_width = Some(64);
    options.features = "-mmx,-sse,-sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2,+soft-float".into();
    options.rustc_abi = Some(RustcAbi::Softfloat);
    options.disable_redzone = true;
    options.plt_by_default = false;
    options.stack_probes = StackProbeType::Inline;
    options.code_model = Some(CodeModel::Small);

    Target {
        llvm_target: "x86_64-unknown-none-elf".into(),
        metadata: TargetMetadata {
            description: Some("VOID OS x86_64 softfloat".into()),
            tier: Some(3),
            host_tools: Some(false),
            std: Some(true),
        },
        pointer_width: 64,
        data_layout:
            "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128".into(),
        arch: Arch::X86_64,
        options,
    }
}
