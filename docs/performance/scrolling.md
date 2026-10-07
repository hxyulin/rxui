# Scrolling through the rendering and accessibility paths

Measured on 2026-10-07, Apple M3 Pro/macOS 27.0.1, Rust 1.98.1. The baseline is
`a37a1f9`; the candidate adds conservative own-ink paint culling and parent-relative
AccessKit transforms. [Metadata](scrolling/metadata.json) records source hashes,
the pinned Astrelis revision, features, fixture and limitations.

The earlier [interaction measurements](interaction.md) measured geometry and mock
text preparation. They did not exercise real text recording, queue submission,
native presentation or native accessibility publication. Those results could not
establish whether the workspace example scrolls smoothly.

## Workload and method

The [one-off fixture](scrolling/offscreen.rs) copies the workspace example's view:
160 file buttons, 120 output labels, nested split panes, header and draggable card.
It prepares Source Sans 3 at scale 2 into a 2080×1440 framebuffer, for a
1040×720 logical viewport. Each iteration alternates file-list motion by 10
logical units, prepares the UI/resources, records, submits, waits for completion,
then builds an AccessKit update. It checks that scrolling performs no view
evaluation, measurement or layout, and uploads no new glyph geometry or atlas data.

There are three runs per configuration, 180 frames per run, with the first 20
frames excluded. Tables show the range of individual-frame medians across runs.
The desktop remained interactive; there was no affinity or isolated machine load.
Final CSV runs did not overlap builds or tests. Startup costs are excluded.

The framebuffer path does **not** acquire or present a native surface. Submission
time includes CPU driver work; submit-and-wait uses serial frames and is not a GPU
timestamp measurement. It does not establish native FPS or a worst-case input
latency budget. The separate native checks below cover accessibility publication.

Reproduce against the current checkout without adding test modes to an example:

```sh
python3 docs/performance/scrolling/reproduce.py --output /tmp/rxui-scroll-debug
python3 docs/performance/scrolling/reproduce.py --release --output /tmp/rxui-scroll-release
```

The runner creates a temporary Cargo project, substitutes source/font paths,
copies the checkout's lockfile, and uses the canonical Git dependency. It compiles
offline against the local Cargo cache and writes three CSVs. The baseline can be
reproduced by copying this fixture/runner into a checkout of `a37a1f9`.

## Paint cost

Previously the painter tested the inherited clip, which remained nonempty for
every row in a scroll container. It therefore recorded shapes and glyph draws even
when the row itself was far outside the viewport. The GPU clipped those draws,
but CPU recording and driver submission still processed their commands.

The candidate tests each element's own visual content against its clip and the
caller's scissor. Shapes include one destination pixel of analytic fringe; text
uses prepared glyph-quad bounds rather than its layout box. Images and editor
selection/caret use their appropriate content clips. Descendants are considered
independently, preserving visible overflow and absolute children of offscreen
parents. GPU resources remain prepared and retained; this is paint culling, not
list virtualization or lazy text preparation.

| Debug stage | Baseline median range (ms) | Candidate median range (ms) |
| --- | ---: | ---: |
| Geometry/routing | 0.259–0.315 | 0.231–0.246 |
| UI/resource preparation | 1.266–1.371 | 1.258–1.291 |
| Validation/recording | 1.852–2.016 | 1.334–1.373 |
| Queue submission | 7.152–8.029 | 1.512–1.589 |
| Combined CPU path through submission | 10.511–11.794 | 4.336–4.511 |
| Completion wait | 1.358–1.551 | 1.297–1.333 |

| Release stage | Baseline median range (ms) | Candidate median range (ms) |
| --- | ---: | ---: |
| Geometry/routing | 0.020–0.025 | 0.022–0.024 |
| UI/resource preparation | 0.139–0.147 | 0.147–0.151 |
| Validation/recording | 0.191–0.202 | 0.156–0.159 |
| Queue submission | 0.116–0.143 | 0.085–0.108 |
| Combined CPU path through submission | 0.468–0.527 | 0.412–0.443 |
| Completion wait | 0.531–1.160 | 0.993–1.001 |

Text draw calls fall from **291 to 28 per frame**. The dominant debug submission
cost falls by about 78–81% across these median ranges. Release was already much
cheaper; the reduction in CPU work is smaller. The completion-wait results do not
support a GPU speedup claim: release wait time increased in some comparisons and
varied widely in the baseline. Treat these as CPU diagnosis and work-count
evidence, not a claim that overall native frame latency improved by the same ratio.

## Accessibility publication

Window-coordinate bounds caused every descendant in a scrolling list to receive
a changed position. The candidate stores local bounds with parent-relative
transforms. Positions come directly from Taffy layout minus the immediate parent's
scroll offset, avoiding floating-point cancellation when an accumulated window
origin changes. AccessKit consumers still see transformed physical window bounds.
Offscreen elements remain available for assistive navigation and reveal.

In the headless fixture, each scroll update falls from **163 to 4 changed nodes**.
Snapshot construction still walks the tree: debug medians are 2.676–2.949 ms before
and 2.684–2.788 ms after; release is 0.288–0.304 ms before and 0.301–0.316 ms after.
This change targets publication volume, not snapshot-building complexity.

Native checks used computer use with a normal, non-topmost window and active
accessibility. One scroll call moved each pane by 200 logical units. The before
trace already includes paint culling, isolating the transform/publication change.

| Pane | Changed nodes before → after | Adapter publication before → after |
| --- | ---: | ---: |
| Files | 163 → 5 | 11.764 ms → 0.626 ms |
| Output | 123 → 3 | 11.021 ms → 0.695 ms |

These are two diagnostic samples, not latency percentiles. Background desktop
scheduling affected native snapshot-building times: candidate build durations
were 5.481 and 10.118 ms in those calls. The whole input handler was consequently
7.491 and 13.057 ms; reduced publication alone does not prove a native frame-rate
budget. Both independent offsets were verified through the native accessibility
tree. The temporary window was closed after testing.

## Validation and remaining work

The AccessKit consumer test scrolls 1,000 rows with fractional two-axis deltas,
checks bounded publication counts, and compares every transformed row with UI
window geometry, including a DPI change. The GPU culling test checks visible
children of offscreen parents, glyph overflow from a zero-height label, fractional
shape edges, caller scissor restoration, newly visible rows and unchanged resource
work. The existing image, text, editing, MSAA and isolated-opacity readbacks pass.

Preparation, paint validation and semantic construction still scan retained nodes;
offscreen text is still shaped/rasterized during initial preparation. The next
performance work should measure and reduce repeated resource/semantic scans and
coalesce native semantic publication with frames where appropriate. Large-list
virtualization would additionally reduce retained nodes, startup text work and
memory, but it is unnecessary to explain these small-list debug bottlenecks.

Raw CSVs are in [baseline debug](scrolling/base-debug/run-1.csv),
[candidate debug](scrolling/fixed-debug/run-1.csv),
[baseline release](scrolling/base-release/run-1.csv) and
[candidate release](scrolling/fixed-release/run-1.csv), with three files in each
directory. Native traces are recorded
[before transforms](scrolling/rxui-scroll-native-fixed-debug.log) and
[after transforms](scrolling/rxui-scroll-native-relative-debug.log).
