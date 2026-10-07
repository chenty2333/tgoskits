# ACPICA AML 解释器

本软件包将 ACPICA 20260930 作为独立 AML 解释器构建，并提供 Rust OSL 边界。默认使用 `no_std`，当前支持 TGOSKits 的 x86_64 LP64 ABI。`vendor/` 中 ACPICA 源码保持原样；构建脚本在生成目录中选择 TGOSKits 平台头文件，不修改上游源码。

## 所有权与集成

调用方为每个内核实例提供一个 `Backend`。后端负责 ACPI 根表定位、内存映射、端口及 PCI 访问、中断注册与屏蔽、计时与休眠、延后工作和日志。必须在解释器初始化前安装，并在解释器生命周期内保持有效。ACPICA 使用进程级全局状态，因此同一时刻只能有一个 `Engine`。本软件包不负责设备发现、寄存器映射、平台策略，也不会默认启动解释器。

调用链为 `BackendRegistration` → `Engine::initialize`（或在 AML 加载前需要安装操作区处理器时使用 `initialize_with_tables`）→ 通过解释器返回的 Rust 自有值和资源。Rust 经 C ABI 桥接调用 ACPICA；ACPICA 的 OSL 回调再进入调用方后端。全局生命周期状态阻止并发创建解释器；子系统初始化后的失败会先静默并终止 ACPICA；`Drop` 会移除通知处理器并终止子系统。若 `AcpiInitializeSubsystem` 本身失败，则关闭全局初始化入口，因为 ACPICA 全局状态可能只完成了部分初始化，不能安全地再次初始化。

`Backend` 是 `unsafe` 实现边界：实现方必须提供有效且对齐的映射、同步中断移除、延后并排空工作、可嵌套的中断屏蔽，以及稳定的线程标识和单调计时值。内核设施的所有权仍属于调用方。在 `Mode::Hardware` 下，AML 可以执行固件描述的 MMIO、端口和 PCI 操作；调用方必须限制可访问范围，并仅在运行时服务就绪后调用 `unsafe` 初始化接口。

EC 操作区同样由调用方负责：调用方可以使用 `Engine::install_ec_handler` 注册命名空间处理器，并实现 `Backend::ec_access`。本 crate 的 EC 模块只提供可移植的 ECDT 解析和有界字节协议，不执行端口 I/O，也不负责控制器发现。

`Mode::Offline` 仅适用于能模拟全部硬件读写的后端，不能替代硬件验收。可选的 `host-test` 功能提供离线后端，用于测试本 crate 编写的合成 AML；它不会映射宿主物理内存或访问宿主 I/O 端口。仓库中的合成测试通过公开 API 调用真实 Rust/C 解释器，覆盖 AML 求值、命名空间遍历、表输出边界和延后通知。

## 范围

本软件包补足 AML 执行能力，不替换 `someboot` 的 ACPI 表解析或现有 PCI 发现路径。复用现有表解析器，避免建立第二套表所有权和发现链路。本软件包不提供内核原生服务、不携带 OEM 表，也不改变任何平台的 ACPI 默认路径。调用方需显式接入后端，并决定何时、如何初始化解释器。睡眠、EC、GPE、SCI 和通知仍需要后端/运行时接线与目标平台验收。宿主合成测试只能证明真实解释器对合成输入的行为，不能证明固件、中断、MMIO、电源管理或板级行为。

这是一个高风险、需显式启用的解释器边界，因为 AML 具有平台级权限。本设计复用 ACPICA，而不自行实现 AML 语义；所有操作系统服务均通过调用方后端提供；Rust/C 桥只传递自有值和有界字节缓冲区。仅扩展静态表解析无法执行 AML 方法；把解释器集成进 `someboot` 或更改平台默认启动会把平台策略混入可复用机制，因此暂不纳入本软件包。没有迁移或回滚步骤：本变更不修改现有调用方，移除本 crate 后静态 ACPI 解析和 PCI 发现仍保持不变。

## 验证边界

项目 host-test 配置会构建并执行 `tests/synthetic_aml.rs`。测试 AML 由本仓库编写，不加载抓取的固件。`cargo xtask clippy --package acpica-interpreter` 检查生产库和构建配置。

`test-suit/arceos/drivers/acpica-interpreter` 通过真实 ArceOS 客体运行生产 Rust/C 解释器。用例使用客体分配器持有合成表，验证 `_INI` 仅执行一次、方法与资源求值、延后 Notify 任务和析构排空；它不启用 `std-compat`，也不替换平台现有 ACPI 路径。运行：

```sh
cargo xtask arceos test qemu --test-group drivers --test-case acpica-interpreter --target x86_64-unknown-none
```

该 QEMU 用例使用 `Mode::Offline`，不是硬件 ACPI 验收。真实固件、EC、SCI/GPE、睡眠、电源管理及实体硬件工作先搁置。

## 许可证

ACPICA 上游源码采用 BSD-3-Clause，见 `LICENSE.BSD-3-Clause` 和 `NOTICE`。本 crate 的 Rust 代码及适配层采用 Apache-2.0，见 `LICENSE.APACHE-2.0`；软件包许可证为 `Apache-2.0 AND BSD-3-Clause`。
