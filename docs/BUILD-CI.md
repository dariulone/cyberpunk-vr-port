# Building and releasing

The package a tester or a player installs always comes out of a CI run. Build locally to iterate;
`scripts\deploy_stereo.ps1` drops the result straight into the game.

Why each piece works the way it does is in the comments in `.github/workflows/` and
`scripts/build_dist.ps1`, next to the code it explains.

## Versioning

`VERSION` at the repo root holds a bare `X.Y.Z`: the number the **next** release will carry.
`scripts/ci_version.sh` derives everything else from it and is the only thing that decides a
version. Tags are bare — no `v` prefix.

| Where the commit lands | Version                | What happens                          |
| ---------------------- | ---------------------- | ------------------------------------- |
| feature branch, PR     | `0.1.6-dev.a1b2c3d`    | package uploaded as a run artifact    |
| `main`                 | `0.1.6-rc.4.a1b2c3d`   | tagged + published as a **prerelease** |
| release                | `0.1.6`                | you write it, CI attaches the binaries |

`N` counts commits on `main` since `VERSION` last changed, so it restarts at `rc.1` on a bump.
Ordering falls out of SemVer: `0.1.6-dev.*` < `0.1.6-rc.*` < `0.1.6`.

Run it locally to see what a ref would produce:

```bash
VERSION_REF_NAME=main VERSION_REF_TYPE=branch bash scripts/ci_version.sh
```

## What CI builds

`.github/workflows/build.yml`, on `windows-latest`: the root CMake project in `Release`, then
`scripts/build_dist.ps1` to assemble the game-root layout.

Two artifacts per run — `CyberpunkVRPort-<version>` (the package) and
`CyberpunkVRPort-<version>-symbols` (the PDBs, never inside the player's package).

## Release candidates

Every push to `main` is tagged and published as a **prerelease** carrying both zips, so a tester
can download an rc without a GitHub account and without the 90-day artifact expiry. One
prerelease per commit on `main` is the intended cost.

## Cutting a release

> GitHub → **Releases** → Draft a new release → tag `0.1.6`, write the notes, **Publish**

Publishing fires `release.yml`. It finds the candidate built from **that same commit**, restamps
the version in `INSTALL.txt`, and attaches the rc's own assets. **Nothing is rebuilt** — what
ships is what a tester ran.

It refuses rather than guesses: a tag that is not a bare `X.Y.Z`, no rc on that commit, an asset
that is not a package, or a restamp that did not take.

Two things to know:

- **Drafts do not trigger workflows.** The assets appear a minute or two after you publish.
- **`VERSION` bumps itself.** Publishing `0.1.6` pushes `0.1.7` to `main`. It only fires when
  `VERSION` still names what was released, so moving it by hand first (to `0.2.0`, say) wins.
  That commit does not build.
