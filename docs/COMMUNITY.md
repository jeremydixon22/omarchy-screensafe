# Community packs

Packs made by others are listed in ScreenSafe's catalog, and installed by copying a folder into
`~/.config/omarchy/screensafe/packs/`, as any pack is. This is how the catalog works; its own README says whether it's
taking listings.

## Shape

There is no site and no gallery server. The catalog is a repository of its own,
[`jeremydixon22/omarchy-screensafe-packs`](https://github.com/jeremydixon22/omarchy-screensafe-packs), which the
panel's **Find a screensaver** opens. It's kept apart from this one so that this repository holds only the plugin,
exactly as submitted for review: listing a pack never changes the plugin, and never needs the plugin reviewed again.

- **Packs stay in their makers' repositories.** A pack is a folder in a public git repository its maker controls: a
  repository of its own, a theme's `screensafe/` folder, or a folder among others.
- **The catalog lists them.** Its `packs.toml` holds one entry per pack: where it is, and the exact commit approved.
  Its `README.md` is generated from that: a page of previews, descriptions and
  install steps, which GitHub shows on the repository's front page.
- **The plugin never fetches anything.** It doesn't touch the network, run git or install packs. Someone who wants
  a listed pack copies it into their packs folder, as they would any other, and the renderer checks it like any
  local pack.

So approving a pack means approving one commit of someone else's repository. Nothing the maker pushes later is
listed until it's reviewed in turn.

## An entry

```toml
[[pack]]
id = "aquarium"                       # unique here; also the folder name it installs as
name = "Aquarium"
author = "Jane Doe"
contact = "https://github.com/janedoe"
repository = "https://github.com/janedoe/omarchy-aquarium"
path = "."                            # the pack's folder within the repository
commit = "4f1c…"                      # the full 40-character commit approved
license = "CC-BY-4.0"                 # CC0-1.0, CC-BY-4.0 or MIT
preview = "preview.png"               # within the pack's folder, at that commit
format = 1
added = "2026-11-02"
provenance = """
All art drawn by the author in Aseprite. No generated material, no photographs, no likenesses.
"""
```

## Submitting

One pull request per pack, to the catalog. It changes only `packs.toml`, adding or updating that pack's entry. The pull
request template asks for:

- a link to the pack's repository at the commit to approve;
- the license, and where every image, font, sound or likeness comes from, including generated material (summarized
  into `provenance`);
- confirmation that the submitter made the pack or has the right to share it, and agrees to it being listed.

The pack's own repository must:

- be public, and reachable with plain `git clone` over HTTPS, without submodules or Git LFS;
- hold, at that commit, the pack folder with its `screensafe.toml`, a `preview.png` rendered by `screensafe render`,
  and a `LICENSE` covering the pack (in the folder or at the repository's root).

The agent skill (`skills/screensafe-packs`) walks a maker through this, and always shows them the pull request text
before anything is submitted on their behalf.

## Checks on every pull request

Automated and deterministic, never decided by a model. A workflow runs on `pull_request` (never
`pull_request_target`), with read-only permissions and no secrets. For each entry that changed, it:

1. **Validates the entry:**
   - its fields;
   - that its id is unique;
   - that the commit is a full SHA;
   - that the license is one of the three allowed.
2. **Fetches the pack:** that one commit of the repository, shallowly, without submodules or LFS, into an empty
   folder. Any check fails if the commit can't be fetched.
3. **Checks the files:**
   - the path exists;
   - only allowed file types, identified by their contents, not their names;
   - no links, hidden files or executables;
   - total size within the pack limit.
4. **Runs `screensafe check`** with the binaries of a pinned ScreenSafe release, fetched by its tag and checked
   against their published digests. This applies every format rule and
   limit, and the flash check over the whole loop at 960×540 and 2304×1440.
5. **Renders a contact sheet** of frames across the loop, attached to the run for reviewers. The pack's own preview
   is shown beside it, so a misleading preview stands out.
6. **For an update:** lists the files that changed between the approved commit and the new one.

## Human review

The maintainer approves each entry, checking:

- **rights:**
  - the license;
  - the provenance;
  - fonts;
  - trademarks;
  - likenesses of real people, which need their consent; parody of public figures is decided case by case;
- **content:** nothing hateful, sexual, gory or shocking. A screensaver comes up unbidden on screens in offices and
  shared rooms;
- **impersonation:** nothing that imitates a lock screen, a login or password prompt, or a system dialog, and no
  instructions to run commands. A screensaver appears on its own, and someone returning to their screen might type a
  password into what looks like a lock screen;
- **photosensitivity:** a second look at anything near the flash limit, even though it passes;
- **the preview:** that it shows what the pack really looks like.

Approval is of the exact commit in the entry.

## Keeping the list honest

- **On a schedule** (weekly), a workflow:
  - fetches every listed pack at its approved commit;
  - checks it with the current renderer.

  A repository that has gone, been rewritten or fails the check is flagged in an issue. If it isn't fixed within a
  set time, it's delisted.
- **On every renderer change**, the same check of every listed pack runs before release. A format change that would
  break a listed pack is either avoided, or noted for its maker in good time.
- **Delisting** is a pull request that moves the entry into a `[[delisted]]` table with a date and a reason, so the
  record stays. A maker's own request to delist is honored without question. A rights or safety problem is delisted
  first and discussed after.

## Installing

The catalog's `README.md` gives each pack its own steps, copied from its entry:

```sh
git clone --filter=blob:none --no-checkout https://github.com/janedoe/omarchy-aquarium /tmp/aquarium
git -C /tmp/aquarium checkout 4f1c…
cp -r /tmp/aquarium/. ~/.config/omarchy/screensafe/packs/aquarium
```

Checking out the approved commit gives exactly the files that were reviewed. The pack appears in the panel the next
time it's opened, and the renderer checks it as it does any pack.

## What the plugin doesn't do, and why

- **Fetching from the plugin.** The plugin would need network access and a process that runs git or an HTTP
  client. That would put it in a different security class. The marketplace's review would treat it as an
  installer, and the rule that nothing reaches the network would no longer hold.
- **An install helper script in the plugin.** Everything in this repository ships with the plugin, so a helper
  would raise the same review questions as fetching from the plugin. The copyable steps above do the same job in
  plain sight.
- **Mirroring packs in the catalog.** That would make the catalog the host and distributor of other people's art,
  with the rights questions and takedowns that brings. Listing a commit leaves the art with its maker.
