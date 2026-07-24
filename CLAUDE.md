# strand-braid (fork notes)

This is `Mharrap/strand-braid`, a personal fork. Don't push to
`strawlab/strand-braid` (upstream) or open a PR there without the user's
explicit go-ahead — only this fork's `origin` is safe to land commits on
for now.

## media-utils/tutorial-video-simulation/

WIP tooling that regenerates strand-braid's tutorial videos by driving the
real `strand-cam`/`braid-run` binaries and a real browser end-to-end,
instead of hand re-recording. If you're picking this up (including on a
different machine), read
`media-utils/tutorial-video-simulation/ONBOARDING.md` first — it has the
current status, hardware caveats, and conventions learned so far. Its own
`README.md` covers the full architecture/prerequisites.
