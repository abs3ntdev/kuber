# kuber

Ephemeral kubeconfig manager for DigitalOcean Kubernetes clusters. Wraps [kubie](https://github.com/kubie-org/kubie) with on-demand config fetching via [doctl](https://github.com/digitalocean/doctl) -- no kubeconfig files stored on disk permanently.

## How it works

1. **Instant context list** -- cached cluster metadata is shown immediately via `fzf`
2. **Live background sync** -- while you browse, doctl discovers new clusters and streams them into the picker in real time
3. **On-demand fetch** -- only the selected cluster's kubeconfig is downloaded
4. **Ephemeral storage** -- configs live in tmpfs (`/dev/shm`) and are deleted when the kubie shell exits
5. **No global state mutation** -- all doctl calls use `--context` flags, never `doctl auth switch`

## Requirements

- [doctl](https://github.com/digitalocean/doctl) with authenticated contexts
- [kubie](https://github.com/kubie-org/kubie)
- [fzf](https://github.com/junegunn/fzf)

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
# Interactive context picker (fzf)
kuber

# Direct context selection
kuber do-sfo3-my-cluster
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `KUBER_CACHE_DIR` | `/dev/shm/kuber-<uid>` | Directory for metadata cache and ephemeral kubeconfigs |

## License

MIT
