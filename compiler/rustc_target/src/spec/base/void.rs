use crate::spec::{Cc, LinkerFlavor, Lld, Os, PanicStrategy, RelocModel, TargetOptions, TlsModel};

/// VOID: микроядро с персистентным контент-адресуемым объектным store
/// (https://github.com/voidfox — Code/kernel). Userspace-программы — статические
/// ELF, загружаемые ядром по content-id; линкуются на USER_REGION_START
/// (0x4000_0000) скриптом линкера на стороне программы.
pub(crate) fn opts() -> TargetOptions {
    TargetOptions {
        os: Os::Void,
        executables: true,
        // Веха 35 — потоки: `#[thread_local]` через нативный TLS (модель local-exec —
        // TPREL-смещения резолвит линкер, динамических релокаций нет, ELF-загрузчику
        // TLS не касается). Рантайм (pal/void/tls.rs) строит per-thread TLS-блок и ставит
        // указатель нити: `tp` на riscv (Variant I), база `%fs` на x86 (Variant II).
        has_thread_local: true,
        // Явно local-exec: программы статические no-PIE, все thread_local — локальные,
        // GD/IE-релокации (GOT, __tls_get_addr) не нужны и загрузчиком не обрабатываются.
        tls_model: TlsModel::LocalExec,
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
