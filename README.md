# Dihrill

Dihrill is a native Rust mining mod for Minecraft 26.3 on Horizon for Windows x64. It follows selected ore veins, digs tunnels and vertical shafts, and checks nearby terrain before breaking blocks.

Choose the release that matches your Horizon build.

| Horizon Nightly | Dihrill release |
| --- | --- |
| November 13, 2026 (`9f42064`) | [v0.9.2](https://github.com/compkrx/dihrill/releases/tag/v0.9.2) |
| November 8, 2026 (`fdc87ae`) | [v0.9.1](https://github.com/compkrx/dihrill/releases/tag/v0.9.1) |

## Install

1. Select the matching nightly in the Horizon launcher.
2. Download the matching `.hnmod` from the release and copy it into the instance's `horizon-native-mods` folder. Use **Mods → Open folder** if available. Move the previous package out of that folder before installing Dihrill. On first launch, Dihrill copies existing mining settings into its own settings folder.
3. Choose **Mods → Native build → Build** and launch the resulting build.
4. In Survival, put suitable tools in your hotbar, open your inventory, and click **Dihrill**. **F8** also opens the menu and can be rebound in Controls.

## Mine

Select blocks in **Targets** and put higher-priority targets near the top. The miner weighs priority and route cost. It finishes reachable blocks in the current vein before choosing another. You can enable diagonal connections and include both regular and deepslate forms in the same vein.

Set a profile in **Mining**, then click **Start**. **Pause** preserves the job and **Resume** continues it. Press **F9** during play to pause or resume, or change the binding in Controls. **Stop** ends the job. The menu shows progress, destination, distance walked, tool durability, and the last warning. Settings persist across restarts. Jobs start idle after reconnecting or changing worlds.

| Profile | Behavior |
| --- | --- |
| Ore Hunter | Mine selected veins and search new routes when exploration is enabled. |
| Tunnel Miner | Favor a forward tunnel with the configured width and height. |
| Branch Miner | Explore side tunnels at the configured interval. |
| Cave Miner | Look for reachable ore from existing passages without digging routes through walls. |
| Area Miner | Limit mining to a cuboid set by two block coordinates. |
| Quantity Miner | Stop after a block count or a count of newly collected items. |

In **Mining**, enable **Explore** and **Continuous exploration** to keep searching past the usual distance limit. The selection only affects exploration. Dihrill still stops at your mining quota or when it runs out of safe routes, and can pause for low health, tool wear, or inventory problems.

Open **Mining → Run timer** and type a custom duration. A plain number such as `25` means minutes. Use `8:45` for minutes and seconds, `2:15:30` for hours, minutes and seconds, or units such as `1d 3h 5m 17s`. The editor previews the duration before you apply it. Select **Turn off** or enter `0` to disable the timer.

Applying a new duration resets the current job's countdown while preserving the job. Each new run starts with the saved duration. The countdown pauses with Dihrill or while it waits for you to close a screen. The gameplay HUD and **Status** show time remaining.

For a straight shaft, open **Safety → Vertical digging**, set the **Shaft length** from 1 to 64 blocks, and select **Dig down** or **Dig up**. Stand near the center of a full block with the right mining tools in your hotbar. **Dig down** removes the floor one block at a time and waits for a solid landing. **Dig up** clears headroom and builds a pillar using cobblestone or cobbled deepslate from your hotbar, one block per level. Both directions pause at liquids, falling blocks, unsafe footing, or unloaded terrain. Cave avoidance also applies when enabled. Vertical digging follows the chosen length and run timer. **F9** pauses or resumes the shaft.

You can also type local commands in chat.

| Command | Behavior |
| --- | --- |
| `#dihrill diamond_ore deepslate_diamond_ore` | Mine exact IDs with no quantity limit. |
| `#dihrill iron gold` | Expand ore groups. |
| `#dihrill #minecraft:diamond_ores` | Expand a loaded block tag. |
| `#dihrill stone 256` | Stop after breaking 256 selected blocks. Filler cleared for the route does not count. |
| `#dihrill items diamond 12 diamond` | Stop once the inventory receives 12 new diamonds. A final drop can exceed the target. |
| `#dihrill forever` | Run the saved target list with no quantity limit. |
| `#dihrill pause`, `#dihrill resume`, `#dihrill stop` | Control the current run. |
| `#dihrill status`, `#dihrill settings` | Read progress or open the menu. |

Press **Tab** at the end of a partially typed command to complete names. `/dihrill` accepts the same arguments.

## Sell diamonds automatically

1. On the server that provides the direct-sale `/shop` menu, go to **Dihrill → Inventory → Diamond shop → Open /shop and enable**.
2. Check that you see the usual shop, then close it. Setup opens the menu without clicking.
3. On the **Diamond shop** page, edit **Minimum** (1–2,304 diamonds) and **Wait** (1–3,600 seconds between visits). Both conditions must be met. Defaults are 64 diamonds and 60 seconds. For example, set **Minimum** to 8 and **Wait** to 30 to sell once your inventory holds at least eight diamonds, at most once every 30 seconds.
4. Start or resume mining. Dihrill left-clicks up to two separate stacks, including partial stacks, then closes the shop and resumes mining. The minimum is the total in your inventory. Each click offers the entire stack, and unsold diamonds count toward your next minimum.

The feature starts off and remembers the server address and menu it saw during setup. The menu must show the player inventory and sell from it with a normal left-click. The seller skips customized and protected diamonds. Use **Turn off** in the same menu to disable it. If the server switches menu layouts, run setup again.

## Movement

Turns ease in and out through Horizon’s normal camera input. Dihrill checks whether the vein can be mined from where you stand before taking another step. It mines exposed faces in reach, favors smaller camera turns between blocks, and then collects the drops. It still repositions if a block is hidden, out of reach, or unsafe to break from the current spot. Item goals check confirmed pickups before mining another block.

## Safety and inventory

- The miner uses normal player input, reach, and block breaking. It checks the server's block result before counting a predicted break.
- Routes exclude lava, waterlogged blocks, gravel, and other unstable blocks. Water is avoided by default. **Safety → Shallow wading** allows crossing thin flowing water on level, solid ground with dry headroom. Deep water, falling water, and mining from water stay excluded. The miner checks support and surrounding terrain before clearing a block. Choose a maximum fall of one to three blocks. One block is the default.
- **Safety → Cave / mob avoidance** gives you two independent controls, both on by default. Cave avoidance rejects routes into large underground openings and skips blocks that would break through into them. A one-block-wide vertical shaft by itself does not count as a cave. Turn cave avoidance off for intentional cave mining.
- Mob avoidance defaults to a 12-block detection radius, adjustable from 6 to 24 blocks. A hostile mob in sight or a nearby swelling creeper triggers a retreat along recently walked terrain when that moves you farther from the threat. Dihrill pauses after up to five retreat steps, when no safe retreat is available, or if you take damage outside a retreat. Resume manually when safe.
- Set a durability floor and choose fastest tool, Fortune, or Silk Touch. The miner uses the hotbar and pauses if it cannot proceed safely.
- The junk list starts empty. Pick individual filler types in **Inventory**. When the inventory fills, Dihrill drops every stack of checked junk, then plans a route that prefers to avoid the pile. In a single passage it can still pick up junk that blocks the only route. The miner protects selected resources, configured valuable items, equipment, and customized stacks.
- Storage is off by default. Set the coordinates of a placed chest, trapped chest, or barrel. When full, the miner can walk there and deposit selected resources from the main inventory. It leaves hotbar items alone and pauses if it cannot use the container.

## Limits

Searches use loaded client terrain with bounded route and exploration budgets. Before searching for a route to ore, Dihrill checks chunk palettes for your selected blocks. When those palettes rule out a target, it skips the ore search and moves on to exploration if enabled. When a target may be present, Dihrill first tries a short search and can fall back to a wider one. If it spends 40 seconds waiting for a route without movement or mining progress, it pauses and reports what it was doing. A server can kick a player for idling after mining stops or pauses. Start near the height of your target ore. Anti-xray can conceal ore from the client. The miner does not bridge gaps, swim, place torches, fight mobs, or override rejected server actions. The planner uses full-block support, so routes over some slabs and other partial blocks may be unavailable. Depleted tools and full inventory can pause a run. Use it on servers where the owner permits automation. The multiplayer tests use a stock Horizon dedicated server on localhost.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| The menu or F8 is missing | Check the Nightly revision and use **Build** again after replacing the package. Confirm you launched the resulting native build. |
| No safe route | Check **Status** for the stop reason. Try starting from a dry tunnel two blocks high with a solid floor. Check your targets, tool durability, and **Explore** setting. |
| Dig up stops | Stand near the center of the block. Keep a mining tool and at least one cobblestone or cobbled deepslate per level in your hotbar. |
| The timer is standing still | Close the open screen or resume a paused run. |
| The shop never opens | Complete setup for the current server, check the minimum diamond count, and wait for the configured interval. |

## Build and test

- Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`, source tag `nightly-source/2026-11-13-9f42064`
- Horizon Mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`
- Launcher Rust `1.99.1-x86_64-pc-windows-gnu`

Run `python tools/package.py --output Dihrill-26.3-9f42064-v0.9.2.hnmod` to package the Rust sources. Run `python tools/build.py --help` for pinned build arguments. Compile the standalone planner suite with `rustc --edition 2024 --test tools/route_tests.rs` and run the resulting executable. Run the Python smoke tests in `tools/` with the compiled Horizon client and a scratch output directory.

On Windows, clone the pinned Horizon source with `git -c core.symlinks=false clone` so the mixin staging tool can copy all files. Set `DIHRILL_TEST_RESOURCES` to the prepared 26.3 resource directory if it is outside the nightly's game folder.

The movement changes used the approach, steady aim, and departure visible in [cave diamond mining](https://www.youtube.com/watch?v=23DvqAN8LvM&t=3695s) and [mineshaft mining](https://www.youtube.com/watch?v=RmevsvRIuOo&t=815s) as references.

The design uses Baritone as a reference for goals and route costs. The implementation uses original Rust code and the pinned Horizon engine. [Baritone](https://github.com/cabaletta/baritone) · [Horizon](https://github.com/horizon-rust/horizon) · [Horizon Mixins](https://github.com/horizon-rust/horizon-mixins) · [Native mod template](https://github.com/horizon-rust/horizon-native-mod-template)
