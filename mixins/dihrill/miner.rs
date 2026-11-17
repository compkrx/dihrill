use super::{BlockStateId, Game, ItemStack, KeyCode, MouseButton, Screen, Source};
use super::dihrill_route::{
    self as route, Cell, ExploreOptions, Pos, Route, RouteKind, Search, SearchOptions,
    SearchResult, Step, Vein, up,
};
use super::dihrill_settings::{Goal, Pace, Profile, Settings, ShaftDirection, Tool, may_discard};
use crate::game::remote::dihrill_remote;
use super::dihrill_shop::{self as shop, Visit};
use horizon_client::blocks::{Fluid, StateFacts};
use horizon_client::menu::Input;
use horizon_client::mobs::{Mob, Species};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ID: &str = "dihrill";
const INPUT: [KeyCode; 3] = [KeyCode::KeyW, KeyCode::Space, KeyCode::ControlLeft];
const MAX_FAILURES: u8 = 12;
const MAX_HISTORY: usize = 4096;
const MAX_BLOCKED: usize = 512;
const DROP_SPAWN_TICKS: u64 = 24;
const PICKUP_WAIT_TICKS: u64 = 40;
const COLLECTION_TICKS: u64 = 220;
const MAX_QUEUED_DROPS: usize = 8;
const PICKUP_RADIUS: i32 = 12;
const DUMP_SITES: usize = 12;
const FAST_SEARCH_NODES: usize = 960;
const ROUTE_BATCH_NODES: usize = 24;
const ROUTE_NODES_PER_TICK: usize = 720;
const ROUTE_SEARCH_SLICE: Duration = Duration::from_millis(4);
const STALL_LIMIT: Duration = Duration::from_secs(40);
const EVASION_CLEAR_TICKS: u64 = 40;
const MOB_COOLDOWN_TICKS: u64 = 1_800;
const MAX_MOB_ZONES: usize = 8;
const ESCAPE_WALL_TICKS: u64 = 12;

thread_local! {
    static MINER: RefCell<Miner> = RefCell::new(Miner::default());
}

#[derive(Default)]
struct Miner {
    settings: Settings,
    job: Option<Job>,
    loaded: bool,
    ended: bool,
    message: String,
    summary: Totals,
    emergency_exit: Option<&'static str>,
}

#[derive(Clone, Default)]
struct Totals {
    mined: u32,
    tunneled: u32,
    discarded: u32,
    collected: u32,
    distance: f64,
}

struct Job {
    action: Action,
    shaft: Option<Shaft>,
    find_only: bool,
    found: Option<Pos>,
    phase: Phase,
    paused: bool,
    attack: bool,
    scan: Option<Search>,
    exploring_scan: bool,
    deep_scan: bool,
    run_remaining: Option<Duration>,
    timer_sample: Option<Instant>,
    last_progress: Instant,
    progress_position: [f64; 3],
    route: Option<Route>,
    vein: Option<Vein>,
    blocked: HashSet<Pos>,
    blocked_edges: HashSet<(Pos, Pos)>,
    blocked_approaches: HashSet<(Pos, Pos)>,
    failures: u8,
    totals: Totals,
    previous_position: [f64; 3],
    recent_stands: VecDeque<Pos>,
    last_health: f32,
    evasion: Option<Evasion>,
    mob_zones: Vec<MobZone>,
    inventory: HashMap<Arc<str>, u32>,
    inventory_scratch: HashMap<Arc<str>, u32>,
    goal_items: u32,
    unknown_pickups: u32,
    unknown_pickup_until: u64,
    pickup_seen: bool,
    travel_yaw: Option<f32>,
    retry_at: u64,
    dump_heading: Option<f32>,
    dump_ready: Option<u64>,
    dump_sites: VecDeque<Pos>,
    dump_next_slot: Option<usize>,
    inventory_alerted: bool,
    missing_target_tool: bool,
    retry_reason: Option<&'static str>,
    retry_count: u8,
    heading: [i32; 2],
    origin: Pos,
    explored_steps: u32,
    visits: HashMap<Pos, u16>,
    visit_order: VecDeque<Pos>,
    branch_index: u32,
    branch: Branch,
    storage: Option<Storage>,
    pickup: Option<Pickup>,
    drops: VecDeque<DropBatch>,
    look: Look,
    walk_progress: Option<(Pos, f64, u64)>,
    shop: Option<Visit>,
    last_shop: Option<u64>,
    shop_unavailable: bool,
}

struct Shaft {
    direction: ShaftDirection,
    stand: Pos,
    travelled: u32,
    length: u32,
    pending_landing: Option<Pos>,
}

struct DropBatch {
    source: Pos,
    block: Arc<str>,
    spawn_until: u64,
    deadline: Option<u64>,
    tried: HashSet<(u32, Pos)>,
    skipped: HashSet<u32>,
}

#[derive(Clone, Copy)]
struct Pickup {
    entity: u32,
    stand: Pos,
    wait_until: Option<u64>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Branch {
    #[default]
    Main,
    Out,
    Return,
}

struct Storage {
    pos: Pos,
    attempted: HashSet<Pos>,
    stand: Option<Pos>,
    phase: StoragePhase,
}

#[derive(Clone, Copy)]
enum StoragePhase {
    Navigate,
    AwaitOpen { deadline: u64 },
    Deposit { window: i32, ready: u64, next_slot: usize, moved: bool },
    Finish { ready: u64, moved: bool },
}

#[derive(Clone, Copy)]
enum Phase {
    Scanning,
    Selecting,
    Planning,
    Navigating,
    Clearing,
    Mining,
    Collecting,
    Inventory,
    Selling,
    Exploring,
    Replanning,
    Retrying,
    Waiting,
    Recovering,
    DiggingDown,
    DiggingUp,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Scanning => "Scanning",
            Self::Selecting => "Selecting target",
            Self::Planning => "Planning",
            Self::Navigating => "Navigating",
            Self::Clearing => "Clearing path",
            Self::Mining => "Mining vein",
            Self::Collecting => "Collecting",
            Self::Inventory => "Inventory management",
            Self::Selling => "Selling diamonds",
            Self::Exploring => "Exploring",
            Self::Replanning => "Replanning",
            Self::Retrying => "Waiting to rescan",
            Self::Waiting => "Waiting for gameplay",
            Self::Recovering => "Recovering",
            Self::DiggingDown => "Digging down",
            Self::DiggingUp => "Digging up",
        }
    }
}

#[derive(Clone, Copy, Default)]
enum Action {
    #[default]
    Ready,
    Walk { step: Step, started: u64, purpose: WalkPurpose },
    Aim { pos: Pos, target: bool, started: u64 },
    Mine {
        pos: Pos,
        before: BlockStateId,
        slot: usize,
        started: u64,
        timeout: u64,
        target: bool,
    },
    Confirm { pos: Pos, deadline: u64, target: bool, before: BlockStateId },
    Collect { pos: Pos },
    ShaftJump { from: Pos, started: u64 },
    ShaftLanding { to: Pos, deadline: u64 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WalkPurpose { Route, Retreat, MobRetreat }

struct Evasion {
    clear_at: u64,
    retry_at: u64,
    failed_destination: Option<Pos>,
    wall: EscapeWall,
}

#[derive(Clone, Copy)]
enum EscapeWall {
    Ready,
    Building { lower: Pos, direction: Pos, until: u64, clicks: u8 },
    Attempted,
}

struct MobZone {
    center: Pos,
    expires: u64,
}

impl MobZone {
    fn contains(&self, [x, y, z]: Pos) -> bool {
        self.center[1].abs_diff(y) <= 2
            && i64::from(self.center[0].abs_diff(x)).pow(2)
                + i64::from(self.center[2].abs_diff(z)).pow(2) <= 16
    }
}

pub(super) struct Snapshot {
    pub state: &'static str,
    pub running: bool,
    pub paused: bool,
    pub finding: bool,
    pub destination: Option<Pos>,
    pub vein: Option<(usize, usize)>,
    pub mined: u32,
    pub tunneled: u32,
    pub collected: u32,
    pub discarded: u32,
    pub distance: f64,
    pub durability: Option<(u32, u32)>,
    pub message: String,
    pub remaining: Option<u32>,
    pub search_checked: Option<usize>,
    pub shaft: Option<(u32, u32)>,
}

impl Job {
    fn new(game: &Game<'_>, settings: &Settings) -> Self {
        let origin = feet(game);
        let mut inventory = HashMap::new();
        read_inventory(game, &mut inventory);
        Self {
            action: Action::Ready,
            shaft: None,
            find_only: false,
            found: None,
            phase: Phase::Scanning,
            paused: false,
            attack: false,
            scan: None,
            exploring_scan: false,
            deep_scan: false,
            run_remaining: (settings.run_seconds > 0).then(|| Duration::from_secs(u64::from(settings.run_seconds))),
            timer_sample: None,
            last_progress: Instant::now(),
            progress_position: game.position(),
            route: None,
            vein: None,
            blocked: HashSet::new(),
            blocked_edges: HashSet::new(),
            blocked_approaches: HashSet::new(),
            failures: 0,
            totals: Totals::default(),
            previous_position: game.position(),
            recent_stands: VecDeque::from([origin]),
            last_health: game.player.as_ref().map_or(0.0, |p| p.health.health()),
            evasion: None,
            mob_zones: Vec::new(),
            inventory,
            inventory_scratch: HashMap::new(),
            goal_items: 0,
            unknown_pickups: 0,
            unknown_pickup_until: 0,
            pickup_seen: false,
            travel_yaw: None,
            retry_at: 0,
            dump_heading: None,
            dump_ready: None,
            dump_sites: VecDeque::new(),
            dump_next_slot: None,
            inventory_alerted: false,
            missing_target_tool: false,
            retry_reason: None,
            retry_count: 0,
            heading: cardinal_heading(game.yaw),
            origin,
            explored_steps: 0,
            visits: HashMap::from([(origin, 1)]),
            visit_order: VecDeque::from([origin]),
            branch_index: 1,
            branch: Branch::Main,
            storage: None,
            pickup: None,
            drops: VecDeque::new(),
            look: Look::default(),
            walk_progress: None,
            shop: None,
            last_shop: None,
            shop_unavailable: false,
        }
    }

    fn replan(&mut self, now: u64) {
        self.route = None;
        self.scan = None;
        self.exploring_scan = false;
        self.deep_scan = false;
        self.action = Action::Ready;
        self.walk_progress = None;
        self.look = Look::default();
        self.retry_at = now + 4;
        self.dump_heading = None;
        self.dump_ready = None;
        self.phase = Phase::Replanning;
        self.missing_target_tool = false;
        self.retry_reason = None;
        self.pickup = None;
    }

    fn schedule_rescan(&mut self, now: u64, reason: &'static str) {
        self.replan(now);
        self.vein = None;
        self.failures = 0;
        self.blocked_edges.clear();
        self.blocked_approaches.clear();
        self.retry_at = now + (40 << self.retry_count.min(2));
        self.retry_count = self.retry_count.saturating_add(1);
        self.retry_reason = Some(reason);
        self.phase = Phase::Retrying;
        self.last_progress = Instant::now();
    }

    fn visit(&mut self, pos: Pos) {
        if let Some(count) = self.visits.get_mut(&pos) {
            *count = count.saturating_add(1);
            return;
        }
        if self.visits.len() >= MAX_HISTORY
            && let Some(oldest) = self.visit_order.pop_front()
        {
            self.visits.remove(&oldest);
        }
        self.visit_order.push_back(pos);
        self.visits.insert(pos, 1);
    }

    fn destination(&self) -> Option<Pos> {
        match self.action {
            Action::Walk { step, .. } => Some(step.to),
            Action::Aim { pos, .. } | Action::Mine { pos, .. }
            | Action::Confirm { pos, .. } | Action::Collect { pos } => Some(pos),
            Action::ShaftJump { from, .. } => Some(up(from, 1)),
            Action::ShaftLanding { to, .. } => Some(to),
            Action::Ready => self.storage.as_ref().map(|s| s.pos)
                .or_else(|| self.pickup.map(|pickup| pickup.stand))
                .or_else(|| self.route.as_ref().map(|r| r.target)),
        }
    }
}

fn say(game: &mut Game<'_>, message: &str) {
    game.native_chat(&format!("[Dihrill] {message}"));
}

pub(super) fn load(game: &Game<'_>) {
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        if !miner.loaded {
            miner.settings = Settings::load(game);
            miner.loaded = true;
        }
    });
}

pub(super) fn settings() -> Settings {
    MINER.with(|state| state.borrow().settings.clone())
}

pub(super) fn update_settings(game: &Game<'_>, update: impl FnOnce(&mut Settings)) {
    load(game);
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        let old_goal = miner.settings.goal.clone();
        let old_shop = miner.settings.diamond_shop.clone();
        update(&mut miner.settings);
        miner.settings.normalize();
        miner.settings.save(game);
        let goal_changed = old_goal != miner.settings.goal;
        let shop_changed = old_shop != miner.settings.diamond_shop;
        if let Some(job) = &mut miner.job {
            if goal_changed { job.goal_items = 0; }
            job.timer_sample = None;
            if shop_changed { job.shop_unavailable = false; }
            job.paused = true;
            job.replan(game.ticks);
            job.vein = None;
            miner.message = "Settings saved. Resume to apply them.".into();
        }
    });
}

pub(super) fn set_run_timer(game: &Game<'_>, seconds: u32) {
    load(game);
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        miner.settings.run_seconds = seconds;
        miner.settings.save(game);
        if let Some(job) = &mut miner.job {
            job.run_remaining = (seconds > 0).then(|| Duration::from_secs(u64::from(seconds)));
            job.timer_sample = None;
        }
    });
}

pub(super) fn hud_settings() -> super::dihrill_settings::HudSettings {
    MINER.with(|state| state.borrow().settings.hud)
}

pub(super) fn update_hud(
    game: &Game<'_>, update: impl FnOnce(&mut super::dihrill_settings::HudSettings),
) {
    load(game);
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        update(&mut miner.settings.hud);
        miner.settings.normalize();
        miner.settings.save(game);
    });
}

pub(super) fn snapshot(game: &Game<'_>) -> Snapshot {
    MINER.with(|state| {
        let miner = state.borrow();
        let job = miner.job.as_ref();
        let totals = job.map_or(&miner.summary, |job| &job.totals);
        let paused = job.is_some_and(|job| job.paused);
        let running = job.is_some() && !paused;
        let state = match job {
            Some(_) if paused => "Paused",
            Some(job) => job.phase.label(),
            None if miner.ended => "Stopped",
            None => "Idle",
        };
        Snapshot {
            state,
            running,
            paused,
            finding: job.is_some_and(|job| job.find_only),
            destination: job.and_then(Job::destination),
            vein: job.and_then(|job| job.vein.as_ref()).map(|vein| {
                let (total, mined) = vein.counts();
                (mined, total)
            }),
            mined: totals.mined,
            tunneled: totals.tunneled,
            collected: totals.collected,
            discarded: totals.discarded,
            distance: totals.distance,
            durability: game.inventory.get(game.selected).and_then(Option::as_ref)
                .and_then(|stack| durability(game, stack)),
            message: match job.filter(|j| !j.paused) {
                Some(job) if matches!(job.phase, Phase::Waiting) => {
                    "Close the open screen to continue the run.".into()
                }
                Some(job) => job.retry_reason.map_or_else(|| miner.message.clone(), str::to_owned),
                _ => miner.message.clone(),
            },
            remaining: job.and_then(|j| j.run_remaining).map(|t| {
                t.as_secs() as u32 + u32::from(t.subsec_nanos() > 0)
            }),
            search_checked: job.and_then(|j| j.scan.as_ref().map(Search::checked_positions)),
            shaft: job.and_then(|j| j.shaft.as_ref()).map(|s| (s.travelled, s.length)),
        }
    })
}

fn other_automation(game: &Game<'_>) -> bool {
    game.agent.task.as_ref().is_some_and(|task| task.kind.releases_input())
}

fn validate_start(game: &Game<'_>, settings: &Settings, shaft: bool, find_only: bool) -> Result<(), &'static str> {
    if game.title.is_some() || game.player.is_none() || game.remote_loading() || game.mode_id() != 0 {
        return Err("Enter a Survival world first.");
    }
    if other_automation(game) {
        return Err("Wait for the other client automation to finish.");
    }
    if shaft { return Ok(()); }
    if settings.targets.is_empty() {
        return Err("Select at least one target block first.");
    }
    if settings.profile == Profile::Area && settings.area.is_none() {
        return Err("Set both area corners before starting Area Miner.");
    }
    if !find_only && settings.profile == Profile::Quantity && matches!(settings.goal, Goal::Unlimited) {
        return Err("Set a block or item quantity before starting Quantity Miner.");
    }
    Ok(())
}

pub(super) fn run_from_menu(game: &mut Game<'_>) -> String {
    start_job(game, false)
}

pub(super) fn find_from_menu(game: &mut Game<'_>) -> String {
    start_job(game, true)
}

fn start_job(game: &mut Game<'_>, find_only: bool) -> String {
    load(game);
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        let message = match validate_start(game, &miner.settings, false, find_only) {
            Err(reason) => reason,
            Ok(()) => {
                stop(game, &mut miner, "");
                let mut job = Job::new(game, &miner.settings);
                job.find_only = find_only;
                miner.job = Some(job);
                miner.ended = false;
                super::dihrill_shelter::cancel(game);
                game.native_mod_screen_close(ID);
                if find_only { "Finding selected blocks along walkable routes." }
                else { "Started mining. Open the miner menu to pause or stop." }
            }
        };
        miner.message = message.into();
        miner.message.clone()
    })
}

pub(super) fn pause_from_menu(game: &mut Game<'_>) -> String {
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        if miner.job.is_none() {
            miner.message = "The miner is idle.".into();
        } else {
            pause(game, &mut miner, "Paused.");
        }
        miner.message.clone()
    })
}

pub(super) fn run_shaft_from_menu(game: &mut Game<'_>, direction: ShaftDirection) -> String {
    load(game);
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        if let Err(reason) = validate_start(game, &miner.settings, true, false) {
            return reason.into();
        }
        if miner.job.is_some() {
            return "Stop the current run before starting a shaft.".into();
        }
        let mut job = Job::new(game, &miner.settings);
        job.shaft = Some(Shaft {
            direction,
            stand: job.origin,
            travelled: 0,
            length: miner.settings.shaft_length,
            pending_landing: None,
        });
        miner.job = Some(job);
        miner.ended = false;
        super::dihrill_shelter::cancel(game);
        game.native_mod_screen_close(ID);
        miner.message = match direction {
            ShaftDirection::Down => "Started digging down.".into(),
            ShaftDirection::Up => "Started digging up.".into(),
        };
        miner.message.clone()
    })
}

pub(super) fn resume_from_menu(game: &mut Game<'_>) -> String {
    load(game);
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        let shaft = miner.job.as_ref().is_some_and(|job| job.shaft.is_some());
        let find_only = miner.job.as_ref().is_some_and(|job| job.find_only);
        let message = match validate_start(game, &miner.settings, shaft, find_only) {
            Err(reason) => reason,
            Ok(()) => match &mut miner.job {
                None => "No paused job. Start a new job from the miner menu.",
                Some(job) if !job.paused => {
                    game.native_mod_screen_close(ID);
                    "Mining is already active."
                }
                Some(job) => {
                    release(game, Some(job));
                    close_shop(game, job);
                    job.shop_unavailable = false;
                    job.paused = false;
                    if let Some(batch) = job.drops.front_mut() {
                        batch.deadline = None;
                        batch.tried.clear();
                        batch.skipped.clear();
                    }
                    job.previous_position = game.position();
                    job.progress_position = game.position();
                    job.last_progress = Instant::now();
                    job.last_health = game.player.as_ref().map_or(0.0, |p| p.health.health());
                    job.evasion = None;
                    if job.recent_stands.back() != Some(&feet(game)) {
                        job.recent_stands.clear();
                        job.recent_stands.push_back(feet(game));
                    }
                    job.unknown_pickups = 0;
                    job.pickup_seen = false;
                    read_inventory(game, &mut job.inventory);
                    job.failures = 0;
                    job.retry_count = 0;
                    job.blocked_edges.clear();
                    job.blocked_approaches.clear();
                    job.blocked.clear();
                    job.storage = None;
                    job.replan(game.ticks);
                    super::dihrill_shelter::cancel(game);
                    game.native_mod_screen_close(ID);
                    if find_only { "Resumed block search." } else { "Resumed mining." }
                }
            },
        };
        miner.message = message.into();
        miner.message.clone()
    })
}

pub(super) fn stop_from_menu(game: &mut Game<'_>) -> String {
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        super::dihrill_shelter::cancel(game);
        stop(game, &mut miner, "Stopped.");
        miner.message.clone()
    })
}

fn release(game: &mut Game<'_>, job: Option<&mut Job>) {
    if let Some(job) = job {
        release_walk(game);
        release_attack(game, &mut job.attack);
        job.look = Look::default();
    }
}

fn release_walk(game: &mut Game<'_>) {
    for key in INPUT {
        game.keys.release(&key, Source::Agent);
    }
}

fn release_attack(game: &mut Game<'_>, attack: &mut bool) {
    if !std::mem::take(attack) { return; }
    let _ = game.agent_mouse(MouseButton::Left, false);
    if game.settings.toggle_attack {
        game.toggles.button(2, false, false, false);
        game.left = false;
    }
    game.request_remote_mining_stop();
}

fn close_storage(game: &mut Game<'_>, storage: Option<&Storage>) {
    if storage.is_some_and(|s| storage_window(game, s.pos).is_some()) {
        game.close_container();
        game.screen = Screen::Playing;
        game.capture(true);
    }
}

fn pause(game: &mut Game<'_>, miner: &mut Miner, reason: &str) {
    if let Some(job) = &mut miner.job {
        release(game, Some(job));
        close_storage(game, job.storage.as_ref());
        job.storage = None;
        close_shop(game, job);
        job.paused = true;
        job.timer_sample = None;
        job.replan(game.ticks);
    }
    miner.message = reason.into();
}

fn close_shop(game: &mut Game<'_>, job: &mut Job) {
    if let Some(visit) = job.shop.take() { visit.close(game); }
}

fn stop(game: &mut Game<'_>, miner: &mut Miner, reason: &str) {
    if let Some(mut job) = miner.job.take() {
        release(game, Some(&mut job));
        close_storage(game, job.storage.as_ref());
        close_shop(game, &mut job);
        miner.summary = job.totals;
    }
    miner.ended = true;
    miner.message = reason.into();
}

pub(in crate::game) fn unload(game: &mut Game<'_>) {
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        if let Some(mut job) = miner.job.take() {
            release(game, Some(&mut job));
            miner.summary = job.totals;
            miner.ended = true;
            miner.message = "Stopped after leaving the level.".into();
        }
    });
}

fn state_at(game: &Game<'_>, pos: Pos) -> Option<BlockStateId> {
    let [x, y, z] = pos;
    if y < game.world.bottom_y() || y >= game.world.top_y() {
        return None;
    }
    let chunk = game.world.chunks.get(&(x >> 4, z >> 4))?;
    Some(chunk.block(x & 15, y, z & 15))
}

fn facts_at<'game>(game: &'game Game<'_>, pos: Pos) -> Option<&'game StateFacts> {
    state_at(game, pos).map(|id| game.res.facts().get(id))
}

fn falling(facts: &StateFacts) -> bool {
    matches!(
        facts.name.as_ref(),
        "minecraft:gravel" | "minecraft:sand" | "minecraft:red_sand"
        | "minecraft:suspicious_sand" | "minecraft:suspicious_gravel"
        | "minecraft:dragon_egg" | "minecraft:anvil" | "minecraft:chipped_anvil"
        | "minecraft:damaged_anvil"
    ) || facts.name.ends_with("_concrete_powder")
        || facts.has_class("FallingBlock")
}

pub(super) fn safe_block(facts: &StateFacts) -> bool {
    facts.fluid == Fluid::None
        && !facts.waterlogged
        && !falling(facts)
        && !matches!(
            facts.name.as_ref(),
            "minecraft:fire" | "minecraft:soul_fire" | "minecraft:powder_snow"
            | "minecraft:magma_block" | "minecraft:cactus" | "minecraft:sweet_berry_bush"
            | "minecraft:pointed_dripstone" | "minecraft:campfire" | "minecraft:soul_campfire"
            | "minecraft:nether_portal" | "minecraft:end_portal" | "minecraft:end_gateway"
            | "minecraft:bubble_column" | "minecraft:wither_rose"
        )
}

pub(super) fn harvestable(facts: &StateFacts) -> bool {
    facts.hardness.is_finite()
        && facts.hardness >= 0.0
        && !facts.block_entity
        && !facts.outline.is_empty()
        && !matches!(facts.name.as_ref(), "minecraft:tnt" | "minecraft:respawn_anchor")
}

fn filler(name: &str) -> bool {
    matches!(
        name,
        "minecraft:stone" | "minecraft:deepslate" | "minecraft:cobblestone"
        | "minecraft:cobbled_deepslate" | "minecraft:dirt" | "minecraft:grass_block"
        | "minecraft:coarse_dirt" | "minecraft:rooted_dirt" | "minecraft:mud"
        | "minecraft:netherrack" | "minecraft:tuff" | "minecraft:andesite"
        | "minecraft:diorite" | "minecraft:granite" | "minecraft:calcite"
        | "minecraft:blackstone" | "minecraft:dripstone_block" | "minecraft:basalt"
        | "minecraft:smooth_basalt" | "minecraft:end_stone" | "minecraft:sandstone"
        | "minecraft:red_sandstone" | "minecraft:obsidian"
    )
}

fn ore_family(name: &str) -> u32 {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    match name.strip_prefix("deepslate_").unwrap_or(name) {
        "coal_ore" => 1,
        "copper_ore" => 2,
        "iron_ore" => 3,
        "gold_ore" | "nether_gold_ore" => 4,
        "redstone_ore" => 5,
        "lapis_ore" => 6,
        "diamond_ore" => 7,
        "emerald_ore" => 8,
        "nether_quartz_ore" => 9,
        "ancient_debris" => 10,
        _ => 0,
    }
}

fn durability(game: &Game<'_>, stack: &ItemStack) -> Option<(u32, u32)> {
    let default = game.res.items().get(&stack.item)?.max_damage;
    let maximum = stack.data().and_then(|data| data.other("minecraft:max_damage"))
        .and_then(|tag| match tag {
            horizon_world::nbt::Tag::Int(n) => u32::try_from(*n).ok(),
            _ => None,
        }).unwrap_or(default);
    (maximum > 0).then_some((maximum.saturating_sub(stack.damage), maximum))
}

fn enchantment(stack: Option<&ItemStack>, id: &str) -> u32 {
    stack.map_or(0, |stack| stack.enchantment_level(id))
}

#[derive(Clone, Copy)]
struct Choice {
    slot: usize,
    ticks: u16,
    score: (u32, u32, u16, bool, bool),
}

fn choose_tool(game: &Game<'_>, settings: &Settings, facts: &StateFacts, target: bool) -> Option<Choice> {
    let items = game.res.items();
    let mut best: Option<Choice> = None;
    for slot in 0..9 {
        let stack = game.inventory[slot].as_ref();
        let uses = stack.and_then(|stack| durability(game, stack));
        if uses.is_some_and(|(remaining, _)| remaining <= settings.durability) {
            continue;
        }
        let correct = items.correct_for_drops(stack, &facts.name);
        if facts.requires_tool && !correct {
            continue;
        }
        let base = items.mining_speed(stack, &facts.name);
        let efficiency = enchantment(stack, "minecraft:efficiency").min(255);
        let bonus = if base > 1.0 && efficiency > 0 { (efficiency * efficiency + 1) as f32 } else { 0.0 };
        let speed = (base + bonus).max(1.0);
        let ticks = (facts.hardness * 30.0 / speed).ceil().clamp(1.0, 60_000.0) as u16;
        let preferred = if target {
            match settings.tool {
                Tool::Fast => 0,
                Tool::Fortune => enchantment(stack, "minecraft:fortune"),
                Tool::Silk => enchantment(stack, "minecraft:silk_touch"),
            }
        } else { 0 };
        let correct_speed = (speed * 16.0) as u32;
        let reserve = if base > 1.0 { uses.map_or(0, |(left, _)| left.min(u32::from(u16::MAX)) as u16) } else { 0 };
        let score = (preferred, correct_speed, reserve, uses.is_none(), slot == game.selected);
        let candidate = Choice { slot, ticks, score };
        if best.is_none_or(|previous| previous.score < score) {
            best = Some(candidate);
        }
    }
    best
}

struct WorldRead<'a, 'g> {
    game: &'a Game<'g>,
    settings: &'a Settings,
    blocked: &'a HashSet<Pos>,
    mob_zones: &'a [MobZone],
    respect_area: bool,
    states: HashMap<BlockStateId, StateCell>,
    missing_target_tool: bool,
    caves: HashMap<Pos, bool>,
}

#[derive(Clone, Copy)]
struct StateCell {
    cell: Cell,
    needs_tool: bool,
}

impl<'a, 'g> WorldRead<'a, 'g> {
    fn new(game: &'a Game<'g>, settings: &'a Settings, blocked: &'a HashSet<Pos>, mob_zones: &'a [MobZone]) -> Self {
        Self { game, settings, blocked, mob_zones, respect_area: true, states: HashMap::new(), missing_target_tool: false, caves: HashMap::new() }
    }

    fn cell(&mut self, pos: Pos) -> Cell {
        let Some(state) = state_at(self.game, pos) else { return Cell::default(); };
        if dihrill_remote::block_pending(self.game, pos) { return Cell::default(); }
        let game = self.game;
        let settings = self.settings;
        let StateCell { mut cell, needs_tool } = *self.states.entry(state).or_insert_with(|| {
            let facts = game.res.facts().get(state);
            let hazard = !safe_block(facts);
            let target_index = settings.targets.iter().position(|id| id == facts.name.as_ref());
            let target = target_index.is_some();
            let diggable = !hazard && harvestable(facts) && (target || filler(&facts.name));
            let dig_cost = if diggable {
                choose_tool(game, settings, facts, target).map(|tool| tool.ticks.saturating_add(8))
            } else {
                None
            };
            StateCell {
                cell: Cell {
                    known: true,
                    clear: game.res.facts().is_air(state),
                    floor: facts.full_collision,
                    hazard,
                    shallow_water: settings.shallow_water && shallow_water(facts),
                    falling: falling(facts),
                    dig_cost,
                    target,
                    vein: if target { ore_family(&facts.name) } else { 0 },
                    priority: target_index.map_or(0, |index| 8u8.saturating_sub(index as u8)),
                    ..Cell::default()
                },
                needs_tool: target && diggable && dig_cost.is_none(),
            }
        });
        let allowed = !self.respect_area || settings.profile != Profile::Area
            || settings.area.is_some_and(|(a, b)| route::inside(pos, [a, b]));
        if (cell.dig_cost.is_some() || needs_tool) && allowed && !self.blocked.contains(&pos) {
            self.missing_target_tool |= needs_tool;
        } else {
            cell.dig_cost = None;
        }
        cell.cave = settings.avoid_caves && cell.clear
            && [pos, up(pos, -1)].into_iter().any(|p| {
                *self.caves.entry(p).or_insert_with(|| cave_opening(game, p))
            });
        // Reuse the planner's no-entry/no-break boundary for observed threats.
        cell.cave |= self.mob_zones.iter().any(|zone| zone.contains(pos));
        cell
    }
}

fn air_at(game: &Game<'_>, pos: Pos) -> bool {
    state_at(game, pos).is_some_and(|id| game.res.facts().is_air(id))
}

/// Distinguish underground chambers and tall openings from narrow mining
/// passages. Skylight makes ordinary outdoor terrain ineligible.
fn cave_opening(game: &Game<'_>, pos: Pos) -> bool {
    let [x, y, z] = pos;
    game.world.light(x, y + 1, z) >> 4 < 12
        && route::cave_opening(pos, |p| air_at(game, p))
}

fn feet(game: &Game<'_>) -> Pos {
    let [x, y, z] = game.position();
    [x.floor() as i32, (y + 0.025).floor() as i32, z.floor() as i32]
}

fn ground(game: &Game<'_>) -> bool {
    game.player.as_ref().is_some_and(|player| player.ground.on_ground)
}

fn cardinal_heading(yaw: f32) -> [i32; 2] {
    let radians = yaw.to_radians();
    let (x, z) = (-radians.sin(), radians.cos());
    if x.abs() > z.abs() { [x.signum() as i32, 0] } else { [0, z.signum() as i32] }
}

fn shallow_water(facts: &StateFacts) -> bool {
    facts.name.as_ref() == "minecraft:water" && (1..=3).contains(&facts.fluid_level)
        && !facts.fluid_falling
}

fn solid_shaft_support(game: &Game<'_>, pos: Pos) -> bool {
    !dihrill_remote::block_pending(game, pos) && facts_at(game, pos).is_some_and(|facts| {
        facts.full_collision && !facts.block_entity && safe_block(facts)
            && !matches!(facts.name.as_ref(), "minecraft:tnt" | "minecraft:respawn_anchor")
    })
}

fn shaft_is_centered(game: &Game<'_>, stand: Pos) -> bool {
    let [x, _, z] = game.position();
    (x - f64::from(stand[0]) - 0.5).abs() <= 0.16
        && (z - f64::from(stand[2]) - 0.5).abs() <= 0.16
}

fn shaft_clearance(game: &Game<'_>, world: &mut WorldRead<'_, '_>, stand: Pos, direction: ShaftDirection) -> bool {
    let (bottom, top) = match direction {
        ShaftDirection::Down => (-2, 1),
        ShaftDirection::Up => (-1, 3),
    };
    for dy in bottom..=top {
        let center = up(stand, dy);
        for offset in route::SIDES.into_iter().chain([[0, 0, 0]]) {
            let pos = route::plus(center, offset);
            let cell = world.cell(pos);
            if !cell.known || cell.hazard || cell.falling || cell.cave {
                return false;
            }
        }
    }
    let support = up(stand, bottom);
    solid_shaft_support(game, support)
}

fn shaft_block_slot(game: &Game<'_>) -> Option<usize> {
    game.inventory[..9].iter().position(|stack| {
        stack.as_ref().is_some_and(|item| item.count > 0 && pillar_material(&item.item))
    })
}

fn pillar_material(id: &str) -> bool {
    matches!(id, "minecraft:cobblestone" | "minecraft:cobbled_deepslate")
}

fn body_safe(game: &Game<'_>, settings: &Settings, position: Pos) -> bool {
    facts_at(game,position).is_some_and(|facts| {
        safe_block(facts) || (settings.shallow_water && shallow_water(facts))
    }) && facts_at(game,up(position,1)).is_some_and(safe_block)
}

fn danger(game: &Game<'_>, settings: &Settings, here: Pos) -> Option<&'static str> {
    if !body_safe(game, settings, here) {
        return Some("Paused because the player is in hazardous or unloaded terrain.");
    }
    let player = game.player.as_ref()?;
    if player.fall_distance > f64::from(settings.max_fall) + 0.5 {
        return Some("Paused because the player is falling.");
    }
    None
}

fn enemy_distance(game: &Game<'_>, settings: &Settings, mob: &Mob, eye: [f64; 3]) -> Option<f64> {
    let [x, y, z] = eye;
    let dx = mob.position.x - x;
    let dz = mob.position.z - z;
    let dy = mob.eye_y() - y;
    let distance = dx.hypot(dy).hypot(dz);
    let close = if mob.species() == Species::CaveSpider && settings.avoid_mobs { 3.0 } else { 2.5 };
    let swelling = settings.avoid_mobs && mob.species() == Species::Creeper
        && mob.swell > 0 && distance <= 7.0;
    let near = if settings.avoid_mobs {
        mob.species() == Species::CaveSpider && distance < close
    } else {
        distance < close
    };
    (distance < 0.01 || near || swelling || settings.avoid_mobs && distance <= f64::from(settings.mob_radius)
        && game.clip_ray(eye, [dx / distance, dy / distance, dz / distance], distance).is_none())
        .then_some(distance)
}

fn enemies<'a>(game: &'a Game<'_>) -> impl Iterator<Item = &'a Mob> {
    game.mobs.iter().filter(|mob| !mob.removed && mob.alive() && mob.is_enemy())
}

fn nearest_threat(game: &Game<'_>, settings: &Settings) -> Option<Pos> {
    let eye = game.eye_position()?;
    enemies(game)
        .filter_map(|mob| Some((enemy_distance(game, settings, mob, eye)?, mob.position)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, at)| [at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32])
}

fn remember_threat(job: &mut Job, center: Pos, now: u64) {
    if let Some(zone) = job.mob_zones.iter_mut().find(|zone| zone.contains(center)) {
        zone.expires = now + MOB_COOLDOWN_TICKS;
        return;
    }
    if job.mob_zones.len() == MAX_MOB_ZONES {
        let oldest = job.mob_zones.iter().enumerate().min_by_key(|(_, zone)| zone.expires)
            .map(|(index, _)| index).expect("mob zones at capacity");
        job.mob_zones.swap_remove(oldest);
    }
    job.mob_zones.push(MobZone { center, expires: now + MOB_COOLDOWN_TICKS });
}

fn safer_from_mobs(game: &Game<'_>, settings: &Settings, [x, y, z]: Pos) -> Option<f64> {
    let eye = game.eye_position()?;
    let eye_height = eye[1] - game.position()[1];
    let destination = [f64::from(x) + 0.5, f64::from(y) + eye_height, f64::from(z) + 0.5];
    let mut current = f64::INFINITY;
    let mut next = f64::INFINITY;
    for mob in enemies(game) {
        let from = enemy_distance(game, settings, mob, eye);
        let to = enemy_distance(game, settings, mob, destination);
        if from.is_none() && to.is_some() { return None; }
        current = current.min(from.unwrap_or(f64::INFINITY));
        next = next.min(to.unwrap_or(f64::INFINITY));
    }
    (next.is_infinite() || next > current + 0.10).then_some(next)
}

fn escape_step(game: &Game<'_>, settings: &Settings, job: &Job, here: Pos, failed: Option<Pos>) -> Option<Step> {
    if !ground(game) || game.screen != Screen::Playing { return None; }
    // The quarantine prevents resumed mining from returning to danger. An
    // escape may have to cross its edge to get out, but still checks terrain.
    let mut world = WorldRead::new(game, settings, &job.blocked, &[]);
    let mut evaluate = |to| {
        if failed == Some(to) { return None; }
        let (step, _) = route::transition_with_fall(
            &mut |p| world.cell(p), here, to, false, settings.max_fall,
        )?;
        Some((safer_from_mobs(game, settings, to)?, step))
    };
    if let Some(step) = job.recent_stands.iter().rev().take(2)
        .copied().filter(|&pos| pos != here).find_map(|pos| evaluate(pos).map(|(_, step)| step))
    {
        return Some(step);
    }
    route::SIDES.into_iter().flat_map(|side| {
        let near = route::plus(here, side);
        [0, 1, -1, -2, -3].into_iter().map(move |dy| up(near, dy))
    }).filter_map(evaluate).max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, step)| step)
}

fn escape_wall_site(
    game: &Game<'_>, settings: &Settings, here: Pos, direction: Pos, min_gap: f64,
) -> Option<EscapeWall> {
    if !route::SIDES.contains(&direction) { return None; }
    let side = [direction[2], 0, -direction[0]];
    let spare = game.inventory[..9].iter().flatten().any(|stack| {
        stack.count >= 2 && stack.extra.is_none() && pillar_material(&stack.item)
    });
    if !spare { return None; }
    let eye = game.eye_position()?;
    let [x, y, z] = game.position();
    let lower = route::plus(here, direction.map(|axis| -2 * axis));
    let mob_behind = enemies(game).any(|mob| {
        let dx = mob.position.x - x;
        let dz = mob.position.z - z;
        let rear = dx * f64::from(direction[0]) + dz * f64::from(direction[2]);
        let across = dx * f64::from(side[0]) + dz * f64::from(side[2]);
        rear <= -min_gap && across.abs() <= 0.85 && (mob.position.y - y).abs() <= 1.0
            && enemy_distance(game, settings, mob, eye).is_some()
    });
    if !mob_behind { return None; }
    let safe_solid = |p| facts_at(game, p).is_some_and(|facts| {
        facts.full_collision && !facts.block_entity && safe_block(facts)
    });
    if [lower, up(lower, 1)].into_iter().all(|p| {
        air_at(game, p) && !dihrill_remote::block_pending(game, p)
            && safe_solid(route::plus(p, side))
            && safe_solid(route::plus(p, side.map(|axis| -axis)))
    }) && safe_solid(up(lower, -1)) && safe_solid(up(lower, 2)) {
        Some(EscapeWall::Building { lower, direction, until: game.ticks + ESCAPE_WALL_TICKS, clicks: 0 })
    } else {
        None
    }
}

fn threat_too_close_to_build(game: &Game<'_>, settings: &Settings) -> bool {
    let Some(eye) = game.eye_position() else { return true; };
    enemies(game).any(|mob| enemy_distance(game, settings, mob, eye).is_some_and(|range| range < 2.8))
}

fn retreat_step(game: &Game<'_>, settings: &Settings, job: &mut Job, here: Pos) -> Option<Step> {
    if !ground(game) || game.screen != Screen::Playing { return None; }
    let at_last_stand = job.recent_stands.back() == Some(&here);
    let previous = job.recent_stands.iter().rev().nth(usize::from(at_last_stand)).copied()?;
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    let (step, _) = route::transition_with_fall(
        &mut |p| world.cell(p), here, previous, false, settings.max_fall,
    )?;
    if at_last_stand { job.recent_stands.pop_back(); }
    Some(step)
}

fn read_inventory(game: &Game<'_>, items: &mut HashMap<Arc<str>, u32>) {
    items.clear();
    for stack in game.inventory[..36].iter().flatten() {
        let count = items.entry(Arc::clone(&stack.item)).or_default();
        *count = count.saturating_add(stack.count);
    }
}

pub(super) fn selected_drop(settings: &Settings, item: &str) -> bool {
    settings.targets.iter().any(|block| {
        let block = block.as_str();
        block == item || drop_item(block) == Some(item)
    })
}

fn drop_item(block: &str) -> Option<&'static str> {
    Some(match ore_family(block) {
            1 => "minecraft:coal",
            2 => "minecraft:raw_copper",
            3 => "minecraft:raw_iron",
            4 if block == "minecraft:nether_gold_ore" => "minecraft:gold_nugget",
            4 => "minecraft:raw_gold",
            5 => "minecraft:redstone",
            6 => "minecraft:lapis_lazuli",
            7 => "minecraft:diamond",
            8 => "minecraft:emerald",
            9 => "minecraft:quartz",
            _ => match block {
                "minecraft:stone" => "minecraft:cobblestone",
                "minecraft:deepslate" => "minecraft:cobbled_deepslate",
                "minecraft:grass_block" | "minecraft:farmland" | "minecraft:dirt_path"
                | "minecraft:podzol" | "minecraft:mycelium" | "minecraft:rooted_dirt" => "minecraft:dirt",
                _ => return None,
            },
    })
}

pub(in crate::game) fn item_pickup(game: &Game<'_>, collector: i32, entity: i32, amount: i32) {
    if amount <= 0 || dihrill_remote::player_id(game) != Some(collector) {
        return;
    }
    MINER.with(|state| {
        let Ok(mut miner) = state.try_borrow_mut() else { return; };
        let Miner { settings, job, .. } = &mut *miner;
        let Some(job) = job.as_mut().filter(|job| !job.paused) else { return; };
        let amount = amount as u32;
        if let Some(drop) = game.dropped.iter().find(|drop| drop.id == entity as u32) {
            record_pickup(settings, job, &drop.stack.item, amount);
            job.pickup_seen = true;
        } else {
            job.unknown_pickups = job.unknown_pickups.saturating_add(amount).min(256);
            job.unknown_pickup_until = game.ticks + 8;
        }
    });
}

fn record_pickup(settings: &Settings, job: &mut Job, item: &str, amount: u32) {
    if selected_drop(settings, item) {
        job.totals.collected = job.totals.collected.saturating_add(amount);
    }
    if matches!(&settings.goal, Goal::Items(wanted, _) if wanted == item) {
        job.goal_items = job.goal_items.saturating_add(amount);
    }
}

fn observe(game: &Game<'_>, settings: &Settings, job: &mut Job) {
    let position = game.position();
    let distance = position.into_iter().zip(job.previous_position)
        .map(|(now, before)| (now - before).powi(2)).sum::<f64>().sqrt();
    if distance < 4.0 { job.totals.distance += distance; }
    let moved = position.into_iter().zip(job.progress_position)
        .map(|(a,b)| (a-b).powi(2)).sum::<f64>();
    if moved >= 0.25 {
        job.progress_position = position;
        job.last_progress = Instant::now();
        job.retry_count = 0;
    }
    job.previous_position = position;

    if game.ticks > job.unknown_pickup_until { job.unknown_pickups = 0; }
    read_inventory(game, &mut job.inventory_scratch);
    // Integrated and remote servers report pickups through TakeItemEntity.
    // Inventory changes alone could be a server correction or storage move.
    let fallback = game.remote.is_none() || (job.unknown_pickups > 0 && !job.pickup_seen);
    if fallback && job.storage.is_none() {
        let mut gains = Vec::with_capacity(4);
        for (item, &count) in &job.inventory_scratch {
            let gained = count.saturating_sub(job.inventory.get(item).copied().unwrap_or(0));
            if gained > 0 { gains.push((Arc::clone(item), gained)); }
        }
        gains.sort_unstable_by(|a,b| a.0.cmp(&b.0));
        for (item, gained) in gains {
            let amount = if game.remote.is_none() { gained } else { gained.min(job.unknown_pickups) };
            job.unknown_pickups = job.unknown_pickups.saturating_sub(amount);
            if amount > 0 { record_pickup(settings, job, &item, amount); }
        }
    }
    job.pickup_seen = false;
    std::mem::swap(&mut job.inventory, &mut job.inventory_scratch);
}

fn goal_met(settings: &Settings, job: &Job) -> bool {
    if let Some(shaft) = &job.shaft {
        return shaft.travelled >= shaft.length;
    }
    match &settings.goal {
        Goal::Unlimited => false,
        Goal::Blocks(n) => job.totals.mined >= *n,
        Goal::Items(_, n) => job.goal_items >= *n,
    }
}

fn storage_available(game: &Game<'_>, pos: Pos) -> bool {
    facts_at(game, pos).is_some_and(|facts| matches!(
        facts.name.as_ref(),
        "minecraft:chest" | "minecraft:trapped_chest" | "minecraft:barrel"
    ))
}

fn storage_window(game: &Game<'_>, pos: Pos) -> Option<i32> {
    if game.screen != Screen::Inventory
        || game.container != Some((pos[0], pos[1], pos[2]))
        || !storage_available(game, pos)
    {
        return None;
    }
    if game.remote.is_none() { return Some(0); }
    let (id, menu) = game.remote_menu();
    (id > 0 && menu == "minecraft:generic_9x3").then_some(id)
}

fn storable(game: &Game<'_>, settings: &Settings, stack: &ItemStack) -> bool {
    stack.count > 0 && stack.damage == 0 && stack.extra.is_none()
        && game.res.items().get(&stack.item).is_some_and(|def| def.max_damage == 0 && def.food.is_none())
        && selected_drop(settings, &stack.item)
}

fn junk_pickup_risk(game: &Game<'_>, settings: &Settings, job: &Job) -> HashSet<Pos> {
    let mut risk = HashSet::new();
    if job.dump_sites.is_empty() { return risk; }
    for item in &game.dropped {
        let kind = item.stack.item.as_ref();
        if !may_discard(settings, kind) || selected_drop(settings, kind) {
            continue;
        }
        let p = item.position;
        let cell = [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32];
        if !job.dump_sites.iter().any(|site| {
            cell[0].abs_diff(site[0]) <= 6 && cell[2].abs_diff(site[2]) <= 6
                && cell[1].abs_diff(site[1]) <= 4
        }) {
            continue;
        }
        for x in cell[0] - 2..=cell[0] + 2 {
            for z in cell[2] - 2..=cell[2] + 2 {
                if (f64::from(x) + 0.5 - p.x).abs() > 1.75
                    || (f64::from(z) + 0.5 - p.z).abs() > 1.75
                {
                    continue;
                }
                for y in cell[1] - 2..=cell[1] + 1 {
                    risk.insert([x, y, z]);
                }
            }
        }
    }
    risk
}

fn junk_slot(game: &Game<'_>, settings: &Settings, job: &Job, start: usize) -> Option<usize> {
    let reserve_pillar = job.shaft.as_ref().is_some_and(|shaft| shaft.direction == ShaftDirection::Up);
    let shelter_slot = if settings.shelter_on_stop || settings.retreat_barricade {
        game.inventory[..9].iter().enumerate().filter_map(|(index, stack)| {
            let stack = stack.as_ref()?;
            (stack.count > 0 && stack.extra.is_none() && stack.damage == 0 && pillar_material(&stack.item))
                .then_some((index, stack.count))
        }).max_by_key(|&(_, count)| count).map(|(index, _)| index)
    } else {
        None
    };
    (start..36).find(|&index| {
        let slot = &game.inventory[index];
        slot.as_ref().is_some_and(|stack| {
            stack.count > 0 && stack.extra.is_none() && stack.damage == 0
                && shelter_slot != Some(index)
                && !(reserve_pillar && index < 9 && pillar_material(&stack.item))
                && may_discard(settings, &stack.item) && !selected_drop(settings, &stack.item)
        })
    })
}

fn maybe_make_room(game: &mut Game<'_>, settings: &Settings, job: &mut Job) -> Result<bool, String> {
    let free = game.inventory[..36].iter().filter(|stack| stack.is_none()).count();
    if let Some(start) = job.dump_next_slot {
        job.phase = Phase::Inventory;
        if let Some(slot) = junk_slot(game, settings, job, start) {
            drop_junk_stack(game, settings, job, slot)?;
            return Ok(true);
        }
        if job.dump_ready.is_some_and(|ready| game.ticks < ready + 8) { return Ok(true); }
        job.dump_next_slot = None;
        job.dump_heading = None;
        job.dump_ready = None;
        job.retry_at = game.ticks + 1;
        job.route = None;
        job.scan = None;
        job.inventory_alerted = false;
        return Ok(true);
    }
    if free > 0 {
        if free == 1 && settings.alert_inventory && !job.inventory_alerted {
            say(game, "One free inventory slot remains.");
            job.inventory_alerted = true;
        } else if free > 2 {
            job.inventory_alerted = false;
        }
        return Ok(false);
    }
    job.phase = Phase::Inventory;
    if game.ticks < job.retry_at { return Ok(true); }
    if let Some(slot) = junk_slot(game, settings, job, 0) {
        release(game, Some(job));
        job.dump_next_slot = Some(0);
        drop_junk_stack(game, settings, job, slot)?;
        return Ok(true);
    }
    if let Some(pos) = settings.storage {
        if !storage_available(game, pos) {
            return Err("Storage is unloaded or is not a single chest or barrel.".into());
        }
        if game.inventory[9..36].iter().flatten().any(|stack| storable(game, settings, stack)) {
            job.storage = Some(Storage {
                pos, attempted: HashSet::new(), stand: None, phase: StoragePhase::Navigate,
            });
            job.route = None;
            job.scan = None;
            return Ok(true);
        }
    }
    Err("Inventory full. Make room, configure junk, or set storage with resources in the main inventory.".into())
}

fn dump_yaw(game: &Game<'_>, job: &Job) -> f32 {
    let forward = job.travel_yaw.unwrap_or(game.yaw);
    let here = feet(game);
    let options = [forward + 180.0, forward + 90.0, forward - 90.0, forward];
    let clear_distance = |yaw: f32| {
        let [dx, dz] = cardinal_heading(yaw);
        (1..=3).take_while(|distance| {
            let stand = [here[0] + dx * distance, here[1], here[2] + dz * distance];
            air_at(game, stand) && air_at(game, up(stand, 1))
        }).count()
    };
    options.into_iter().enumerate().max_by_key(|&(order, yaw)| {
        (clear_distance(yaw), std::cmp::Reverse(order))
    }).map_or(forward + 180.0, |(_, yaw)| yaw)
}

fn drop_junk_stack(game: &mut Game<'_>, settings: &Settings, job: &mut Job, slot: usize) -> Result<(), String> {
    if game.carried.is_some() || (game.remote.is_some() && game.remote_menu().0 != 0) {
        return Err("Close the open inventory or container before discarding junk.".into());
    }
    let wanted = job.dump_heading.unwrap_or_else(|| dump_yaw(game, job));
    job.dump_heading = Some(wanted);
    if !turn(game, &mut job.look, wanted, 0.0, settings.turn_speed) {
        return Ok(());
    }
    let ready = *job.dump_ready.get_or_insert(game.ticks + 2);
    if game.ticks < ready { return Ok(()); }
    let accepted = if game.remote.is_some() {
        game.remote_menu_ops(&[(Some(slot), 1, Input::Throw)])
    } else {
        game.menu_click(Some(slot), 1, Input::Throw)
    };
    if !accepted || game.inventory[slot].is_some() {
        return Err("The inventory did not accept the junk disposal.".into());
    }
    let here = feet(game);
    if !job.dump_sites.contains(&here) {
        job.dump_sites.push_back(here);
        if job.dump_sites.len() > DUMP_SITES { job.dump_sites.pop_front(); }
    }
    job.totals.discarded = job.totals.discarded.saturating_add(1);
    job.dump_ready = Some(game.ticks + 2);
    job.dump_next_slot = Some(slot + 1);
    Ok(())
}

fn angle_delta(from: f32, to: f32) -> f32 {
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

#[derive(Default)]
struct Look {
    velocity: [f32; 2],
    tick: Option<u64>,
}

fn turn_step(error: f32, velocity: f32, limit: f32) -> f32 {
    if error.abs() <= 0.22 { return error; }
    let acceleration = (limit * 0.27).max(1.0);
    let braking_speed = (2.0 * acceleration * error.abs()).sqrt();
    let desired = error.signum() * (error.abs() * 0.46).min(braking_speed).min(limit);
    let step = velocity + (desired - velocity).clamp(-acceleration, acceleration);
    // A new target may reverse the steering direction. Stop before pushing
    // the camera through the target or drifting away from the new aim point.
    if step.signum() != error.signum() { 0.0 }
    else { error.signum() * step.abs().min(error.abs()) }
}

fn turn(game: &mut Game<'_>, look: &mut Look, yaw: f32, pitch: f32, speed: u8) -> bool {
    let limit = f32::from(speed);
    if look.tick != Some(game.ticks) {
        let previous = if look.tick.is_some_and(|tick| tick + 1 == game.ticks) {
            look.velocity
        } else { [0.0; 2] };
        let dx = turn_step(angle_delta(game.yaw, yaw), previous[0], limit);
        let dy = turn_step(pitch - game.pitch, previous[1], limit * 0.80);
        // Horizon's normal Entity.turn takes pre-scaled mouse deltas.
        game.turn_player(f64::from(dx) / 0.15, f64::from(dy) / 0.15);
        look.velocity = [dx, dy];
        look.tick = Some(game.ticks);
    }
    angle_delta(game.yaw, yaw).abs() < 1.0 && (pitch - game.pitch).abs() < 1.0
}

fn target_angles(game: &mut Game<'_>, pos: Pos) -> Option<(f32, f32)> {
    let previous = (game.yaw, game.pitch);
    let angles = game.aim_block(pos, None).ok().map(|_| (game.yaw, game.pitch));
    (game.yaw, game.pitch) = previous;
    angles
}

fn look_hits(game: &Game<'_>, pos: Pos) -> bool {
    let Some(eye) = game.eye_position() else { return false; };
    let direction = super::super::pick_view_vector(game.yaw, game.pitch);
    game.clip_ray(eye, direction, game.reach()).is_some_and(|hit| hit.pos == pos)
        && game.pick_entity_hit().is_none()
}

fn step_shaft(game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos) -> Result<(), String> {
    let shaft = job.shaft.as_ref().expect("active shaft");
    let direction = shaft.direction;
    if let Some(to) = shaft.pending_landing {
        job.action = Action::ShaftLanding { to, deadline: game.ticks + 100 };
        return Ok(());
    }
    if !ground(game) { return Ok(()); }
    if here != shaft.stand || !shaft_is_centered(game, shaft.stand) {
        return Err("Return to the center of the last shaft block, then resume.".into());
    }
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    world.respect_area = false;
    if !shaft_clearance(game, &mut world, here, direction) {
        return Err("Vertical digging paused for unsafe ground, an opening, falling blocks, or liquid.".into());
    }
    if direction == ShaftDirection::Up && shaft_block_slot(game).is_none() {
        return Err("Digging up needs cobblestone or cobbled deepslate in your hotbar.".into());
    }
    let pos = up(here, if direction == ShaftDirection::Down { -1 } else { 2 });
    let cell = world.cell(pos);
    job.phase = match direction {
        ShaftDirection::Down => Phase::DiggingDown,
        ShaftDirection::Up => Phase::DiggingUp,
    };
    if direction == ShaftDirection::Up && cell.clear {
        if !world.cell(up(here, 1)).clear {
            return Err("The space above you needs two clear blocks for a jump.".into());
        }
        if turn(game, &mut job.look, game.yaw, 90.0, settings.turn_speed) {
            job.action = Action::ShaftJump { from: here, started: game.ticks };
            game.keys.press(KeyCode::Space, Source::Agent);
        }
        return Ok(());
    }
    if cell.clear || !route::can_break(&mut |block| world.cell(block), pos) {
        return Err("Vertical digging paused. The next block cannot be broken safely with your hotbar tools.".into());
    }
    job.action = Action::Aim { pos, target: cell.target, started: game.ticks };
    Ok(())
}

fn shaft_jump(
    game: &mut Game<'_>, settings: &Settings, job: &mut Job, from: Pos, started: u64,
) -> Result<(), String> {
    game.keys.release(&KeyCode::Space, Source::Agent);
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    world.respect_area = false;
    if !shaft_is_centered(game, from)
        || !shaft_clearance(game, &mut world, from, ShaftDirection::Up)
        || (0..=2).any(|dy| !world.cell(up(from, dy)).clear)
    {
        return Err("The space for the next pillar block is no longer safe.".into());
    }
    if game.ticks.saturating_sub(started) >= 14 {
        return Err("Could not jump high enough to place a pillar block.".into());
    }
    if game.position()[1] < f64::from(from[1]) + 1.005 {
        job.action = Action::ShaftJump { from, started };
        return Ok(());
    }
    let floor = up(from, -1);
    let Some(eye) = game.eye_position() else { return Err("Cannot see your pillar support.".into()); };
    let dir = super::super::pick_view_vector(game.yaw, game.pitch);
    if !game.clip_ray(eye, dir, game.reach()).is_some_and(|hit| {
        hit.pos == floor && hit.face == [0, 1, 0]
    }) || game.pick_entity_hit().is_some() || game.right {
        return Err("The top of your pillar support must be clear and in reach.".into());
    }
    let slot = shaft_block_slot(game).ok_or("Put cobblestone or cobbled deepslate in your hotbar.")?;
    game.selected = slot;
    let pressed = game.agent_mouse(MouseButton::Right, true);
    let released = game.agent_mouse(MouseButton::Right, false);
    if game.settings.toggle_use {
        game.toggles.button(3, false, false, false);
        game.right = false;
    }
    let to = up(from, 1);
    job.shaft.as_mut().expect("active shaft").pending_landing = Some(to);
    job.action = Action::ShaftLanding { to, deadline: game.ticks + 100 };
    if pressed.is_err() || released.is_err() {
        return Err("Horizon could not place the pillar block.".into());
    }
    Ok(())
}

fn shaft_landing(game: &Game<'_>, job: &mut Job, here: Pos, to: Pos, deadline: u64) -> Result<(), String> {
    let shaft = job.shaft.as_mut().expect("active shaft");
    let support = up(to, -1);
    if here == to && ground(game) && shaft_is_centered(game, to) && solid_shaft_support(game, support)
        && (shaft.direction == ShaftDirection::Down
            || facts_at(game, support).is_some_and(|facts| pillar_material(&facts.name)))
    {
        shaft.stand = to;
        shaft.travelled += 1;
        shaft.pending_landing = None;
        job.last_progress = Instant::now();
        return Ok(());
    }
    if ground(game) && here != shaft.stand && here != to {
        return Err("You moved out of the vertical shaft. Move back to the expected landing and resume.".into());
    }
    if game.ticks >= deadline {
        if ground(game) && here == shaft.stand && !dihrill_remote::block_pending(game, support) {
            shaft.pending_landing = None;
        }
        return Err("The vertical step did not finish. Check your footing or server block permissions.".into());
    }
    job.action = Action::ShaftLanding { to, deadline };
    Ok(())
}

fn search_options(game: &Game<'_>, settings: &Settings, job: &Job) -> SearchOptions {
    SearchOptions {
        tunnel: !job.find_only && settings.profile != Profile::Cave,
        find_only: job.find_only,
        max_fall: settings.max_fall,
        blocked_edges: job.blocked_edges.clone(),
        blocked_approaches: job.blocked_approaches.clone(),
        junk_pickup_risk: junk_pickup_risk(game, settings, job),
        area: (settings.profile == Profile::Area)
            .then(|| settings.area.map(|(a,b)| [a,b])).flatten(),
        ..SearchOptions::default()
    }
}

fn setback(job: &mut Job, now: u64) -> Result<(), String> {
    job.failures = job.failures.saturating_add(1);
    if job.failures >= MAX_FAILURES {
        if job.shaft.is_some() || job.storage.is_some() {
            return Err("Paused after repeated blocked routes or actions. Check the area and resume.".into());
        }
        job.schedule_rescan(now, "Repeated route obstructions. Waiting for a fresh scan.");
    } else {
        job.replan(now);
    }
    Ok(())
}

fn reject_break(
    job: &mut Job, now: u64, pos: Pos, stand: Pos, target: bool, rejected_by_server: bool,
) -> Result<(), String> {
    if rejected_by_server || !target {
        if job.blocked.len() < MAX_BLOCKED { job.blocked.insert(pos); }
    } else if job.blocked_approaches.len() < MAX_BLOCKED {
        job.blocked_approaches.insert((stand, pos));
    }
    setback(job, now)
}

fn targets_may_be_present(game: &Game<'_>, settings: &Settings, here: Pos) -> bool {
    let radius = settings.radius;
    let bounds = [radius, radius.min(12), radius];
    let mut lo = std::array::from_fn(|i| here[i] - bounds[i]);
    let mut hi = std::array::from_fn(|i| here[i] + bounds[i]);
    lo[1] = lo[1].max(game.world.bottom_y());
    hi[1] = hi[1].min(game.world.top_y() - 1);
    if settings.profile == Profile::Area && let Some((a, b)) = settings.area {
        lo = std::array::from_fn(|i| lo[i].max(a[i]));
        hi = std::array::from_fn(|i| hi[i].min(b[i]));
    }
    if (0..3).any(|i| lo[i] > hi[i]) {
        return false;
    }
    // Global palettes conservatively return true. A false result is enough
    // to skip the expensive route search without overlooking a loaded target.
    let mut matches = HashMap::new();
    game.world.may_hold(lo, hi, |state| {
        *matches.entry(state).or_insert_with(|| {
            let facts = game.res.facts().get(state);
            settings.targets.iter().any(|id| id == facts.name.as_ref())
        })
    })
}

fn scan_next(game: &Game<'_>, settings: &Settings, job: &mut Job, here: Pos) -> Result<(), String> {
    job.retry_reason = None;
    if job.scan.is_none() {
        let mut remaining_vein = None;
        if let Some(vein) = &mut job.vein {
            let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
            if !vein.advance(96, |pos| world.cell(pos)) {
                job.phase = Phase::Scanning;
                return Ok(());
            }
            let remaining = vein.remaining();
            if remaining.is_empty() {
                job.vein = None;
            } else {
                remaining_vein = Some(remaining);
            }
        }
        if remaining_vein.is_none() && !targets_may_be_present(game, settings, here) {
            if !job.deep_scan && settings.explore && settings.profile != Profile::Area {
                begin_explore(game, settings, job, here)?;
            } else {
                job.schedule_rescan(game.ticks, "No selected blocks in loaded terrain. Will rescan.");
            }
        } else {
            let mut options = search_options(game, settings, job);
            options.vein = remaining_vein;
            if !job.deep_scan && options.vein.is_none() && can_explore(settings, job) {
                options.target_node_limit = Some(FAST_SEARCH_NODES);
            }
            job.phase = Phase::Selecting;
            job.exploring_scan = false;
            job.scan = Some(Search::with_options(here, settings.radius, options));
        }
    }
    let (result, missing_tool) = {
        let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
        let Some(scan) = &mut job.scan else { return Ok(()); };
        let started = Instant::now();
        let mut result = SearchResult::Pending;
        for _ in 0..ROUTE_NODES_PER_TICK / ROUTE_BATCH_NODES {
            result = scan.advance(ROUTE_BATCH_NODES, |pos| world.cell(pos));
            if !matches!(result, SearchResult::Pending) || started.elapsed() >= ROUTE_SEARCH_SLICE {
                break;
            }
        }
        (result, world.missing_target_tool)
    };
    job.missing_target_tool |= missing_tool;
    match result {
        SearchResult::Pending => {
            if !job.exploring_scan {
                job.phase = Phase::Planning;
            }
        }
        SearchResult::Found(path) => {
            job.scan = None;
            if path.kind == RouteKind::Target && !job.find_only && job.vein.is_none() {
                let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
                let family = world.cell(path.target).vein;
                if family != 0 {
                    let mut vein = Vein::new(path.target, family, settings.diagonals);
                    vein.advance(96, |pos| world.cell(pos));
                    job.vein = Some(vein);
                }
            }
            job.route = Some(path);
        }
        SearchResult::Exhausted => {
            job.scan = None;
            if job.vein.take().is_some() {
                job.phase = Phase::Selecting;
            } else if std::mem::take(&mut job.exploring_scan) {
                if settings.profile == Profile::Branch && job.branch != Branch::Main {
                    job.branch = Branch::Main;
                    job.branch_index = job.branch_index.saturating_add(1);
                } else {
                    job.deep_scan = true;
                    job.phase = Phase::Selecting;
                }
            } else if !job.deep_scan && can_explore(settings, job) {
                begin_explore(game, settings, job, here)?;
            } else if job.missing_target_tool && !job.find_only {
                return Err("A nearby target needs a suitable hotbar tool above the durability limit.".into());
            } else if job.deep_scan {
                job.schedule_rescan(game.ticks, "No safe route in loaded terrain. Will rescan.");
            } else if settings.explore && settings.profile != Profile::Area {
                begin_explore(game, settings, job, here)?;
            } else {
                job.schedule_rescan(game.ticks, "No reachable selected blocks in loaded terrain. Will rescan.");
            }
        }
    }
    Ok(())
}

fn branch_waypoint(job: &mut Job, settings: &Settings, here: Pos) -> Pos {
    let [hx, hz] = job.heading;
    let spacing = settings.branch_spacing as i32;
    let progress = ((here[0] - job.origin[0]) * hx + (here[2] - job.origin[2]) * hz).max(0);
    if job.branch == Branch::Main && progress >= spacing * job.branch_index as i32 {
        job.branch = Branch::Out;
    }
    let station = spacing * job.branch_index as i32;
    let base = [job.origin[0] + hx * station, here[1], job.origin[2] + hz * station];
    if job.branch == Branch::Out {
        let sign = if job.branch_index % 2 == 0 { -1 } else { 1 };
        [base[0] - hz * spacing * sign, base[1], base[2] + hx * spacing * sign]
    } else {
        base
    }
}

fn can_explore(settings: &Settings, job: &Job) -> bool {
    settings.explore && settings.profile != Profile::Area
        && (settings.continuous_explore || job.explored_steps < settings.explore_limit)
}

fn begin_explore(game: &Game<'_>, settings: &Settings, job: &mut Job, here: Pos) -> Result<(), String> {
    let remaining = if settings.continuous_explore { 10 }
        else { settings.explore_limit.saturating_sub(job.explored_steps) };
    if remaining == 0 {
        job.schedule_rescan(game.ticks, "Exploration limit reached. Waiting for reachable targets.");
        return Ok(());
    }
    let [hx, hz] = job.heading;
    let mut options = search_options(game, settings, job);
    let profile = if job.find_only { Profile::Cave } else { settings.profile };
    let tunnel = matches!(profile, Profile::Tunnel | Profile::Branch | Profile::Hunter | Profile::Quantity);
    if matches!(profile, Profile::Tunnel | Profile::Branch) {
        options.tunnel_height = settings.tunnel_height;
        options.tunnel_width = settings.tunnel_width;
    }
    let forward = [here[0] + hx * 6, here[1], here[2] + hz * 6];
    let preferred = match profile {
        Profile::Tunnel => Some(forward),
        Profile::Branch => Some(branch_waypoint(job, settings, here)),
        _ => None,
    };
    let radius = settings.radius.min(10).min(remaining as i32).max(1);
    if profile == Profile::Branch && job.branch == Branch::Return {
        let target = preferred.expect("branch has a waypoint");
        options.tunnel = false;
        options.tunnel_height = 2;
        options.tunnel_width = 1;
        options.goal = Some(target);
        job.scan = Some(Search::with_options(here, settings.radius, options));
    } else {
        let heading = preferred.map_or(job.heading, |p| [(p[0] - here[0]).signum(), (p[2] - here[2]).signum()]);
        let visited = job.visits.iter()
            .filter(|(p, _)| p[0].abs_diff(here[0]) <= radius as u32 && p[2].abs_diff(here[2]) <= radius as u32)
            .map(|(&pos, &count)| (pos, count)).collect();
        job.scan = Some(Search::explore(here, radius, options, ExploreOptions {
            heading, visited, tunnel, preferred,
        }));
    }
    job.exploring_scan = true;
    job.phase = Phase::Exploring;
    Ok(())
}

fn follow_route(game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos) -> Result<bool, String> {
    let Some(path) = &job.route else { return Ok(false); };
    let now = game.ticks;
    let kind = path.kind;
    let target = path.target;
    let next_step = path.steps.front().copied();
    if kind == RouteKind::Target && !job.find_only && next_step.is_some()
        && begin_visible_mining(game, settings, job, here, Some(target))
    {
        return Ok(true);
    }
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    if kind == RouteKind::Target && !world.cell(target).target {
        job.replan(now);
        return Ok(true);
    }
    if let Some(step) = next_step {
        let (height, width) = if kind == RouteKind::Explore && matches!(settings.profile, Profile::Tunnel | Profile::Branch) {
            (settings.tunnel_height, settings.tunnel_width)
        } else { (2, 1) };
        let tunnel = !job.find_only && kind != RouteKind::Destination && settings.profile != Profile::Cave;
        let Some((verified, _)) = route::transition_configured(
            &mut |pos| world.cell(pos), here, step.to, tunnel, settings.max_fall, height, width,
        ) else {
            if job.blocked_edges.len() < MAX_BLOCKED { job.blocked_edges.insert((here, step.to)); }
            setback(job, now)?;
            return Ok(true);
        };
        if let Some(pos) = verified.dig.into_iter().flatten().next() {
            job.phase = Phase::Clearing;
            job.action = Action::Aim { pos, target: false, started: now };
        } else {
            job.phase = if kind == RouteKind::Explore { Phase::Exploring } else { Phase::Navigating };
            job.action = Action::Walk { step: verified, started: now, purpose: WalkPurpose::Route };
        }
    } else {
        job.route = None;
        match kind {
            RouteKind::Target if job.find_only => {
                if game.aim_block(target, None).is_ok() {
                    job.found = Some(target);
                } else {
                    if job.blocked_approaches.len() < MAX_BLOCKED {
                        job.blocked_approaches.insert((here, target));
                    }
                    setback(job, now)?;
                }
            }
            RouteKind::Target => {
                job.phase = Phase::Mining;
                job.action = Action::Aim { pos: target, target: true, started: now };
            }
            RouteKind::Explore => {
                if settings.profile == Profile::Branch && job.branch == Branch::Out {
                    let preferred = branch_waypoint(job, settings, here);
                    if preferred[0].abs_diff(here[0]) + preferred[2].abs_diff(here[2]) <= 2 {
                        job.branch = Branch::Return;
                    }
                }
                job.phase = Phase::Scanning;
            }
            RouteKind::Destination => {
                if settings.profile == Profile::Branch && job.branch == Branch::Return && job.storage.is_none() && job.pickup.is_none() {
                    job.branch = Branch::Main;
                    job.branch_index = job.branch_index.saturating_add(1);
                }
            }
        }
    }
    Ok(true)
}

fn straight_with(step: Step, next: Step) -> bool {
    step.to[1] == step.from[1] && next.to[1] == next.from[1]
        && next.to[0] - next.from[0] == step.to[0] - step.from[0]
        && next.to[2] - next.from[2] == step.to[2] - step.from[2]
        && next.dig.iter().all(Option::is_none)
}

fn step_walk(
    game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos,
    step: Step, started: u64, purpose: WalkPurpose,
) -> Result<(), String> {
    let [x,y,z] = game.position();
    let dx = f64::from(step.to[0]) + 0.5 - x;
    let dz = f64::from(step.to[2]) + 0.5 - z;
    let distance = dx.hypot(dz);
    let now = game.ticks;
    if distance < 0.31 && (y - f64::from(step.to[1])).abs() < 0.24 && ground(game) {
        job.walk_progress = None;
        if purpose == WalkPurpose::Route {
            if job.recent_stands.back() != Some(&step.from) {
                job.recent_stands.clear();
                job.recent_stands.push_back(step.from);
            }
            job.recent_stands.push_back(step.to);
            if job.recent_stands.len() > 32 { job.recent_stands.pop_front(); }
        } else if purpose == WalkPurpose::MobRetreat {
            while job.recent_stands.back().is_some_and(|&pos| pos != step.to) {
                job.recent_stands.pop_back();
            }
            if job.recent_stands.is_empty() { job.recent_stands.push_back(step.to); }
            let direction = std::array::from_fn(|axis| step.to[axis] - step.from[axis]);
            if settings.retreat_barricade && !job.find_only && let Some(evasion) = &mut job.evasion
                && matches!(evasion.wall, EscapeWall::Ready)
                && let Some(wall) = escape_wall_site(game, settings, step.to, direction, 3.2)
            {
                release_walk(game);
                evasion.wall = wall;
                return Ok(());
            }
            if nearest_threat(game, settings).is_some()
                && let Some(next) = escape_step(game, settings, job, here, None)
            {
                if straight_with(step, next) {
                    return step_walk(game, settings, job, here, next, now, purpose);
                }
                job.action = Action::Walk { step: next, started: now, purpose };
            }
            release_walk(game);
            return Ok(());
        }
        job.visit(step.to);
        if purpose == WalkPurpose::Route { job.deep_scan = false; }
        if purpose == WalkPurpose::Route && let Some(path) = &mut job.route {
            path.steps.pop_front();
            if path.kind == RouteKind::Explore {
                job.explored_steps = job.explored_steps.saturating_add(1);
                if !settings.continuous_explore && job.explored_steps >= settings.explore_limit {
                    job.route = None;
                }
            }
        }
        if purpose == WalkPurpose::Retreat {
            release_walk(game);
            return Err("Retreated to the previous safe stand. Review the area and resume.".into());
        }
        job.failures = 0;
        if job.route.as_ref().and_then(|route| route.steps.front())
            .is_some_and(|next| straight_with(step, *next))
        {
            follow_route(game, settings, job, here)?;
            if let Action::Walk { step: next, started, purpose } = job.action
                && straight_with(step, next)
            {
                return step_walk(game, settings, job, here, next, started, purpose);
            }
        }
        release_walk(game);
        return Ok(());
    }
    let safe = {
        let mob_zones = if purpose == WalkPurpose::MobRetreat { &[][..] } else { &job.mob_zones };
        let mut world = WorldRead::new(game, settings, &job.blocked, mob_zones);
        route::transition_with_fall(
            &mut |pos| world.cell(pos), step.from, step.to, false, settings.max_fall,
        ).is_some()
    };
    let in_corridor = [step.from, step.to].iter().any(|p| {
        p[0] == here[0] && p[2] == here[2]
            && (step.from[1].min(step.to[1])..=step.from[1].max(step.to[1]) + 1).contains(&here[1])
    });
    let progress = job.walk_progress.get_or_insert((step.to, distance, now));
    if progress.0 != step.to || distance + 0.075 < progress.1 {
        *progress = (step.to, distance, now);
    }
    let mob_ahead = purpose == WalkPurpose::MobRetreat && here != step.to
        && safer_from_mobs(game, settings, step.to).is_none();
    if !safe || !in_corridor || mob_ahead || now.saturating_sub(progress.2) > 34
        || now.saturating_sub(started) > 105
    {
        release_walk(game);
        if purpose == WalkPurpose::MobRetreat {
            if let Some(evasion) = &mut job.evasion {
                evasion.failed_destination = Some(step.to);
                evasion.retry_at = now + 5;
            }
            return Ok(());
        }
        if purpose != WalkPurpose::Route {
            return Err("Paused because the retreat route is no longer safe.".into());
        }
        if purpose == WalkPurpose::Route {
            if job.blocked_edges.len() < MAX_BLOCKED { job.blocked_edges.insert((step.from, step.to)); }
            setback(job, now)?;
        }
        return Ok(());
    }
    let ahead = job.route.as_ref().map_or(0, |path| {
        if purpose != WalkPurpose::Route { return 0; }
        let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
        path.steps.iter().skip(1).take(2).take_while(|next| {
            straight_with(step, **next)
                && route::transition_with_fall(
                    &mut |p| world.cell(p), next.from, next.to, false, settings.max_fall,
                ).is_some()
        }).count()
    });
    let forward_x = f64::from(step.to[0] - step.from[0]);
    let forward_z = f64::from(step.to[2] - step.from[2]);
    // A short look-ahead on a straight leg keeps small lateral corrections
    // from swinging the camera as the player approaches the next block center.
    let look_ahead = ahead as f64 * 0.8;
    let yaw = (-(dx + forward_x * look_ahead))
        .atan2(dz + forward_z * look_ahead).to_degrees() as f32;
    let pitch = match step.to[1].cmp(&step.from[1]) {
        std::cmp::Ordering::Less => 15.0,
        std::cmp::Ordering::Equal => 4.0,
        std::cmp::Ordering::Greater => -3.0,
    };
    let turn_speed = if purpose == WalkPurpose::MobRetreat {
        settings.turn_speed.max(34)
    } else {
        settings.turn_speed
    };
    turn(game, &mut job.look, yaw, pitch, turn_speed);
    let yaw_tolerance = if settings.pace == Pace::Calm || ahead == 0 { 9.0 } else { 16.0 };
    let aligned = distance > 0.23 && angle_delta(game.yaw, yaw).abs() <= yaw_tolerance;
    if aligned {
        job.travel_yaw = Some(yaw);
        game.keys.press(KeyCode::KeyW, Source::Agent);
    } else {
        game.keys.release(&KeyCode::KeyW, Source::Agent);
    }
    let jump = aligned && ground(game) && step.to[1] > step.from[1] && y < f64::from(step.to[1]) - 0.02;
    if jump { game.keys.press(KeyCode::Space, Source::Agent); }
    else { game.keys.release(&KeyCode::Space, Source::Agent); }
    let well_fed = game.player.as_ref().is_some_and(|player| player.health.food.food >= 10);
    let dry = [here,step.to].into_iter().all(|p| facts_at(game,p).is_some_and(|f| f.fluid == Fluid::None));
    let sprint = purpose == WalkPurpose::MobRetreat
        || (settings.sprint && settings.pace == Pace::Normal && ahead == 2);
    if sprint && aligned && well_fed && dry {
        game.keys.press(KeyCode::ControlLeft, Source::Agent);
    } else {
        game.keys.release(&KeyCode::ControlLeft, Source::Agent);
    }
    job.action = Action::Walk { step, started, purpose };
    Ok(())
}

fn step_aim(
    game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos, pos: Pos, target: bool, started: u64,
) -> Result<(), String> {
    let now = game.ticks;
    if !ground(game) {
        if now.saturating_sub(started) > 55 { return reject_break(job, now, pos, here, target, false); }
        job.action = Action::Aim { pos, target, started };
        return Ok(());
    }
    if facts_at(game, here).is_none_or(|f| f.fluid != Fluid::None) {
        return reject_break(job, now, pos, here, target, false);
    }
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    if let Some(shaft) = &job.shaft {
        world.respect_area = false;
        if here != shaft.stand || !shaft_is_centered(game, here)
            || !shaft_clearance(game, &mut world, here, shaft.direction)
        {
            return Err("Vertical mining stopped because the shaft is no longer safe.".into());
        }
    }
    let entry = world.cell(pos);
    if world.missing_target_tool {
        return Err("No suitable hotbar tool above the durability limit.".into());
    }
    if !route::can_break(&mut |p| world.cell(p), pos) || entry.target != target {
        return reject_break(job, now, pos, here, target, false);
    }
    let Some(before) = state_at(game, pos) else { return reject_break(job, now, pos, here, target, false); };
    let facts = game.res.facts().get(before);
    let Some(tool) = choose_tool(game, settings, facts, target) else {
        return Err("No suitable hotbar tool above the durability limit.".into());
    };
    if now.saturating_sub(started) > 60 {
        return reject_break(job, now, pos, here, target, false);
    }
    if !look_hits(game, pos) {
        let Some((yaw,pitch)) = target_angles(game, pos) else {
            return reject_break(job, now, pos, here, target, false);
        };
        turn(game, &mut job.look, yaw, pitch, settings.turn_speed);
        if !look_hits(game, pos) {
            job.action = Action::Aim { pos, target, started };
            return Ok(());
        }
    }
    job.look = Look::default();
    if game.left {
        return Err("Paused for manual mining input.".into());
    }
    game.selected = tool.slot;
    if game.agent_mouse(MouseButton::Left, true).is_err() {
        return reject_break(job, now, pos, here, target, false);
    }
    job.attack = true;
    let timeout = (u64::from(tool.ticks) * 8 + 80).clamp(120, 6_000);
    job.action = Action::Mine { pos, before, slot: tool.slot, started: now, timeout, target };
    Ok(())
}

fn mined_block(game: &Game<'_>, job: &mut Job, pos: Pos, target: bool, before: BlockStateId) {
    job.last_progress = Instant::now();
    job.retry_count = 0;
    if target {
        let block = Arc::clone(&game.res.facts().get(before).name);
        job.drops.push_back(DropBatch {
            source: pos,
            block,
            spawn_until: game.ticks + DROP_SPAWN_TICKS,
            deadline: None,
            tried: HashSet::new(),
            skipped: HashSet::new(),
        });
        job.totals.mined = job.totals.mined.saturating_add(1);
        if let Some(vein) = &mut job.vein {
            vein.mark_mined(pos);
            vein.refresh(pos);
        }
    } else {
        job.totals.tunneled = job.totals.tunneled.saturating_add(1);
    }
    job.failures = 0;
    job.phase = Phase::Collecting;
    job.action = if target { Action::Collect { pos } } else { Action::Ready };
    if let Some(shaft) = &mut job.shaft {
        job.phase = match shaft.direction {
            ShaftDirection::Down => Phase::DiggingDown,
            ShaftDirection::Up => Phase::DiggingUp,
        };
        job.action = Action::Ready;
        if shaft.direction == ShaftDirection::Down {
            shaft.pending_landing = Some(pos);
        }
    }
}

fn step_mine(
    game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos, action: Action,
) -> Result<(), String> {
    let Action::Mine { pos, before, slot, started, timeout, target } = action else { unreachable!(); };
    let now = game.ticks;
    let state = state_at(game, pos);
    if state.is_some_and(|id| game.res.facts().is_air(id)) {
        release_attack(game, &mut job.attack);
        if dihrill_remote::block_pending(game, pos) {
            job.action = Action::Confirm { pos, deadline: now + 100, target, before };
        } else {
            mined_block(game, job, pos, target, before);
        }
        return Ok(());
    }
    let same_block = state.is_some_and(|id| game.res.facts().get(id).name == game.res.facts().get(before).name);
    if !same_block || now.saturating_sub(started) > timeout {
        release_attack(game, &mut job.attack);
        return reject_break(job, now, pos, here, target, true);
    }
    if game.selected != slot {
        return Err("Paused because the selected hotbar slot changed.".into());
    }
    if game.inventory[slot].as_ref().and_then(|stack| durability(game, stack))
        .is_some_and(|(remaining, _)| remaining <= settings.durability)
    {
        release_attack(game, &mut job.attack);
        job.replan(now);
        return Ok(());
    }
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    if let Some(shaft) = &job.shaft {
        world.respect_area = false;
        if here != shaft.stand || !shaft_is_centered(game, here)
            || !shaft_clearance(game, &mut world, here, shaft.direction)
        {
            return Err("Vertical mining stopped because the shaft is no longer safe.".into());
        }
    }
    if !ground(game) || !route::can_break(&mut |p| world.cell(p), pos) {
        release_attack(game, &mut job.attack);
        return reject_break(job, now, pos, here, target, false);
    }
    if !look_hits(game, pos) {
        release_attack(game, &mut job.attack);
        if target_angles(game, pos).is_none() {
            return reject_break(job, now, pos, here, target, false);
        }
        job.action = Action::Aim { pos, target, started };
        return Ok(());
    }
    job.action = action;
    Ok(())
}

fn begin_visible_mining(
    game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos, planned: Option<Pos>,
) -> bool {
    if job.shaft.is_some() || !ground(game) || goal_met(settings, job)
        || job.drops.len() >= MAX_QUEUED_DROPS
    {
        return false;
    }
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    let stand = world.cell(here);
    if stand.hazard || stand.cave || !stand.clear {
        return false;
    }
    if let Some(vein) = &mut job.vein {
        vein.advance(96, |pos| world.cell(pos));
    }
    let (yaw, pitch) = (game.yaw, game.pitch);
    let eye = game.eye_position().unwrap_or_else(|| game.position());
    let reach = game.reach() + 0.87;
    let vein_blocks = job.vein.as_ref().into_iter().flat_map(Vein::positions);
    let next = planned.into_iter().chain(vein_blocks).filter_map(|pos| {
        let distance = (0..3).map(|axis| (f64::from(pos[axis]) + 0.5 - eye[axis]).powi(2))
            .sum::<f64>().sqrt();
        if distance > reach || job.blocked_approaches.contains(&(here, pos)) {
            return None;
        }
        let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
        if !world.cell(pos).target || !route::can_break(&mut |p| world.cell(p), pos) {
            return None;
        }
        let (next_yaw, next_pitch) = target_angles(game, pos)?;
        let sweep = angle_delta(yaw, next_yaw).hypot(next_pitch - pitch);
        Some((f64::from(sweep) + distance * 8.0, pos))
    }).min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, pos)| pos);
    let Some(pos) = next else { return false; };
    job.route = None;
    job.phase = Phase::Mining;
    job.action = Action::Aim { pos, target: true, started: game.ticks };
    release_walk(game);
    true
}

fn pickup_room(game: &Game<'_>, stack: &ItemStack) -> bool {
    let limit = game.res.items().get(&stack.item).map_or(64, |item| item.max_stack);
    game.inventory[..36].iter().any(Option::is_none)
        || game.inventory[..36].iter().flatten().chain(game.offhand.as_ref()).any(|held| {
            held.count < limit && held.same_item_same_components(stack)
        })
}

fn can_touch_drop(game: &Game<'_>, player: [f64; 3], drop: [f64; 3]) -> bool {
    let Some(local) = game.player.as_ref() else { return false; };
    let range = f64::from(local.dimensions.width) / 2.0 + 1.0;
    (drop[0] - player[0]).abs() < range && (drop[2] - player[2]).abs() < range
        && (-0.50..f64::from(local.dimensions.height) + 0.50).contains(&(drop[1] - player[1]))
}

fn clear_pickup(job: &mut Job) {
    job.pickup = None;
    job.route = None;
    job.scan = None;
}

fn collect_drops(game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos) -> Result<(), String> {
    if game.ticks < job.retry_at { return Ok(()); }
    job.phase = Phase::Collecting;
    let now = game.ticks;
    let batch = job.drops.front_mut().expect("pending mined block");
    let source = batch.source;
    let deadline = *batch.deadline.get_or_insert(now + COLLECTION_TICKS);
    let center = source.map(|v| f64::from(v) + 0.5);
    let point = |item: &horizon_client::entities::ItemEntity| {
        [item.position.x, item.position.y, item.position.z]
    };
    let matches_batch = |item: &&horizon_client::entities::ItemEntity| {
        let item_id = item.stack.item.as_ref();
        let [x, y, z] = point(item);
        let dxz = (center[0] - x).hypot(center[2] - z);
        (item_id == batch.block.as_ref() || drop_item(&batch.block) == Some(item_id))
            && dxz <= 3.0 && (-5.0..=2.5).contains(&(y - center[1]))
    };

    let drop = game.dropped.iter().filter(matches_batch)
        .filter(|item| !batch.skipped.contains(&item.id))
        .min_by_key(|item| (job.pickup.is_none_or(|pickup| pickup.entity != item.id), item.id));
    let Some(drop) = drop else {
        if now < batch.spawn_until { return Ok(()); }
        job.drops.pop_front();
        clear_pickup(job);
        return Ok(());
    };
    if now >= deadline {
        say(game, &format!("Could not safely collect every drop near {}, {}, {}.", source[0], source[1], source[2]));
        job.drops.pop_front();
        clear_pickup(job);
        return Ok(());
    }
    let entity = drop.id;
    let at = point(drop);
    if !pickup_room(game, &drop.stack) {
        job.drops.front_mut().expect("pending mined block").deadline = None;
        clear_pickup(job);
        if try_shop(game, settings, job) { return Ok(()); }
        maybe_make_room(game, settings, job)?;
        return Ok(());
    }
    if job.pickup.is_none_or(|pickup| pickup.entity != entity) {
        clear_pickup(job);
    }
    if let Some(pickup) = job.pickup {
        if let Some(until) = pickup.wait_until {
            if now >= until {
                job.drops.front_mut().expect("pending mined block").tried.insert((entity, pickup.stand));
                clear_pickup(job);
            }
            return Ok(());
        }
        if can_touch_drop(game, game.position(), at) || here == pickup.stand && job.route.as_ref().is_some_and(|route| route.steps.is_empty()) {
            job.route = None;
            job.scan = None;
            job.pickup = Some(Pickup { stand: here, wait_until: Some(now + PICKUP_WAIT_TICKS), ..pickup });
            return Ok(());
        }
        if job.route.is_some() {
            follow_route(game, settings, job, here)?;
            return Ok(());
        }
        if let Some(scan) = &mut job.scan {
            let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
            match scan.advance(110, |pos| world.cell(pos)) {
                SearchResult::Pending => (),
                SearchResult::Found(path) => {
                    job.scan = None;
                    job.route = Some(path);
                }
                SearchResult::Exhausted => {
                    job.drops.front_mut().expect("pending mined block").tried.insert((entity, pickup.stand));
                    clear_pickup(job);
                }
            }
            return Ok(());
        }
    }

    let batch = job.drops.front_mut().expect("pending mined block");
    if can_touch_drop(game, game.position(), at) && !batch.tried.contains(&(entity, here)) {
        job.pickup = Some(Pickup { entity, stand: here, wait_until: Some(now + PICKUP_WAIT_TICKS) });
        return Ok(());
    }
    let landing = [at[0].floor() as i32, (at[1] + 0.05).floor() as i32, at[2].floor() as i32];
    let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
    let mut best = None;
    for dx in -1..=1 {
        for dz in -1..=1 {
            for y in landing[1] - 2..=landing[1] + 1 {
                let stand = [landing[0] + dx, y, landing[2] + dz];
                let feet = [f64::from(stand[0]) + 0.5, f64::from(y), f64::from(stand[2]) + 0.5];
                if stand == here || batch.tried.contains(&(entity, stand))
                    || (0..3).any(|i| stand[i].abs_diff(here[i]) > PICKUP_RADIUS as u32)
                    || !can_touch_drop(game, feet, at)
                {
                    continue;
                }
                let floor = world.cell(up(stand, -1));
                if !floor.floor || floor.hazard || floor.falling || ![stand, up(stand, 1)].into_iter().all(|p| {
                    let cell = world.cell(p);
                    cell.clear && !cell.cave && route::dry_margin(&mut |p| world.cell(p), p)
                }) {
                    continue;
                }
                let distance = here[0].abs_diff(stand[0]) + here[2].abs_diff(stand[2])
                    + here[1].abs_diff(stand[1]) * 2;
                if best.is_none_or(|choice| (distance, stand) < choice) {
                    best = Some((distance, stand));
                }
            }
        }
    }
    let Some((_, stand)) = best else {
        if now >= batch.spawn_until && !can_touch_drop(game, game.position(), at) {
            batch.skipped.insert(entity);
            say(game, &format!("A drop near {}, {}, {} is outside safe pickup reach.", source[0], source[1], source[2]));
        }
        return Ok(());
    };
    let mut options = search_options(game, settings, job);
    options.tunnel = false;
    options.goal = Some(stand);
    options.area = None;
    job.pickup = Some(Pickup { entity, stand, wait_until: None });
    job.scan = Some(Search::with_options(here, PICKUP_RADIUS, options));
    Ok(())
}

fn try_shop(game: &mut Game<'_>, settings: &Settings, job: &mut Job) -> bool {
    let now = game.ticks;
    if job.shop_unavailable
        || job.last_shop.is_some_and(|last| now.saturating_sub(last) < 20 * u64::from(settings.shop_interval_seconds))
        || settings.protected.iter().any(|item| item == shop::DIAMOND)
    {
        return false;
    }
    let Some(visit) = settings.diamond_shop.as_ref()
        .and_then(|menu| Visit::start(game, menu, settings.shop_min_diamonds))
    else { return false; };
    release(game, Some(job));
    job.shop = Some(visit);
    job.last_shop = Some(now);
    job.phase = Phase::Selling;
    true
}

fn step_storage(
    game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos,
) -> Result<(), String> {
    let mut storage = job.storage.take().expect("storage operation");
    let result = advance_storage(game, settings, job, here, &mut storage);
    if !matches!(result, Ok(true)) { job.storage = Some(storage); }
    result.map(|_| ())
}

fn advance_storage(
    game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos, storage: &mut Storage,
) -> Result<bool, String> {
    let pos = storage.pos;
    let now = game.ticks;
    job.phase = Phase::Inventory;
    if !storage_available(game, pos) {
        return Err("The configured storage changed or unloaded.".into());
    }
    match storage.phase {
        StoragePhase::Navigate => {
            if game.screen != Screen::Playing {
                return Err("Close the open screen to continue to storage.".into());
            }
            if let Some((yaw, pitch)) = target_angles(game, pos) {
                if turn(game, &mut job.look, yaw, pitch, settings.turn_speed) && look_hits(game, pos) {
                    if game.right {
                        return Err("Release the use button before opening storage.".into());
                    }
                    game.agent_mouse(MouseButton::Right, true)
                        .and_then(|_| game.agent_mouse(MouseButton::Right, false))
                        .map_err(|_| "Could not interact with the configured storage.".to_owned())?;
                    storage.phase = StoragePhase::AwaitOpen { deadline: now + 70 };
                    job.route = None;
                    job.scan = None;
                }
                return Ok(false);
            }
            if job.route.is_some() {
                follow_route(game, settings, job, here)?;
                return Ok(false);
            }
            if storage.stand.is_none() {
                let mut stands = Vec::with_capacity(12);
                for offset in route::SIDES {
                    for dy in [0, 1, -1] {
                        let candidate = up(route::plus(pos, offset), dy);
                        if storage.attempted.contains(&candidate) { continue; }
                        let clear = [candidate, up(candidate, 1)].into_iter().all(|p|
                            state_at(game, p).is_some_and(|id| game.res.facts().is_air(id)));
                        let supported = facts_at(game, up(candidate, -1)).is_some_and(|facts|
                            facts.full_collision && safe_block(facts));
                        if clear && supported { stands.push(candidate); }
                    }
                }
                let stand = stands.into_iter().min_by_key(|p| {
                    p[0].abs_diff(here[0]) + p[1].abs_diff(here[1]) + p[2].abs_diff(here[2])
                });
                let Some(stand) = stand else {
                    return Err("No reachable standing position beside storage.".into());
                };
                storage.stand = Some(stand);
                storage.attempted.insert(stand);
                let mut options = search_options(game, settings, job);
                options.tunnel = false;
                options.goal = Some(stand);
                options.area = None;
                job.scan = Some(Search::with_options(here, settings.radius, options));
            }
            if let Some(scan) = &mut job.scan {
                let mut world = WorldRead::new(game, settings, &job.blocked, &job.mob_zones);
                match scan.advance(210, |p| world.cell(p)) {
                    SearchResult::Pending => (),
                    SearchResult::Found(path) if path.steps.is_empty() => {
                        job.scan = None;
                        storage.stand = None;
                    }
                    SearchResult::Found(path) => {
                        job.scan = None;
                        job.route = Some(path);
                        storage.stand = None;
                    }
                    SearchResult::Exhausted => {
                        job.scan = None;
                        storage.stand = None;
                    }
                }
            }
        }
        StoragePhase::AwaitOpen { deadline } => {
            if let Some(window) = storage_window(game, pos) {
                storage.phase = StoragePhase::Deposit { window, ready: now + 5, next_slot: 9, moved: false };
            } else if now >= deadline || game.screen != Screen::Playing {
                return Err("Storage did not open as the configured single chest or barrel.".into());
            }
        }
        StoragePhase::Deposit { window, ready, next_slot, moved } => {
            if storage_window(game, pos) != Some(window) || game.carried.is_some() {
                return Err("Storage was closed or changed while depositing items.".into());
            }
            if now >= ready {
                let free = game.inventory[..36].iter().filter(|stack| stack.is_none()).count();
                let next = (next_slot..36).find(|&slot| {
                    game.inventory[slot].as_ref().is_some_and(|stack| storable(game, settings, stack))
                });
                if free >= 6 || next.is_none() {
                    storage.phase = StoragePhase::Finish { ready: now + 8, moved };
                } else if let Some(slot) = next {
                    let before = game.inventory[slot].as_ref().map_or(0, |stack| stack.count);
                    if !game.menu_click(Some(slot), 0, Input::QuickMove) {
                        return Err("Storage rejected an inventory transfer.".into());
                    }
                    let after = game.inventory[slot].as_ref().map_or(0, |stack| stack.count);
                    storage.phase = StoragePhase::Deposit {
                        window, ready: now + 6, next_slot: slot + 1, moved: moved || after < before,
                    };
                }
            }
        }
        StoragePhase::Finish { ready, moved } if now >= ready => {
            close_storage(game, Some(storage));
            if !moved || game.inventory[..36].iter().all(Option::is_some) {
                return Err("Storage did not free an inventory slot.".into());
            }
            job.replan(now);
            return Ok(true);
        }
        StoragePhase::Finish { .. } => (),
    }
    Ok(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome { Active, Complete(&'static str), Found(Pos) }

fn evade(game: &mut Game<'_>, settings: &Settings, job: &mut Job, here: Pos, hurt: bool) -> bool {
    let now = game.ticks;
    job.mob_zones.retain(|zone| now < zone.expires);
    let threat = nearest_threat(game, settings);
    if let Some(pos) = threat { remember_threat(job, pos, now); }
    let fire = game.player.as_ref().is_some_and(|player| player.fire_ticks > 0);
    let unsafe_now = threat.is_some() || hurt || fire;
    if unsafe_now && job.evasion.is_none() {
        super::dihrill_shelter::cancel(game);
        release(game, Some(job));
        if let Some(visit) = job.shop.take() { visit.cancel_for_avoidance(game); }
        close_storage(game, job.storage.as_ref());
        job.storage = None;
        job.vein = None;
        job.replan(now);
        job.evasion = Some(Evasion {
            clear_at: now + EVASION_CLEAR_TICKS,
            retry_at: now + 2,
            failed_destination: None,
            wall: EscapeWall::Ready,
        });
        say(game, "Leaving danger. Mining will resume from safety.");
    }
    let Some(evasion) = &mut job.evasion else { return false; };
    if unsafe_now { evasion.clear_at = now + EVASION_CLEAR_TICKS; }
    job.phase = Phase::Recovering;
    job.last_progress = Instant::now();
    if let EscapeWall::Building { lower, direction, until, clicks } = evasion.wall {
        use super::dihrill_shelter::{Seal, escape_wall_step};
        let cells = [lower, up(lower, 1)];
        let pending = cells.into_iter().any(|pos| dihrill_remote::block_pending(game, pos));
        let confirmed = !pending && cells.into_iter().all(|pos| facts_at(game, pos)
            .is_some_and(|facts| pillar_material(&facts.name)));
        let result = if confirmed {
            Seal::Finished
        } else if hurt || now >= until || threat_too_close_to_build(game, settings) {
            Seal::Failed
        } else if pending {
            Seal::Waiting
        } else if clicks >= 2 {
            Seal::Failed
        } else {
            escape_wall_step(game, lower, direction)
        };
        match result {
            Seal::Started => {
                evasion.wall = EscapeWall::Building { lower, direction, until, clicks: clicks + 1 };
                return true;
            }
            Seal::Waiting => return true,
            Seal::Finished => {
                job.blocked.extend([lower, up(lower, 1)]);
                say(game, "Barricaded the tunnel behind you.");
            }
            Seal::Failed => (),
        }
        evasion.wall = EscapeWall::Attempted;
    }
    if let Action::Walk { step, started, purpose: WalkPurpose::MobRetreat } = job.action {
        job.action = Action::Ready;
        let _ = step_walk(game, settings, job, here, step, started, WalkPurpose::MobRetreat);
        return true;
    }
    if !unsafe_now && now >= evasion.clear_at {
        job.evasion = None;
        job.replan(now);
        say(game, "Clear of danger. Looking for a safer mining route.");
        return true;
    }
    if unsafe_now && now >= evasion.retry_at {
        let failed = evasion.failed_destination.take();
        if let Some(step) = escape_step(game, settings, job, here, failed) {
            let direction = std::array::from_fn(|axis| step.to[axis] - step.from[axis]);
            let evasion = job.evasion.as_mut().expect("active evasion");
            if settings.retreat_barricade && !job.find_only && matches!(evasion.wall, EscapeWall::Ready)
                && let Some(wall) = escape_wall_site(game, settings, here, direction, 4.2)
            {
                evasion.wall = wall;
                return true;
            }
            job.walk_progress = None;
            job.action = Action::Walk { step, started: now, purpose: WalkPurpose::MobRetreat };
        } else {
            job.evasion.as_mut().expect("active evasion").retry_at = now + 5;
        }
    }
    true
}

fn advance(game: &mut Game<'_>, settings: &Settings, job: &mut Job, hurt: bool) -> Result<Outcome, String> {
    let here = feet(game);
    let now = game.ticks;
    if evade(game, settings, job, here, hurt) { return Ok(Outcome::Active); }
    if game.player.as_ref().is_some_and(|player| {
        player.health.health() < 5.0 || player.health.food.food < 3
    }) {
        return Ok(Outcome::Complete("Stopped because health or hunger is too low."));
    }
    if let Some(reason) = danger(game, settings, here) {
        let retreating = matches!(job.action, Action::Walk { purpose: WalkPurpose::Retreat, .. });
        if !retreating {
            if job.shop.is_some() { return Err(reason.into()); }
            release(game, Some(job));
            if let Some(step) = retreat_step(game, settings, job, here) {
                job.phase = Phase::Recovering;
                job.action = Action::Walk { step, started: now, purpose: WalkPurpose::Retreat };
                return Ok(Outcome::Active);
            }
            return Err(reason.into());
        }
    }
    observe(game, settings, job);
    if !job.find_only && goal_met(settings, job) && job.pickup.is_none() && job.drops.is_empty() && job.shop.is_none() && matches!(job.action, Action::Ready) {
        return Ok(Outcome::Complete(if job.shaft.is_some() {
            "Vertical digging finished."
        } else {
            "Mining goal reached."
        }));
    }
    if let Some(mut visit) = job.shop.take() {
        match visit.tick(game) {
            shop::Result::Active => job.shop = Some(visit),
            shop::Result::Finished => {
                job.last_shop = Some(now);
                job.phase = Phase::Scanning;
            }
            shop::Result::Unavailable(reason) => {
                job.shop_unavailable = true;
                job.phase = Phase::Scanning;
                say(game, reason);
            }
            shop::Result::Interrupted(reason) => {
                job.shop = Some(visit);
                return Err(reason.into());
            }
        }
        return Ok(Outcome::Active);
    }
    match std::mem::take(&mut job.action) {
        Action::Walk { step, started, purpose } => {
            step_walk(game, settings, job, here, step, started, purpose)?;
        }
        Action::ShaftJump { from, started } => {
            shaft_jump(game, settings, job, from, started)?;
        }
        Action::ShaftLanding { to, deadline } => {
            shaft_landing(game, job, here, to, deadline)?;
        }
        Action::Aim { pos, target, started } => {
            step_aim(game, settings, job, here, pos, target, started)?;
        }
        action @ Action::Mine { .. } => {
            step_mine(game, settings, job, here, action)?;
        }
        Action::Confirm { pos, deadline, target, before } => {
            if dihrill_remote::block_pending(game, pos) && now < deadline {
                job.action = Action::Confirm { pos, deadline, target, before };
            } else if !dihrill_remote::block_pending(game, pos) &&
                state_at(game, pos).is_some_and(|id| game.res.facts().is_air(id))
            {
                mined_block(game, job, pos, target, before);
            } else {
                reject_break(job, now, pos, here, target, true)?;
            }
        }
        Action::Collect { .. } => {
            // Item quotas depend on confirmed pickups. Block quotas can
            // safely finish reachable ore faces before walking for drops.
            if !matches!(settings.goal, Goal::Items(_, _)) {
                begin_visible_mining(game, settings, job, here, None);
            }
        }
        Action::Ready => {
            if job.find_only {
                if ground(game) && now >= job.retry_at && !follow_route(game, settings, job, here)? {
                    scan_next(game, settings, job, here)?;
                }
            } else if job.shaft.as_ref().is_some_and(|shaft| shaft.pending_landing.is_some()) {
                step_shaft(game, settings, job, here)?;
            } else if job.storage.is_some() {
                step_storage(game, settings, job, here)?;
            } else if job.dump_next_slot.is_some() {
                if ground(game) { maybe_make_room(game, settings, job)?; }
            } else if ground(game) && !job.drops.is_empty() {
                collect_drops(game, settings, job, here)?;
            } else if ground(game) && !try_shop(game, settings, job)
                && now >= job.retry_at && !maybe_make_room(game, settings, job)?
                && !follow_route(game, settings, job, here)?
            {
                if job.shaft.is_some() {
                    step_shaft(game, settings, job, here)?;
                } else {
                    scan_next(game, settings, job, here)?;
                }
            }
        }
    }
    Ok(job.found.map_or(Outcome::Active, Outcome::Found))
}

pub(in crate::game) fn shop_manual() {
    MINER.with(|state| {
        if let Ok(mut miner) = state.try_borrow_mut()
            && let Some(visit) = miner.job.as_mut().and_then(|job| job.shop.as_mut())
        {
            visit.manual_click();
        }
    });
}

pub(in crate::game) fn tick(game: &mut Game<'_>) {
    MINER.with(|state| {
        let mut miner = state.borrow_mut();
        if miner.job.is_none() { return; }
        let unavailable = game.title.is_some()
            || game.remote_loading()
            || game.mode_id() != 0
            || game.player.as_ref().is_none_or(|player| player.health.health() <= 0.0);
        if unavailable {
            stop(game, &mut miner, "Stopped because the world or player state changed.");
            return;
        }
        let emergency_exit = miner.settings.emergency_exit;
        let job = miner.job.as_mut().expect("active miner");
        let player = game.player.as_ref().expect("surviving player");
        let health = player.health.health();
        let hurt = health < job.last_health || player.health.hurt_animation_time > 0;
        job.last_health = health;
        if emergency_exit && health <= 6.0 && (hurt || player.fire_ticks > 0) {
            let reason = "Critical health while taking damage.";
            stop(game, &mut miner, reason);
            miner.emergency_exit = Some(reason);
            return;
        }
        if miner.job.as_ref().is_some_and(|job| job.paused) {
            if let Some(job) = &mut miner.job {
                release(game, Some(job));
                close_storage(game, job.storage.as_ref());
                job.storage = None;
                close_shop(game, job);
            }
            return;
        }
        let movement = [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::Space];
        let manual = movement.iter().any(|key| {
            game.keys.held_by(key, Source::Keyboard) || game.keys.held_by(key, Source::Pad)
        });
        if manual || !game.focused || other_automation(game) {
            pause(game, &mut miner, "Paused for manual control.");
            return;
        }
        let own_storage = miner.job.as_ref().is_some_and(|job| {
            job.storage.as_ref().is_some_and(|storage| {
                matches!(storage.phase, StoragePhase::AwaitOpen { .. } | StoragePhase::Deposit { .. } | StoragePhase::Finish { .. })
            })
        });
        let own_shop = miner.job.as_ref().is_some_and(|job| job.shop.is_some());
        if (game.screen != Screen::Playing && !own_storage && !own_shop) || game.pause || game.dialog_open() {
            if let Some(job) = &mut miner.job {
                release(game, Some(job));
                close_shop(game, job);
                if !matches!(job.phase, Phase::Waiting) {
                    job.replan(game.ticks);
                    job.phase = Phase::Waiting;
                }
                job.timer_sample = None;
                job.last_progress = Instant::now();
            }
            return;
        }
        let Miner { settings, job, .. } = &mut *miner;
        let job = job.as_mut().expect("active miner");
        let now = Instant::now();
        let elapsed = job.timer_sample.replace(now).map_or(Duration::ZERO, |last| now.duration_since(last));
        if let Some(remaining) = &mut job.run_remaining {
            *remaining = remaining.saturating_sub(elapsed);
        }
        let outcome = if job.run_remaining == Some(Duration::ZERO) {
            Ok(Outcome::Complete("Run timer finished."))
        } else if job.evasion.is_none() && matches!(job.action, Action::Ready)
            && now.duration_since(job.last_progress) >= STALL_LIMIT
        {
            if job.shaft.is_none() && job.storage.is_none() && job.shop.is_none() {
                job.schedule_rescan(game.ticks, "No progress for 40 seconds. Waiting for a fresh scan.");
                Ok(Outcome::Active)
            } else {
                Err(format!("Paused after 40 seconds without progress ({}).", job.phase.label()))
            }
        } else {
            advance(game, settings, job, hurt)
        };
        match outcome {
            Ok(Outcome::Active) => (),
            Ok(Outcome::Complete(reason)) => {
                let shelter = settings.shelter_on_stop && !job.find_only;
                stop(game, &mut miner, reason);
                say(game, &miner.message);
                if shelter { super::dihrill_shelter::begin(game); }
            }
            Ok(Outcome::Found([x, y, z])) => {
                let block = facts_at(game, [x, y, z]).map_or("Selected block", |facts|
                    facts.name.strip_prefix("minecraft:").unwrap_or(&facts.name));
                let message = format!("Found {block} at {x}, {y}, {z}.");
                stop(game, &mut miner, &message);
                say(game, &message);
            }
            Err(reason) => {
                pause(game, &mut miner, &reason);
                say(game, &reason);
            }
        }
    });
}

pub(in crate::game) fn finish_emergency_exit(game: &mut Game<'_>) -> bool {
    let reason = MINER.with(|state| state.borrow_mut().emergency_exit.take());
    let Some(reason) = reason else { return false; };
    if game.title.is_some() || game.player.is_none() { return false; }
    // Leaving invokes the level-unload hook, which also accesses MINER.
    // Run it after the tick and after releasing the RefCell borrow.
    let [x, y, z] = feet(game);
    say(game, &format!("Safety exit at {x}, {y}, {z}. {reason}"));
    if let Err(error) = game.close_world() {
        say(game, &format!("Safety exit failed: {error}. Leave the world manually."));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dihrill_selected_drops_include_silk_and_regular_ore_results() {
        let settings = Settings {
            targets: vec!["minecraft:deepslate_diamond_ore".into(), "minecraft:stone".into()],
            ..Settings::default()
        };
        for id in ["minecraft:diamond", "minecraft:deepslate_diamond_ore", "minecraft:cobblestone"] {
            assert!(selected_drop(&settings, id), "{id}");
        }
        for id in ["minecraft:raw_gold", "minecraft:gravel", "minecraft:iron_pickaxe"] {
            assert!(!selected_drop(&settings, id), "{id}");
        }
    }

    #[test]
    fn dihrill_turns_across_the_angle_wrap_in_the_shorter_direction() {
        assert_eq!(angle_delta(175.0, -175.0), 10.0);
        assert_eq!(angle_delta(-175.0, 175.0), -10.0);
    }
}
