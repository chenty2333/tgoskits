# Venus 与 VirGL 资源回收回归

在具有可读写 `/dev/dri/renderD128`、KVM 及支持 Venus 的 QEMU/virglrenderer 的
x86_64 主机上运行：

```sh
cargo xtask starry app qemu -t venus-regression --arch x86_64 --cap virgl
```

这是有界自动回归，不启动桌面或无限运行。客户机只选择 Mesa virtio Vulkan ICD，
不允许切换到客户机内的软件 Vulkan ICD；同一 VirGL fd 反复创建、映射和关闭 GEM，
并验证最后一个 handle
关闭后 VMA 仍有效。随后在同一 Vulkan device 上反复分配 host-visible 内存，提交
真实 `vkCmdFillBuffer`，等待 fence 并逐字回读，再释放资源和映射。失败必须传递到
最外层运行器。普通不具备主机 GPU 的 CI 不声明 `virgl` 能力，不执行此应用。

宿主可通过标准的 `VK_DRIVER_FILES` 显式选择 Lavapipe 后端。此时客户机仍运行
Venus，设备名可能包含 `Virtio-GPU Venus (llvmpipe ...)`；这是宿主软件实现，不是
客户机绕过 VirtIO 的回退，也不能作为物理 GPU 已通过验证的证据。回归不关闭 Mesa
的 fence-feedback 路径。

这些结果证明 QEMU 和当前主机渲染栈上的命令、映射与回收路径，不代表其他物理
GPU 或显示器的扫描输出已验证。
