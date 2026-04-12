FROM debian:sid-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    coreutils \
    curl \
    dosfstools \
    findutils \
    gawk \
    git \
    gnupg \
    grub-efi-amd64-bin \
    grub-pc-bin \
    isolinux \
    iproute2 \
    live-build \
    mtools \
    openssh-client \
    ovmf \
    qemu-system-x86 \
    qemu-utils \
    python3 \
    ripgrep \
    rsync \
    sed \
    shim-signed \
    squashfs-tools \
    syslinux-common \
    syslinux-utils \
    tar \
    xz-utils \
    xorriso \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace/repo

COPY . /workspace/repo

RUN chmod +x /workspace/repo/scripts/maker-build-container-entrypoint.sh

ENTRYPOINT ["bash", "/workspace/repo/scripts/maker-build-container-entrypoint.sh"]
