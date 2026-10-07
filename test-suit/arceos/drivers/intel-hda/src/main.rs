#![feature(used_with_arg)]
#![no_std]
#![no_main]

extern crate alloc;
extern crate ax_std as std;

use alloc::{format, vec, vec::Vec};
use core::{
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use ax_driver::{
    model_register,
    probe::{OnProbeError, pci::ProbePci},
    register::{ProbeKind, ProbeLevel, ProbePriority},
};
use dma_api::{DmaCoherency, DmaConstraints, DmaDeviceInfo, DmaDomainId};
use intel_hda::{Clock, Controller, MappedHdaIo, identify};
use pcie::CommandRegister;

const PERIOD_BYTES: usize = 4096;
const PERIOD_COUNT: usize = 4;
const PLAYBACK_TIMEOUT_NS: u64 = 3_000_000_000;
static TEST_STATUS: AtomicU8 = AtomicU8::new(0);

struct QemuClock;

impl Clock for QemuClock {
    fn delay_us(&mut self, micros: u32) {
        ax_hal::time::busy_wait(Duration::from_micros(u64::from(micros)));
    }

    fn now_ns(&self) -> u64 {
        ax_hal::time::monotonic_time_nanos()
    }
}

model_register!(
    name: "QEMU Intel HDA integration test",
    level: ProbeLevel::PostKernel,
    priority: ProbePriority::DEFAULT,
    probe_kinds: &[ProbeKind::Pci { on_probe: probe_hda }],
);

fn probe_hda(mut probe: ProbePci<'_>) -> Result<(), OnProbeError> {
    let endpoint = probe.endpoint();
    let class = endpoint.revision_and_class();
    let Some(device) = identify(
        endpoint.vendor_id(),
        endpoint.device_id(),
        class.base_class,
        class.sub_class,
        class.interface,
    ) else {
        return Err(OnProbeError::NotMatch);
    };
    if probe.info().iommu.is_some() {
        return Err(OnProbeError::other(
            "QEMU Intel HDA case expects a direct DMA domain",
        ));
    }

    let bar = endpoint
        .bar_mmio(0)
        .ok_or_else(|| OnProbeError::other("Intel HDA BAR0 is not a memory BAR"))?;
    let bar_size = bar.end.saturating_sub(bar.start);
    if bar_size == 0 {
        return Err(OnProbeError::other("Intel HDA BAR0 has zero size"));
    }

    probe.endpoint_mut().update_command(|mut command| {
        command.insert(CommandRegister::MEMORY_ENABLE | CommandRegister::BUS_MASTER_ENABLE);
        command
    });

    let mmio = axklib::mmio::ioremap(bar.start.into(), bar_size)
        .map_err(|error| OnProbeError::other(format!("mapping Intel HDA BAR0 failed: {error}")))?;
    let coherency = if probe.info().dma_coherent {
        DmaCoherency::Coherent
    } else {
        DmaCoherency::NonCoherent
    };
    let dma = ax_driver::dma_device_for_info(DmaDeviceInfo::new(
        DmaDomainId::Direct,
        coherency,
        DmaConstraints::new(u64::MAX),
    ))
    .map_err(|error| OnProbeError::other(format!("creating HDA DMA capability failed: {error}")))?;

    let bus = MappedHdaIo::new(mmio, QemuClock);
    let mut controller = Controller::new(bus, device, dma).map_err(|error| {
        OnProbeError::other(format!("constructing QEMU HDA controller failed: {error}"))
    })?;
    if controller
        .route()
        .path
        .last()
        .is_none_or(|converter| (converter.caps >> 20) & 0x0f != 0)
    {
        return Err(OnProbeError::other(
            "QEMU HDA codec did not expose an analog output converter",
        ));
    }

    run_pcm_lifecycle(&mut controller)
        .map_err(|error| OnProbeError::other(format!("QEMU HDA playback failed: {error}")))?;
    TEST_STATUS.store(1, Ordering::Release);
    Ok(())
}

fn run_pcm_lifecycle(
    controller: &mut Controller<MappedHdaIo<QemuClock>>,
) -> Result<(), &'static str> {
    controller
        .prepare(PERIOD_BYTES as u32, PERIOD_COUNT as u32)
        .map_err(|_| "preparing the PCM stream failed")?;

    let mut pcm = vec![0; PERIOD_BYTES];
    pcm.fill(0x20);

    let deadline = ax_hal::time::monotonic_time_nanos().saturating_add(PLAYBACK_TIMEOUT_NS);
    let mut submitted = Vec::with_capacity(PERIOD_COUNT);
    let mut completed = Vec::with_capacity(PERIOD_COUNT);
    while submitted.len() < PERIOD_COUNT {
        match controller.submit(&pcm) {
            Ok(token) => submitted.push(token),
            Err(intel_hda::Error::Again) => poll_completion(controller, &mut completed, deadline)?,
            Err(_) => return Err("submitting a PCM period failed"),
        }
    }

    while completed.len() < PERIOD_COUNT {
        poll_completion(controller, &mut completed, deadline)?;
    }
    if completed != submitted {
        return Err("PCM completion tokens were not retired in submission order");
    }

    controller
        .release()
        .map_err(|_| "releasing the drained PCM stream failed")?;
    controller
        .shutdown()
        .map_err(|_| "shutting down the HDA controller failed")?;
    Ok(())
}

fn poll_completion(
    controller: &mut Controller<MappedHdaIo<QemuClock>>,
    completed: &mut Vec<u16>,
    deadline: u64,
) -> Result<(), &'static str> {
    match controller
        .complete()
        .map_err(|_| "polling the HDA stream failed")?
    {
        Some(token) => completed.push(token),
        None if ax_hal::time::monotonic_time_nanos() < deadline => {
            ax_hal::time::busy_wait(Duration::from_micros(100));
        }
        None => return Err("timed out waiting for HDA playback completion"),
    }
    Ok(())
}

#[unsafe(no_mangle)]
fn main() {
    assert_eq!(
        TEST_STATUS.load(Ordering::Acquire),
        1,
        "QEMU Intel HDA PCI probe and playback integration did not complete"
    );
    std::println!("INTEL_HDA_QEMU_OK");
    std::process::exit(0);
}
