# Dihrill development checklist

Pinned target is Minecraft 26.3 on Horizon Nightly 2026-11-08, Windows x64. The package uses the existing Ore Miner ID to retain compatible settings.

- [x] Read the pinned movement, raycast, mining prediction, chat, inventory, and native screen interfaces.
- [x] Implement bounded route search, terrain safety, vein grouping, and forward exploration.
- [x] Run standalone route tests for geometry, hazards, priority, vein commitment, changed terrain, exploration, and search limits.
- [x] Implement persistent settings, legacy target/radius migration, explicit junk protection, and local commands.
- [x] Compile the integrated client with all mixins.
- [x] Verify the inventory button, target picker, pause/resume, settings persistence, and normal mining in a scratch single-player world.
- [x] Verify exploration, inventory storage and disposal, quantity goals, tool choice, and disconnect behavior with controlled fixtures.
- [x] Verify mining and inventory results on a stock dedicated server bound to localhost.
- [x] Match the final archive sources to the tested stage and validate the exact .hnmod bundle.

The release build passed 20 Rust unit tests and both gameplay suites. The fixtures verified all six profiles, face-connected vein commitment, protected ore rejection, client reconnect, exact block quotas, item pickups, enchanted tool preference, the durability floor, the inventory menu, opt-in disposal and wading, and chest storage. The multiplayer fixtures used the stock November 8 Horizon server on loopback.

Known limits include partial-block floors, underwater navigation, loaded terrain and bounded exploration, and single-chest or barrel storage using main inventory slots.

## Version 0.3.0

The Windows x64 build and native bundle validator passed. The planner suite passed 16 tests. The menu and extended gameplay suites passed in scratch single-player worlds and on the stock loopback server. The final build's movement trace recorded 40 samples in a straight tunnel with no near-stops, then completed a corner and three-block vein.

The loopback shop fixture held all sale updates until the client closed the menu. It received exactly two left-click actions, applied a partial first sale and a full second sale, kept the third stack, and observed mining resume. Horizon SMP's live shop has not been tested.

## Version 0.4.0

This update adds configurable diamond count and wait time, cave avoidance, and hostile-mob retreat. The scratch-world safety test mined through a three-wide tunnel, preserved the wall into a cave, then accessed the cave with avoidance disabled. The player ignored a skeleton behind a wall, retreated from a visible creeper and paused without losing health, then resumed manually. The same fixture verified that taking damage during a run pauses mining. The loopback shop fixture verified settings set from the menu, below-threshold inventory, partial stacks, two clicks per visit, cooldown, and returning to mining. Horizon SMP was not used for these tests.

## Version 0.5.0

Dihrill tracks remaining ore item entities before leaving a vein or finishing a quota. It can wait for a pickup, try other safe approaches, and resume a pending pickup after an inventory pause. It reports drops beyond safe reach in chat. The pickup smoke test reproduces the old failure using a delayed diamond and checks both the game inventory and remaining item entities.

## Version 0.9.3

The v0.9.2 loopback baseline left its player connected after pausing, allowing an active cave spider to deal damage. The updated client backed away and switched tunnels in single-player. The stock loopback server and simulated shop verified two diamond sale clicks, continued mining, and full health with the pickaxe, chestplate, and totem still equipped.

The single-player pursuit fixture held off an AI-enabled cave spider with a confirmed two-block barricade, then mined the alternate tunnel without a manual resume. The player finished at full health with the totem still equipped.

The single-player timer fixture verified two-block-high walls, a roof, and clear player space. The loopback timer fixture verified the same enclosure through server-side block checks with an existing roof. The planner suite passed 19 checks, and the native package passed the Horizon validator.

## Version 0.9.4

The utility fixture kept an unlimited run active while bedrock blocked the only route to ore. It verified that the corridor stayed open, then removed the obstruction and observed the same run mine the ore. The finder fixture took the player to a selected ore without a tool or a block break. The client checks also verified right-click expansion, persisted HUD settings, and reassigned menu and HUD keys.

The planner suite passed 19 checks. The cave fixture preserved the barrier into the cave until avoidance was explicitly disabled. The loopback fixture verified retreat, diamond sales, resumed mining and confirmed shelter blocks after the timer.
