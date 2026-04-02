# Intel Arc SR-IOV live-host build

This document describes the opt-in `yggdrasil` path for building a live host that can create Intel Arc SR-IOV virtual functions for KVM guests.

This is a host-side workflow.
It does not guarantee that a particular guest OS will load a graphics driver for the VF.

`yggdrasil` defaults to the stock in-kernel `i915` driver.
Only enable this document's path if you are deliberately testing the out-of-tree `i915-sriov-dkms` branch for Intel GPU virtualization experiments.

## What this build does

When enabled, the build pipeline:

- appends the required kernel arguments for Intel Arc SR-IOV
- installs the pinned out-of-tree `i915-sriov-dkms` package into the live image
- provisions Intel Arc VFs at boot through a dedicated systemd service
- optionally binds those VFs to `vfio-pci` for guest assignment

## Local config

Set these values in your local config file:

```toml
enable_intel_arc_sriov = true
intel_arc_sriov_release = "2026.03.05"
intel_arc_sriov_vf_count = 7
intel_arc_sriov_pf_pci = ""
intel_arc_sriov_device_id = "0x56a0"
intel_arc_sriov_bind_vfs = "vfio-pci"
```

Env-file equivalent:

```bash
YGG_ENABLE_INTEL_ARC_SRIOV="true"
YGG_INTEL_ARC_SRIOV_RELEASE="2026.03.05"
YGG_INTEL_ARC_SRIOV_VF_COUNT="7"
YGG_INTEL_ARC_SRIOV_PF_PCI=""
YGG_INTEL_ARC_SRIOV_DEVICE_ID="0x56a0"
YGG_INTEL_ARC_SRIOV_BIND_VFS="vfio-pci"
```

Notes:

- leave `intel_arc_sriov_pf_pci` empty to let the boot service autodiscover the PF
- set it explicitly if you want to pin the host to a known Arc PF such as `0000:83:00.0`
- `intel_arc_sriov_device_id` defaults to `0x56a0`, which matches Intel Arc A770
- `intel_arc_sriov_bind_vfs = "vfio-pci"` is the right default for KVM hosts

## Boot arguments

When SR-IOV is enabled, the build appends:

```text
intel_iommu=on iommu=pt i915.max_vfs=<vf-count> module_blacklist=xe
```

The base build already appends:

```text
i915.enable_guc=3
```

## Boot-time provisioning

The image installs and enables:

- `ygg-intel-arc-sriov.service`
- `/usr/local/sbin/ygg-intel-arc-sriov`
- `/etc/default/ygg-intel-arc-sriov`

The service:

1. finds the Intel Arc PF dynamically unless one is explicitly configured
2. resets `sriov_numvfs`
3. creates the requested VF count
4. binds VFs to `vfio-pci` if configured

## BIOS prerequisites

Before you blame Linux, verify the platform is configured to expose the PF correctly:

- `Above 4G Decoding = Enabled`
- `Re-Size BAR Support = Enabled`
- `SR-IOV Support = Enabled`
- `IOMMU = Enabled`

If the GPU still comes up without `sriov_numvfs`, the image will now log the live PCIe state it sees, including:

- `LnkSta`
- `BAR 2`
- `Physical Resizable BAR`
- `Single Root I/O Virtualization`

That is the fastest way to separate:

- Linux/service bugs
- BIOS prerequisites not actually taking effect
- a GPU/slot/platform path that still is not exposing SR-IOV

## Validation after boot

Check the PF and VF state:

```bash
lspci -nnk | grep -A3 -E 'VGA|Display|Intel'
systemctl status ygg-intel-arc-sriov.service
journalctl -u ygg-intel-arc-sriov.service -b
```

If you know the PF address, verify VF creation directly:

```bash
cat /sys/bus/pci/devices/0000:83:00.0/sriov_numvfs
ls -l /sys/bus/pci/devices/0000:83:00.0/virtfn*
```

If the service fails with:

```text
PF 0000:83:00.0 does not expose sriov_numvfs
```

then check:

```bash
lspci -s 83:00.0 -vv | egrep 'LnkSta|BAR 2|Resizable BAR|SR-IOV'
```

Interpretation:

- `Physical Resizable BAR` active is necessary but not sufficient
- missing `SR-IOV` capability means Linux cannot create VFs for that PF
- a degraded PCIe link such as `Width x1` is a hardware-path smell and should be fixed before deeper guest work

If binding to `vfio-pci` is enabled, verify:

```bash
lspci -nnk -d 8086:56a0
lspci -nnk | grep -A3 vfio-pci
```

## Practical caveat

This path assumes the host-side Intel Arc SR-IOV DKMS branch works on your kernel.
That is a host prerequisite only.

Guest support is a separate question.
For macOS guests especially, you should treat the host SR-IOV build as the first milestone, not the final graphics guarantee.

Also note:

- Intel does not officially support `Arc A-Series` SR-IOV
- the `i915-sriov-dkms` route is an out-of-tree community path
- success on one board/slot/firmware combination does not imply success on another
