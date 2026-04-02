# Intel Arc SR-IOV live-host build

This document describes the opt-in `yggdrasil` path for building a live host that can create Intel Arc SR-IOV virtual functions for KVM guests.

This is a host-side workflow.
It does not guarantee that a particular guest OS will load a graphics driver for the VF.

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
