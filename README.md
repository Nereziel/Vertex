# Vertex

Vertex runs an Xbox Series X|S title natively on Windows, from files you supply.
It is a work-in-progress fork of [VIctory](https://github.com/Marvisak/VIctory)
by Marvisak; the program, its settings (`VICTORY_*`) and its crates keep that
name so that work can still be shared between the forks.

**There are no game files, code or assets here.** You need your own copy of the
game. This project is not affiliated with or endorsed by the game's developer
or publisher.

> **Not playable yet.** The title boots, loads and can be played in its world
> for tens of minutes, at about 7 to 10 frames per second on one PC, and with
> faults in the picture. Read [What works](#what-works) before you try it.

## Contents

- [Quick start](#quick-start)
- [What works](#what-works)
- [Known problems](#known-problems)
- [Make it run better](#make-it-run-better)
- [When something goes wrong](#when-something-goes-wrong)
- [Controls](#controls)
- [For developers](#for-developers)
- [Debugging](#debugging)
- [Credits and license](#credits-and-license)

## Quick start

### What you need

| | |
|---|---|
| Windows | 10 or 11, 64-bit |
| CPU | AVX2 |
| Graphics card | Direct3D 12 with shader model 6.6, resource binding tier 3 and mesh shaders; 12 GB or more is better |
| Memory | 32 GB. The title uses about 20 GB, so close other programs |
| Disk | an SSD, with room for the shader cache (several GB) |

### Getting started

1. Get the code: `git clone https://github.com/Nereziel/Vertex`
2. Double-click `Launcher.cmd`. It finds your copy's executable, checks this
   PC, installs what is missing and starts the title.

Or the same in PowerShell:

```powershell
.\start.ps1                                  # check the tools, build, check this PC
.\start.ps1 -Install                         # the same, installing what is missing
.\start.ps1 -GameExe <the title's exe>       # the same, then run your own copy
.\start.ps1 -GameExe <the exe> -Prebuild     # prepare shaders ahead of a run
```

If PowerShell refuses scripts:
`powershell -ExecutionPolicy Bypass -File start.ps1`

`victory doctor` (part of `start.ps1`) names what a PC lacks. Every FAIL line is
a reason for a black picture or a run that does not start.

### What to expect on a first run

- Shaders are translated and compiled for your card. The first load is much
  slower than later ones, and parts of the picture come in late.
- **A black screen near the start may be waiting for a button.** Press A, Enter
  or Space.
- **After a skipped cutscene the screen can stay black for several minutes**
  while the next scene loads.
- Single frames of 5 to 20 seconds happen when a scene arrives. Wait, and do not
  hold or drag the window's title bar: the title ends itself when it gets no
  frames for about 30 seconds.
- Close the title with the X or Alt+F4.

## What works

State on 2026-11-26, branch `main`, Series X profile, on one PC (RTX 5070
12 GB, Ryzen 7 8700F, 32 GB RAM, Windows 11; NVIDIA driver 621.40 for the last
run, 610.88 before). Other PCs will differ. The runs and how they were measured
are in [docs/test-runs.md](docs/test-runs.md).

| Stage | State |
|---|---|
| Boot, menus | works, 30 fps |
| Loading into the world | works |
| In the world | 6.4 to 9.9 fps median by run and scene (2,500 draws and more), best about 12. Not measured with a fixed scene |
| Cutscenes | start and can be skipped. Characters and models can come in coarse, and parts of a room can be missing for a while |
| Stability | no crash in 16 of 18 recorded sessions (25 and 26 November); the player closed them. Longest 25 min 34 s. The other two: run 18 hung, with ray tracing and several new switches on together, cause not known; run 24 lost the graphics device while a graphics driver was being installed |
| Sound | works at first; can break after a long freeze |
| Mouths | a fix for the network that animates faces is in (from fxceless/victory-fork pull request 927). On this card the old path kept 1 of 65,536 values, all now; in the game it is not confirmed yet |
| Ray-traced light | off by default. An ambient light stands in for it (`VICTORY_AMBIENT`). With it on, about 4 fps in a cutscene (run 17); the build merged since has not run with it apart from run 18 |
| Series S profile | boots; not tried further |

## Known problems

| What you see | What is known |
|---|---|
| Low frame rate | The submitting thread: about 100 of 108 ms of a usual frame in the last run, 78 of it preparing (copying memory for the card). The card's own work was 43 ms |
| Black boxes on the water in places, and a very dark sea in some shots | Open. Seen in runs 19 to 25 with different settings |
| Eyes white in some shots | Open. The same shot, when a face is lit from the front, so it looks tied to light or view angle (not proven) |
| Black clothes and objects for a moment, textures that come in late | Textures wait for room on a 12 GB card (thousands at times) |
| Coarse, blocky characters and missing walls in cutscenes | Open. Likely the title's models come in late |
| Clouds look smooth and blobby | Open |
| A new graphics driver | makes the saved pipelines invalid: the first run afterwards builds hundreds and can stutter for seconds |
| Memory | about 20 GB used; Windows can have less than 1 GB free. Close other programs |
| Square blocks or wrong light on some effects | Not all programs of the title translate for 32-lane cards. The log names them |

## Make it run better

1. **Close other programs**, browsers first. With little memory free, freezes
   get longer.
2. **Run twice.** The second start uses the shaders and pipelines of the first.
3. **Try the faster submission settings:** `.\start.ps1 -GameExe <exe> -Tuned`. The relay already enables these settings by default; the switch also supports runs with the relay disabled.
4. **Try experimental faster world streaming** after a warm-cache baseline:
   `.\start.ps1 -GameExe <exe> -FastStreaming`. This enables the existing
   texture worker, parallel texture checks, persistent physical-page views
   and parallel WriteWatch (plus `-Tuned`). Individual environment overrides
   still win; omit the switch to return to the default. The combined profile
   is not yet benchmarked and should not be treated as a proven FPS gain.
5. **Try the second-stage performance experiments** with `-PerformanceLab`
   instead of `-FastStreaming` first. It enables the switches below for
   identical mappings, both copy lanes, adaptive texture time slices,
   redundant fixed-function state calls, asynchronous submission and live
   PSO sharing. The two profiles can be combined after individual tests.
6. **Change one setting at a time**, set before the start
   (`$env:NAME = 'value'`):

| Variable | Effect |
|---|---|
| `VICTORY_TEXTURE_SHARE=33` | the share of each frame, in percent, the texture pass may take. Lower: smoother, textures come in later. `0`: no limit, as before |
| `VICTORY_TEXTURE_STAGE_MS=250` | the most that pass may take in any case, in ms (three times that with a large backlog) |
| `VICTORY_TEXTURE_BUDGET_MB=<n>` | at most this much card memory for those textures: less pressure on memory, blurrier picture |
| `VICTORY_CARD_BUFFERS_MB`, `VICTORY_CARD_WINDOWS_MB` | how much of the title's memory is copied to the card (`VICTORY_CARD_WINDOWS_MB=0`: no windows) |
| `VICTORY_HEAP_WORKER=1` | experimental texture uploads on a separate worker; needs late submission. Already implemented in Vertex; compare warm-cache runs and check texture pop-in before enabling permanently |
| `VICTORY_WIDE_TEXTURES=1` | parallel submission-time texture checks and detiling; experimental |
| `VICTORY_XMEM_PERSISTENT_VIEW=1` | reuse physical-page section mappings instead of mapping and unmapping on every write; experimental |
| `VICTORY_DSTORAGE_WORKERS=<n>` | limit the existing parallel read/decode pool to at most this many logical processors; unset uses all available processors. Try fewer workers if rendering threads are starved |
| `VICTORY_XMEM_SKIP_SAME=1` | skip remapping only when the known section and offset match on every requested page; experimental, may save nothing if mappings rarely repeat |
| `VICTORY_QUEUE2=3` | use both existing copy lanes (readback and upload); default `1` is readback only; `0` disables both |
| `VICTORY_MAP_THREADS=2` (1–4) | experimental parallel mapping of independent physical runs and nonoverlapping GPU aliases; default 1. Placeholder splits and tracking remain serial; try 2 before 4 |
| `VICTORY_TEXTURE_ADAPTIVE=1` | limit the foreground texture pass based on backlog and the observed interval between passes; background worker gets a longer slice; not a GPU utilisation measurement |
| `VICTORY_SKIP_STATE_CALLS=1` | omit identical blend, stencil and depth bounds values within one command list; experimental |
| `VICTORY_ASYNC_SUBMIT=1` | prepare and submit on the existing submission worker; falls back if the worker cannot start |
| `VICTORY_PSO_SHARE=1` | reuse live pending PSOs by pipeline key (weak references); does not keep released PSOs resident |
| `VICTORY_WATCH_APART=1` | experimental parallel WriteWatch queries for disjoint pages; off by default. Compare the same warm-cache scene with `0` and `1`; disable if geometry, textures or stability regress |
| `VICTORY_SHADER_OPT=0` | no clean-up of translated shaders: try it if you see black blocks. Everything is translated again |
| `VICTORY_BVH_BUILDER=1` | asks for ray tracing. Slow (about 4 fps), and not tested with the merged build apart from one run that hung |
| `--work cpu`, `balanced`, `gpu` (`victory run`) | where work is done that both sides can do; `balanced` is the measured choice and the launcher's default |

More switches are described in comments where the code reads them.

For loading comparisons, keep the driver, shader cache, save and scene the
same. Measure start-to-world, time until geometry and textures are present,
median and 90th-percentile frame time, and DirectStorage failures. Test
`HEAP_WORKER`, `WIDE_TEXTURES`, `XMEM_PERSISTENT_VIEW`, and `WATCH_APART`
individually before attributing a change to the combined profile. DirectStorage
workers already decode in parallel; this fork reuses bounded scratch buffers
and avoids repeated allocations without changing completion ordering.
For `-PerformanceLab`, also compare remapping time, copy-lane bytes and
waits, texture backlog, submission preparation time, and library loads
versus actual PSO creations. Test switches individually with the same cache;
none of the experimental combinations has a measured speedup in this fork.

## When something goes wrong

1. **Update first**, then build again: `git pull`, then `.\start.ps1`.
2. **Look at `crashes.log`** in the data folder
   (`%LOCALAPPDATA%\victory\data` by default).
3. **If it names a host GPU failure**, run once more with the debug layer, which
   names the command. It needs the Windows feature "Graphics Tools":

   ```powershell
   $env:VICTORY_D3D12_DEBUG = '1'
   .\start.ps1 -GameExe <the title's exe>
   ```

4. **If `crashes.log` reports Windows error 1455**, Windows ran out of memory to
   promise: close programs, or make the paging file bigger.

In a report, please say: your graphics card, driver version and memory; the
commit (`git rev-parse --short HEAD`); what you saw and when; and the end of
`crashes.log`. Leave out paths with your name, and **do not attach game files,
screenshots or shader dumps** ([asset policy](docs/asset-policy.md)).

## Controls

XInput pads work. Without one, keyboard and mouse stand in while the window has
focus. There are two layouts, one on foot and one for driving; **G** switches
between them. They are in `keys.txt` in the data folder, which the running title
reads again within a second, and the launcher has a **Keys...** button for it.

| On foot | Keys |
|---|---|
| Move, look | W A S D, the mouse (or I J K L) |
| Aim, fire | right click, left click |
| A, B | Shift or Enter, R or Backspace |
| X, Y | Space or X, F or Y |
| Shoulders | Tab, Q |
| Stick clicks | Ctrl (crouch), C or middle click |
| D-pad | arrows, or 1 2 3 4 |
| Menu, View | Esc, V |

| Driving | Keys |
|---|---|
| Accelerate, brake | W, S |
| Steer | A, D |
| Left shoulder, right shoulder | left or right click, Space |

Click in the window to give it the mouse (that click presses nothing); left Alt
or switching windows gives it back. `VICTORY_MOUSE_SENSITIVITY=1.5` turns
faster (1 by default).

## For developers

Rust is pinned in `rust-toolchain.toml`. The tests need no game data.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace
cargo run -p victory -- config           # check your paths
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window's caption shows the frame rate. Saves, settings and `crashes.log`
are in the data folder, derived data in the cache folder.

### Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory translate [paths...] --cache [--pipelines [--used-pipelines]] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
victory doctor
```

`translate --cache` fills the shader cache ahead of a run; with `--pipelines`
also the pipelines, which belong to one card and driver, and with
`--used-pipelines` only those earlier runs asked for:

```powershell
target\release\victory.exe translate --cache --pipelines <game dir>\shaders.rpf <game dir>\<the title's exe> <game dir>\XboxBvhBuilder.dll <game dir>\amd_fidelityfx_upscaler_xbox.dll
```

Why a program is dropped on a card with 32-lane waves (the hash is in the log):

```powershell
$env:VICTORY_HOST_MAX_WAVE = '32'; $env:VICTORY_WAVE_WHY = '1'
$env:VICTORY_TRANSLATE_ONLY = '<hash>'
victory translate --list <game dir>\<the title's exe> <game dir>\shaders.rpf
```

### More to read

| | |
|---|---|
| [Architecture](docs/architecture.md) | how it works |
| [What this fork changed](docs/fork-changes.md) | the list, with numbers |
| [Test runs](docs/test-runs.md) | measured runs, 2026-11-25 |
| [Ray tracing](docs/fork-ray-tracing.md) | what is there and what is missing |
| [`victory-pso`](docs/victory-pso.md) | pipelines without a title |
| [Upstream status](docs/upstream-status.md) | subsystem by subsystem, 2026-11-23 |
| [Asset policy](docs/asset-policy.md) | what must never be committed |

## Debugging

A release build has no debugging aids. `start.ps1` makes the build that has
them; by hand:

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output, for example `info,victory_hle::modules::audio=debug` |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not (keys as in the pad table above) |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight; for a list that does not close, its commands |
| `VICTORY_PIPELINE_WAIT_MS=24` | the longest a frame waits for pipelines being made (500 ms by default, 24 with every `--work` choice). A draw can be omitted until its pipeline is ready |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with `VICTORY_D3D12_DRED=1`, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | every translated shader loop stops after `n` iterations per lane, instead of 256 in pixel, vertex and mesh shaders and 4,096 in compute (own shader cache entries: everything is translated again; a low `n` cuts genuine compute loops short); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## Credits and license

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of everything here: the loader, the runtime
  services, the Direct3D 12 back end and the shader recompiler. Its history is
  kept.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)** and its
  contributors: many changes here come from there, each credited in its commit.
- **[soldsawn/victory-nvidia-8gb](https://github.com/soldsawn/victory-nvidia-8gb)**:
  the GPU and shader engine this branch is built on.
- Parts ported or merged from WillyboPeep's, PastiiJay's (klbf), LeonardoS11's,
  myokex's (victory-amd), reddxd-byte's (ShakiraRED, with Belobug's ambient
  light), clankenjoyer's and tippythetaidum's forks or pull requests, each
  credited in its commit.
- **[XCL](https://github.com/ivall/xcl)** by ivall and its contributors
  (GPL-2.0-only) was used as a reference for how the same title behaves under
  another layer. Fixes inspired by it were written anew in VIctory's code.
- Developed with [Claude Code](https://claude.com/claude-code).

GPL-3.0-or-later. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md).

### Vice-Tay / 7900XTX / Neptune P0 integration notes

- Vice-Tay `565f4a5` inspired the bounded parallel mapping experiment. Vertex does not yet parallelise arbitrary scatter lists. The experiment is deliberately independent of `-PerformanceLab`; benchmark `VICTORY_MAP_THREADS=1`, `2`, and `4` on the same save.
- The relay returns completed dynamic descriptor slots before recycling detached lists, using nonblocking locks. Contended ranges are released by the ordinary destructor.
- Indirect argument gathering now stops at buffer copies and explicit synchronization boundaries (inspired by Vice-Tay `0375962`). This preserves the existing window/gather algorithm; it is not a port of the new batched clamp renderer.
- `PLACED_ORDER`, the thin command-list recorder, and texture uploads on a dedicated copy queue are not yet integrated. Their upstream implementations depend on a different command ordering and resource lifetime architecture. The existing buffer copy queues, async submission, and texture worker remain available, but are not equivalent to those features.
- No Windows build or gameplay benchmark has validated this integration yet.
