use crate::spec::{Cc, LinkerFlavor, Lld, Os, PanicStrategy, RelocModel, TargetOptions};

/// VOID: микроядро с персистентным контент-адресуемым объектным store
/// (https://github.com/voidfox — Code/kernel). Userspace-программы — статические
/// ELF, загружаемые ядром по content-id; линкуются на USER_REGION_START
/// (0x4000_0000) скриптом линкера на стороне программы.
pub(crate) fn opts() -> TargetOptions {
    TargetOptions {
        os: Os::Void,
        executables: true,
        // ELF-загрузчик VOID не обрабатывает ни релокации, ни PT_TLS:
        // код статический по месту линковки, thread_local в std — "statik"
        // (потоков нет: ядро планирует процессы, не нити внутри них).
        has_thread_local: false,
        relocation_model: RelocModel::Static,
        position_independent_executables: false,
        static_position_independent_executables: false,
        linker_flavor: LinkerFlavor::Gnu(Cc::No, Lld::Yes),
        linker: Some("rust-lld".into()),
        main_needs_argc_argv: true,
        // Раскрутки стека нет (panic = гибель процесса, ядро переживает).
        panic_strategy: PanicStrategy::Abort,
        eh_frame_header: false,
        emit_debug_gdb_scripts: false,
        ..Default::default()
    }
}
