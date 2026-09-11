Place local persistent SSH host keys here for ISO embedding.

You can also point the build at a different local directory with:
- env: `YGG_SSH_HOST_KEYS_DIR=/path/to/ssh-host-keys`
- toml: `ssh_host_keys_dir = "/path/to/ssh-host-keys"`

Expected files (local, not committed):
- ssh_host_rsa_key
- ssh_host_rsa_key.pub
- ssh_host_ed25519_key
- ssh_host_ed25519_key.pub
- ssh_host_ecdsa_key
- ssh_host_ecdsa_key.pub
- hostid
- authorized_keys (optional)
