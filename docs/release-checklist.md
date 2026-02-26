# Release Checklist

1. Run `./mkconfig.sh --profile both`
2. Confirm both ISO artifacts are produced
3. Run `./tests/smoke/run.sh --profile both --require-artifacts --with-iso-rootfs`
4. Optional on KVM-capable host: `./tests/smoke/run.sh --profile both --require-artifacts --with-qemu-boot`
5. Boot-test both artifacts and validate runtime services
6. Publish only after both profiles pass
