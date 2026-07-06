# personal-emr

A small Rust web app for keeping the George family's medical history in one place. Incidents, records (X-rays, labs, notes, prescriptions), and the source each came from. Deployed to kant at `emr.roshangeorge.dev`, behind Cloudflare Access.

See `CLAUDE.md` for conventions and the runtime contract.

## Local dev

```sh
docker run --rm -d --name pemr-pg -p 5433:5432 -e POSTGRES_PASSWORD=postgres postgres:18

export DATABASE_URL=postgresql://postgres:postgres@127.0.0.1:5433/postgres
export FILES_DIR=/tmp/personal-emr-files
export BIND_ADDR=127.0.0.1:8080
export DEV_VIEWER_EMAIL=roshan@technologybrother.com
mkdir -p $FILES_DIR

cargo run
```

## Deploy

```sh
mise run css            # regenerate the Tailwind bundle
git commit -am '...'    # skybuild builds committed HEAD
export SKYBUILD_TOKEN=$(kubectl --context tb-0-0 -n skybuild get secret skybuild-secrets -o jsonpath='{.data.auth-token}' | base64 -d)
mise run deploy
```

`mise run deploy` builds the amd64 image on **skybuild** (the tb-0-0 build service — see `skybuild.toml`), which pushes it to the kant registry; then it scps the quadlet + backup units and restarts the service on kant. It refuses to build a dirty tree, since skybuild builds committed HEAD, not your working copy — so commit any regenerated CSS first.

Cloudflare hostname + Access policy are configured manually in the Zero Trust dashboard the first time.
