# mkconfig TUI

Run:

```bash
./scripts/mkconfig-tui.sh
```

The TUI supports two user paths:

- `recommended`: SSH keys + secure defaults
- `quick-try`: minimal setup for evaluation with reminders

Saved output is an env file consumed by `mkconfig.sh --config`.

The generated env currently supports:

- SSH authorized keys embedding toggle/path
- Hostname override
- LXC parent interface + macvlan CIDR/route
- DHCP or static host networking (IP/gateway/DNS)
