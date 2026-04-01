# kuber

Ephemeral kubeconfig manager for DigitalOcean Kubernetes clusters. Wraps [kubie](https://github.com/kubie-org/kubie) with on-demand config fetching via [doctl](https://github.com/digitalocean/doctl) -- no kubeconfig files stored on disk permanently.

## How it works

1. **Instant context list** -- cached cluster metadata is shown immediately via built-in fuzzy picker ([skim](https://github.com/skim-rs/skim))
2. **Live background sync** -- while you browse, doctl discovers new clusters and streams them into the picker in real time
3. **On-demand fetch** -- only the selected cluster's kubeconfig is downloaded
4. **Ephemeral configs** -- kubeconfigs are written to `/tmp` and handed to kubie, which copies them into its own temp storage
5. **No global state mutation** -- all doctl calls use `--context` flags, never `doctl auth switch`

## Requirements

- [doctl](https://github.com/digitalocean/doctl) with authenticated contexts
- [kubie](https://github.com/kubie-org/kubie)

## Install

```sh
make install
```

Installs to `~/.local/bin/kuber` by default. Override with `PREFIX`:

```sh
make install PREFIX=/usr/local
```

## Usage

```sh
# Interactive context picker
kuber

# Direct context selection
kuber do-sfo3-my-cluster
```

## Storage

- **Metadata** (cluster list): `$XDG_CACHE_HOME/kuber/metadata.json` (default `~/.cache/kuber/`) -- persists across reboots. Contains only cluster names, regions, versions, and node pool sizing. No credentials or API server URLs are stored.
- **Kubeconfigs**: `/tmp/kuber-<uid>/configs/` -- ephemeral, cleared on reboot

All files are created with `0600` permissions (owner-only read/write). The ephemeral configs directory is `0700`.

Set `XDG_CACHE_HOME` to control where persistent metadata is stored.

## License

MIT
