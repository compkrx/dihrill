use super::dihrill::safe_block;
use super::dihrill_route::{Pos, SIDES, plus, up};
use super::{Game, KeyCode, MOVE_KEYS, MouseButton, Screen, Source};
use crate::game::remote::dihrill_remote;
use horizon_core::geometry::{Aabb, Vec3};
use std::cell::{Cell, RefCell};

const ATTACH: [Pos; 6] = [
    [0, -1, 0], [1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1], [0, 1, 0],
];
const BUILD_TICKS: u64 = 200;
const LAND_TICKS: u64 = 30;
const CONFIRM_TICKS: u64 = 35;

thread_local! {
    static BUILD: RefCell<Option<Shelter>> = const { RefCell::new(None) };
    static ESCAPE_CLICK: Cell<Option<EscapeClick>> = const { Cell::new(None) };
}

struct EscapeClick {
    selected: usize,
    original_slot: usize,
    aim: (f32, f32),
    original_view: (f32, f32),
}

struct Shelter {
    started: u64,
    base: Option<Pos>,
    pending: Option<(Pos, u64)>,
    jump: Jump,
    original_slot: usize,
    placed_slot: Option<usize>,
}

#[derive(Default)]
enum Jump {
    #[default]
    Ready,
    Rising { started: u64, owns_key: bool },
    Landing(u64),
    Used,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Air,
    Solid,
    Other,
}

fn block(game: &Game<'_>, [x, y, z]: Pos) -> Block {
    if y < game.world.bottom_y() || y >= game.world.top_y()
        || dihrill_remote::block_pending(game, [x, y, z])
    {
        return Block::Other;
    }
    let Some(chunk) = game.world.chunks.get(&(x >> 4, z >> 4)) else {
        return Block::Other;
    };
    let state = chunk.block(x & 15, y, z & 15);
    let facts = game.res.facts().get(state);
    if game.res.facts().is_air(state) {
        Block::Air
    } else if facts.full_collision && safe_block(facts) {
        Block::Solid
    } else {
        Block::Other
    }
}

fn attachable(game: &Game<'_>, [x, y, z]: Pos) -> bool {
    if block(game, [x, y, z]) != Block::Solid {
        return false;
    }
    let state = game.world.block(x, y, z);
    let facts = game.res.facts().get(state);
    !facts.block_entity && matches!(
        facts.name.as_ref(),
        "minecraft:bedrock" | "minecraft:stone" | "minecraft:granite" | "minecraft:diorite"
            | "minecraft:andesite" | "minecraft:deepslate" | "minecraft:cobbled_deepslate"
            | "minecraft:cobblestone" | "minecraft:tuff" | "minecraft:calcite"
            | "minecraft:dripstone_block" | "minecraft:dirt" | "minecraft:grass_block"
            | "minecraft:obsidian" | "minecraft:netherrack" | "minecraft:blackstone"
            | "minecraft:basalt" | "minecraft:smooth_basalt" | "minecraft:end_stone"
    ) || (!facts.block_entity && facts.name.starts_with("minecraft:") && facts.name.ends_with("_ore"))
}

pub(super) fn material(item: &str) -> bool {
    matches!(item, "minecraft:cobblestone" | "minecraft:cobbled_deepslate")
}

fn usable(game: &Game<'_>) -> bool {
    game.title.is_none() && game.player.is_some() && game.mode_id() == 0
        && !game.remote_loading() && game.screen == Screen::Playing && game.focused
        && !game.pause && !game.dialog_open()
        && !game.left && !game.right
        && !MOVE_KEYS.iter().any(|key| {
            game.keys.held_by(key, Source::Keyboard) || game.keys.held_by(key, Source::Pad)
        })
        && !game.agent.task.as_ref().is_some_and(|task| task.kind.releases_input())
}

pub(in crate::game) fn begin(game: &mut Game<'_>) {
    if !usable(game) { return; }
    BUILD.with(|active| {
        let mut active = active.borrow_mut();
        if active.is_none() {
            *active = Some(Shelter {
                started: game.ticks,
                base: None,
                pending: None,
                jump: Jump::Ready,
                original_slot: game.selected,
                placed_slot: None,
            });
        }
    });
}

fn restore(game: &mut Game<'_>, shelter: &mut Shelter) {
    if matches!(shelter.jump, Jump::Rising { owns_key: true, .. }) {
        game.keys.release(&KeyCode::Space, Source::Agent);
        shelter.jump = Jump::Used;
    }
    if shelter.placed_slot.take() == Some(game.selected) {
        game.selected = shelter.original_slot;
    }
}

pub(in crate::game) fn cancel(game: &mut Game<'_>) {
    restore_escape_click(game);
    if let Some(mut shelter) = BUILD.with(|active| active.borrow_mut().take()) {
        restore(game, &mut shelter);
    }
}

pub(in crate::game) fn restore_escape_click(game: &mut Game<'_>) {
    let Some(click) = ESCAPE_CLICK.with(Cell::take) else { return; };
    if game.selected == click.selected {
        game.selected = click.original_slot;
    }
    if (game.yaw, game.pitch) == click.aim {
        (game.yaw, game.pitch) = click.original_view;
    }
}

fn walls(base: Pos) -> impl Iterator<Item = Pos> {
    SIDES.into_iter().flat_map(move |d| [plus(base, d), up(plus(base, d), 1)])
}

fn centered(game: &Game<'_>, base: Pos) -> bool {
    let [x, _, z] = game.position();
    (x - f64::from(base[0]) - 0.5).abs() <= 0.16
        && (z - f64::from(base[2]) - 0.5).abs() <= 0.16
}

fn click_place(game: &mut Game<'_>, pos: Pos) -> Result<Option<usize>, &'static str> {
    if block(game, pos) != Block::Air {
        return Err("A shelter block is obstructed.");
    }
    let min = Vec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
    let max = Vec3::new(min.x + 1.0, min.y + 1.0, min.z + 1.0);
    if game.player.as_ref().is_none_or(|player| player.bounds().intersects(Aabb::new(min, max))) {
        return Err("The shelter block would overlap the player.");
    }
    let Some(slot) = game.inventory[..9].iter().position(|stack| {
        stack.as_ref().is_some_and(|stack| stack.count > 0 && material(&stack.item))
    }) else {
        return Err("Put spare cobblestone or cobbled deepslate in your hotbar.");
    };
    for d in ATTACH {
        let support = plus(pos, d);
        if !attachable(game, support) || game.aim_block(support, Some(d.map(|n| -n))).is_err()
            || game.pick_entity_hit().is_some()
        {
            continue;
        }
        game.selected = slot;
        let pressed = game.agent_mouse(MouseButton::Right, true);
        let released = game.agent_mouse(MouseButton::Right, false);
        if game.settings.toggle_use {
            game.toggles.button(3, false, false, false);
            game.right = false;
        }
        if pressed.is_err() || released.is_err() {
            return Err("Horizon could not place a shelter block.");
        }
        return Ok(Some(slot));
    }
    Ok(None)
}

fn place(game: &mut Game<'_>, shelter: &mut Shelter, pos: Pos) -> Result<bool, &'static str> {
    let before = game.selected;
    let slot = match click_place(game, pos) {
        Ok(slot) => slot,
        Err(reason) => {
            game.selected = before;
            return Err(reason);
        }
    };
    if let Some(slot) = slot {
        shelter.placed_slot = Some(slot);
        shelter.pending = Some((pos, game.ticks + CONFIRM_TICKS));
    }
    Ok(slot.is_some())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Seal { Started, Waiting, Finished, Failed }

/// Place one block into a two-block-high passage behind the retreat.
/// The caller owns the deadline and must only attempt one passage per chase.
pub(super) fn escape_wall_step(game: &mut Game<'_>, lower: Pos, forward: Pos) -> Seal {
    if !usable(game) || !SIDES.contains(&forward) {
        return Seal::Failed;
    }
    let [x, y, z] = game.position();
    let here = [x.floor() as i32, (y + 0.025).floor() as i32, z.floor() as i32];
    if !(1..=3).any(|distance| plus(lower, forward.map(|v| v * distance)) == here) {
        return Seal::Failed;
    }
    let left = [-forward[2], 0, forward[0]];
    let right = left.map(|v| -v);
    let [a, b] = [plus(lower, left), plus(lower, right)];
    let upper = up(lower, 1);
    let frame = [up(lower, -1), up(lower, 2), a, up(a, 1), b, up(b, 1)];
    if frame.into_iter().any(|pos| block(game, pos) != Block::Solid) {
        return Seal::Failed;
    }
    let targets = [lower, upper];
    if targets.into_iter().any(|pos| dihrill_remote::block_pending(game, pos)) {
        return Seal::Waiting;
    }
    let states = targets.map(|pos| block(game, pos));
    if states.contains(&Block::Other) {
        return Seal::Failed;
    }
    let missing = states.iter().filter(|&&state| state == Block::Air).count() as u32;
    let Some(pos) = targets.into_iter().zip(states)
        .find_map(|(pos, state)| (state == Block::Air).then_some(pos))
    else {
        return Seal::Finished;
    };
    let available: u32 = game.inventory[..9].iter().flatten()
        .filter(|stack| material(&stack.item)).map(|stack| stack.count).sum();
    if available < missing {
        return Seal::Failed;
    }
    let original_slot = game.selected;
    let original_view = (game.yaw, game.pitch);
    if let Ok(Some(selected)) = click_place(game, pos) {
        // Horizon consumes the queued right-click after the pre-tick hook.
        // Keep the placement slot and aim until the next tick.
        ESCAPE_CLICK.set(Some(EscapeClick {
            selected, original_slot, aim: (game.yaw, game.pitch), original_view,
        }));
        Seal::Started
    } else {
        game.selected = original_slot;
        (game.yaw, game.pitch) = original_view;
        Seal::Failed
    }
}

fn step(game: &mut Game<'_>, shelter: &mut Shelter) -> Result<bool, &'static str> {
    let now = game.ticks;
    let Some(player) = game.player.as_ref() else { return Ok(true); };
    if now.saturating_sub(shelter.started) >= BUILD_TICKS {
        return Err("Shelter construction timed out.");
    }
    if let Some(slot) = shelter.placed_slot && slot != game.selected {
        return Ok(true);
    }
    let grounded = player.ground.on_ground;
    let [x, y, z] = game.position();
    let base = if let Some(base) = shelter.base {
        if !centered(game, base) || y < f64::from(base[1]) - 0.04 {
            return Err("The player moved away from the shelter.");
        }
        base
    } else {
        if !grounded || player.velocity.x.hypot(player.velocity.z) > 0.025 {
            if now.saturating_sub(shelter.started) >= LAND_TICKS {
                return Err("A stationary, grounded position is needed for shelter.");
            }
            return Ok(false);
        }
        let base = [x.floor() as i32, (y + 0.025).floor() as i32, z.floor() as i32];
        if !centered(game, base) || block(game, up(base, -1)) != Block::Solid
            || [base, up(base, 1)].into_iter().any(|pos| block(game, pos) != Block::Air)
        {
            return Err("Shelter needs a clear, centered space above a solid floor.");
        }
        let roof = up(base, 2);
        let mut missing = 0;
        for pos in walls(base).chain([roof]) {
            match block(game, pos) {
                Block::Air => missing += 1,
                Block::Solid => (),
                Block::Other => return Err("The shelter space contains an obstruction."),
            }
        }
        if missing == 0 { return Ok(true); }
        let extra = block(game, roof) == Block::Air
            && !ATTACH.into_iter().any(|d| attachable(game, plus(roof, d)));
        let available: u32 = game.inventory[..9].iter().flatten()
            .filter(|stack| material(&stack.item)).map(|stack| stack.count).sum();
        if available < missing + u32::from(extra) {
            return Err("Put up to ten spare cobblestone or cobbled deepslate blocks in your hotbar.");
        }
        shelter.base = Some(base);
        base
    };

    if let Some((pos, deadline)) = shelter.pending {
        if block(game, pos) == Block::Solid {
            shelter.pending = None;
        } else if now >= deadline {
            return Err("The world did not confirm the shelter block.");
        } else {
            return Ok(false);
        }
    }
    match shelter.jump {
        Jump::Rising { started, owns_key } => {
            if owns_key {
                game.keys.release(&KeyCode::Space, Source::Agent);
                shelter.jump = Jump::Rising { started, owns_key: false };
            }
            if game.eye_position().is_some_and(|eye| eye[1] > f64::from(base[1]) + 2.10) {
                for d in SIDES {
                    let anchor = plus(up(base, 2), d);
                    if block(game, anchor) == Block::Air && place(game, shelter, anchor)? {
                        shelter.jump = Jump::Landing(now);
                        return Ok(false);
                    }
                }
            }
            if now.saturating_sub(started) >= 16 {
                return Err("There is no reachable support for the shelter roof.");
            }
            return Ok(false);
        }
        Jump::Landing(started) => {
            if grounded {
                shelter.jump = Jump::Used;
            } else if now.saturating_sub(started) >= LAND_TICKS {
                return Err("The player could not settle after the shelter jump.");
            } else {
                return Ok(false);
            }
        }
        Jump::Ready | Jump::Used => (),
    }
    if !grounded { return Ok(false); }
    let mut walls_missing = false;
    for pos in walls(base) {
        if block(game, pos) == Block::Solid { continue; }
        walls_missing = true;
        if place(game, shelter, pos)? { return Ok(false); }
    }
    if walls_missing {
        return Err("There are no reachable supports for the remaining shelter walls.");
    }
    let roof = up(base, 2);
    if block(game, roof) == Block::Solid { return Ok(true); }
    if place(game, shelter, roof)? { return Ok(false); }
    if matches!(shelter.jump, Jump::Ready) && block(game, up(base, 3)) == Block::Air {
        game.keys.press(KeyCode::Space, Source::Agent);
        shelter.jump = Jump::Rising { started: now, owns_key: true };
        return Ok(false);
    }
    Err("There is no reachable support for the shelter roof.")
}

pub(in crate::game) fn tick(game: &mut Game<'_>) {
    let Some(mut shelter) = BUILD.with(|active| active.borrow_mut().take()) else { return; };
    if !usable(game) {
        restore(game, &mut shelter);
        return;
    }
    match step(game, &mut shelter) {
        Ok(false) => BUILD.with(|active| *active.borrow_mut() = Some(shelter)),
        Ok(true) => {
            restore(game, &mut shelter);
            game.native_chat("[Dihrill] Shelter complete.");
        }
        Err(reason) => {
            restore(game, &mut shelter);
            game.native_chat(&format!("[Dihrill] Shelter incomplete. {reason}"));
        }
    }
}
