use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

pub(super) type Pos = [i32; 3];
pub(super) const SIDES: [Pos; 4] = [[1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 0, -1]];
const AROUND: [Pos; 6] = [[1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 0, -1], [0, 1, 0], [0, -1, 0]];
const MAX_NODES: usize = 24_000;
const MAX_EXPLORE_NODES: usize = 3_600;
const MAX_VEIN: usize = 1_024;
const DIG_CAPACITY: usize = 12;

pub(super) fn plus(a: Pos, b: Pos) -> Pos {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn up(p: Pos, dy: i32) -> Pos {
    plus(p, [0, dy, 0])
}

#[derive(Clone, Copy, Default)]
pub(super) struct Cell {
    pub known: bool,
    pub clear: bool,
    pub floor: bool,
    pub hazard: bool,
    pub cave: bool,
    // This liquid is traversable at feet height only with opt-in wading.
    pub shallow_water: bool,
    pub falling: bool,
    pub dig_cost: Option<u16>,
    pub target: bool,
    pub vein: u32,
    pub priority: u8,
}

pub(super) fn dry_margin(get: &mut impl FnMut(Pos) -> Cell, p: Pos) -> bool {
    let safe = |c: Cell| c.known && !c.hazard;
    safe(get(p)) && SIDES.iter().all(|&direction| safe(get(plus(p, direction))))
}

fn walking_margin(get: &mut impl FnMut(Pos) -> Cell, p: Pos, allow_wading: bool) -> bool {
    let safe = |c: Cell| c.known && (!c.hazard || (allow_wading && c.shallow_water));
    safe(get(p)) && SIDES.iter().all(|&direction| safe(get(plus(p, direction))))
}

pub(super) fn can_break(get: &mut impl FnMut(Pos) -> Cell, p: Pos) -> bool {
    let c = get(p);
    c.known
        && !c.hazard
        && !c.cave
        && !c.falling
        && !c.clear
        && c.dig_cost.is_some()
        && AROUND.iter().all(|&direction| {
            let next = get(plus(p, direction));
            next.known && !next.hazard && !next.cave && (direction != [0, 1, 0] || !next.falling)
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Step {
    pub from: Pos,
    pub to: Pos,
    // Clear the near center of the opening first so its sides can be reached.
    pub dig: [Option<Pos>; DIG_CAPACITY],
}

pub(super) fn transition_with_fall(
    get: &mut impl FnMut(Pos) -> Cell,
    from: Pos,
    to: Pos,
    tunnel: bool,
    max_fall: u8,
) -> Option<(Step, u32)> {
    transition_configured(get, from, to, tunnel, max_fall, 2, 1)
}

pub(super) fn transition_configured(
    get: &mut impl FnMut(Pos) -> Cell,
    from: Pos,
    to: Pos,
    tunnel: bool,
    max_fall: u8,
    height: u8,
    width: u8,
) -> Option<(Step, u32)> {
    let dy = to[1] - from[1];
    if dy > 1
        || dy < -i32::from(max_fall.min(3))
        || from[0].abs_diff(to[0]) + from[2].abs_diff(to[2]) != 1
    {
        return None;
    }
    if get(to).cave { return None; }
    let floor = get(up(to, -1));
    if !floor.known || !floor.floor || floor.hazard || floor.falling {
        return None;
    }

    if dy != 0 && get(from).shallow_water { return None; }
    if dy > 0 {
        let clearance = up(from, 2);
        if !get(clearance).clear || !dry_margin(get, clearance) {
            return None;
        }
    }
    if dy < 0 {
        // The player walks out over the lip before dropping. Validate the
        // entire fall shaft from the landing through the incoming head height.
        for y in to[1]..=from[1] + 1 {
            let p = [to[0], y, to[2]];
            if !get(p).clear || get(p).cave || !dry_margin(get, p) {
                return None;
            }
        }
        return Some((Step { from, to, dig: [None; DIG_CAPACITY] }, 10 + dy.unsigned_abs() * 8));
    }

    let (height, width) = if dy == 0 { (height.clamp(2, 4), width.clamp(1, 3)) } else { (2, 1) };
    let [dx, dz] = [to[0] - from[0], to[2] - from[2]];
    let lateral = [-dz, 0, dx];
    let offsets = [[0, 0, 0], lateral, lateral.map(|component| -component)];
    let mut dig = [None; DIG_CAPACITY];
    let mut blocks = 0;
    let mut cost = 10 + u32::from(dy > 0) * 6;
    for direction in offsets.into_iter().take(usize::from(width)) {
        let column = plus(to, direction);
        if column != to {
            let floor = get(up(column, -1));
            if !floor.known || !floor.floor || floor.hazard || floor.falling {
                return None;
            }
        }
        // Check both body cells and extra space requested for a wider tunnel.
        for y in (0..i32::from(height)).rev() {
            let p = up(column, y);
            let wading = dy == 0 && y == 0 && width == 1;
            if !walking_margin(get, p, wading) {
                return None;
            }
            let c = get(p);
            if c.shallow_water && wading {
                cost += 12;
                continue;
            }
            if c.cave { return None; }
            if c.clear {
                continue;
            }
            if !tunnel || get(from).shallow_water || c.target || !can_break(get, p) {
                return None;
            }
            dig[blocks] = Some(p);
            blocks += 1;
            cost += u32::from(c.dig_cost?);
        }
    }
    Some((Step { from, to, dig }, cost))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteKind {
    Target,
    Explore,
    Destination,
}

pub(super) struct Route {
    pub target: Pos,
    pub steps: VecDeque<Step>,
    pub kind: RouteKind,
}

pub(super) enum SearchResult {
    Pending,
    Found(Route),
    Exhausted,
}

pub(super) struct SearchOptions {
    pub tunnel: bool,
    pub max_fall: u8,
    pub vein: Option<HashSet<Pos>>,
    pub blocked_edges: HashSet<(Pos, Pos)>,
    pub blocked_approaches: HashSet<(Pos, Pos)>,
    pub junk_pickup_risk: HashSet<Pos>,
    pub area: Option<[Pos; 2]>,
    pub goal: Option<Pos>,
    pub target_node_limit: Option<usize>,
    pub tunnel_height: u8,
    pub tunnel_width: u8,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            tunnel: true,
            max_fall: 1,
            vein: None,
            blocked_edges: HashSet::new(),
            blocked_approaches: HashSet::new(),
            junk_pickup_risk: HashSet::new(),
            area: None,
            goal: None,
            target_node_limit: None,
            tunnel_height: 2,
            tunnel_width: 1,
        }
    }
}

#[derive(Default)]
pub(super) struct ExploreOptions {
    pub heading: [i32; 2],
    pub visited: HashMap<Pos, u16>,
    pub tunnel: bool,
    pub preferred: Option<Pos>,
}

pub(super) fn inside(pos: Pos, area: [Pos; 2]) -> bool {
    (0..3).all(|i| (area[0][i].min(area[1][i])..=area[0][i].max(area[1][i])).contains(&pos[i]))
}

struct PathNode {
    cost: u32,
    previous: Option<Step>,
}

struct Candidate {
    score: i64,
    target: Pos,
    stand: Pos,
}

pub(super) struct Search {
    start: Pos,
    radius: u32,
    options: SearchOptions,
    explore: Option<ExploreOptions>,
    queue: BinaryHeap<Reverse<(u32, u32, Pos)>>,
    visited: HashMap<Pos, PathNode>,
    cells: HashMap<Pos, Cell>,
    best: Option<Candidate>,
    expanded: usize,
}

impl Search {
    pub fn with_options(start: Pos, radius: i32, options: SearchOptions) -> Self {
        let heuristic = goal_heuristic(start, options.goal);
        Self {
            start,
            radius: radius.clamp(1, 64) as u32,
            options,
            explore: None,
            queue: BinaryHeap::from([Reverse((heuristic, 0, start))]),
            visited: HashMap::from([(start, PathNode { cost: 0, previous: None })]),
            cells: HashMap::new(),
            best: None,
            expanded: 0,
        }
    }

    pub fn explore(start: Pos, radius: i32, mut options: SearchOptions, explore: ExploreOptions) -> Self {
        options.tunnel = explore.tunnel;
        options.goal = None;
        let mut search = Self::with_options(start, radius.min(12), options);
        search.explore = Some(explore);
        search
    }

    fn finish(&self) -> SearchResult {
        let Some(best) = &self.best else { return SearchResult::Exhausted };
        let mut steps = VecDeque::new();
        let mut end = best.stand;
        while let Some(step) = self.visited[&end].previous {
            steps.push_front(step);
            end = step.from;
        }
        let kind = if self.explore.is_some() {
            RouteKind::Explore
        } else if self.options.goal.is_some() {
            RouteKind::Destination
        } else {
            RouteKind::Target
        };
        SearchResult::Found(Route { target: best.target, steps, kind })
    }

    pub fn checked_positions(&self) -> usize { self.expanded }

    pub fn advance(&mut self, budget: usize, mut read: impl FnMut(Pos) -> Cell) -> SearchResult {
        let limit = if self.explore.is_some() {
            MAX_EXPLORE_NODES
        } else {
            self.options.target_node_limit.unwrap_or(MAX_NODES)
        };
        for _ in 0..budget {
            let Some(Reverse((_, cost, here))) = self.queue.pop() else { return self.finish() };
            // Exploration can reward distance and forward progress by at most
            // 55 and 60 points respectively. Once the frontier cannot beat
            // our best score, scanning the rest of the area adds no value.
            let lower_bound = if self.explore.is_some() {
                i64::from(cost) / 5 - 55 - 60
            } else {
                i64::from(cost)
            };
            if self.expanded >= limit
                || self.best.as_ref().is_some_and(|best| lower_bound > best.score)
            {
                return self.finish();
            }
            if self.visited[&here].cost != cost {
                continue;
            }
            self.expanded += 1;
            let mut get = |pos| *self.cells.entry(pos).or_insert_with(|| read(pos));
            if self.options.goal == Some(here) {
                self.best = Some(Candidate { score: i64::from(cost), target: here, stand: here });
                return self.finish();
            }
            if let Some(explore) = &self.explore {
                consider_exploration(&mut self.best, explore, self.start, here, cost, self.options.area);
            } else if self.options.goal.is_none() {
                consider_targets(&mut self.best, &mut get, here, cost, &self.options, self.start, self.radius);
            }
            for direction in SIDES {
                let side = plus(here, direction);
                for dy in -(i32::from(self.options.max_fall.min(3)))..=1 {
                    let to = up(side, dy);
                    if !within_radius(to, self.start, self.radius)
                        || self.options.blocked_edges.contains(&(here, to))
                    {
                        continue;
                    }
                    let Some((step, mut weight)) = transition_configured(
                        &mut get, here, to, self.options.tunnel, self.options.max_fall,
                        self.options.tunnel_height, self.options.tunnel_width,
                    ) else {
                        continue;
                    };
                    if let Some(area) = self.options.area
                        && step.dig.iter().flatten().any(|&pos| !inside(pos, area))
                    {
                        continue;
                    }
                    if let Some(explore) = &self.explore {
                        weight += u32::from(explore.visited.get(&to).copied().unwrap_or(0).min(6)) * 14;
                    }
                    if self.options.junk_pickup_risk.contains(&to) { weight += 80; }
                    let score = cost.saturating_add(weight);
                    if self.visited.get(&to).is_none_or(|node| score < node.cost) {
                        self.visited.insert(to, PathNode { cost: score, previous: Some(step) });
                        let estimate = score.saturating_add(goal_heuristic(to, self.options.goal));
                        self.queue.push(Reverse((estimate, score, to)));
                    }
                }
            }
        }
        SearchResult::Pending
    }
}

fn goal_heuristic(pos: Pos, goal: Option<Pos>) -> u32 {
    // Each edge changes only one horizontal coordinate and costs at least 10.
    // This keeps A* admissible even when a single edge can descend several blocks.
    goal.map_or(0, |goal| {
        pos[0].abs_diff(goal[0])
            .saturating_add(pos[2].abs_diff(goal[2]))
            .saturating_mul(10)
    })
}

fn within_radius(pos: Pos, start: Pos, radius: u32) -> bool {
    pos[0].abs_diff(start[0]) <= radius
        && pos[2].abs_diff(start[2]) <= radius
        && pos[1].abs_diff(start[1]) <= radius.min(12)
}

fn offer(best: &mut Option<Candidate>, score: i64, target: Pos, stand: Pos) {
    if best.as_ref().is_none_or(|current| (score, target, stand) < (current.score, current.target, current.stand)) {
        *best = Some(Candidate { score, target, stand });
    }
}

fn consider_targets(
    best: &mut Option<Candidate>,
    get: &mut impl FnMut(Pos) -> Cell,
    stand: Pos,
    cost: u32,
    options: &SearchOptions,
    start: Pos,
    radius: u32,
) {
    if get(stand).shallow_water || get(stand).cave { return; }
    for direction in SIDES.into_iter().chain([[0, 0, 0]]) {
        let side = plus(stand, direction);
        let heights: &[i32] = if direction == [0, 0, 0] { &[2] } else { &[0, 1, -1, 2] };
        for &dy in heights {
            let p = up(side, dy);
            if !within_radius(p, start, radius) {
                continue;
            }
            let c = get(p);
            if c.target
                && !options.area.is_some_and(|area| !inside(p, area))
                && !options.vein.as_ref().is_some_and(|vein| !vein.contains(&p))
                && !options.blocked_approaches.contains(&(stand, p))
                && can_break(get, p)
            {
                let priority_penalty = i64::from(8 - c.priority.min(8)) * 12;
                let angle_penalty = if (0..=1).contains(&dy) { 0 } else { 12 };
                offer(best, i64::from(cost) + i64::from(c.dig_cost.unwrap_or_default()) + priority_penalty + angle_penalty, p, stand);
            }
        }
    }
}

fn consider_exploration(
    best: &mut Option<Candidate>,
    options: &ExploreOptions,
    start: Pos,
    here: Pos,
    cost: u32,
    area: Option<[Pos; 2]>,
) {
    if here == start || options.visited.contains_key(&here) || area.is_some_and(|area| !inside(here, area)) {
        return;
    }
    let [dx, dz] = [here[0] - start[0], here[2] - start[2]];
    let horizontal = dx.abs() + dz.abs();
    let forward = dx * options.heading[0].clamp(-1, 1) + dz * options.heading[1].clamp(-1, 1);
    // A short waypoint keeps ore discovery responsive. Prefer forward progress,
    // but route around a dead end when its other side is safe and unvisited.
    let preferred_distance = options.preferred.map_or(0, |p| {
        i64::from(p[0].abs_diff(here[0]))
            + i64::from(p[2].abs_diff(here[2]))
            + i64::from(p[1].abs_diff(here[1])) * 2
    });
    let score = i64::from(cost) / 5 - i64::from(horizontal.min(5)) * 11
        - i64::from(forward.clamp(-5, 5)) * 12 + preferred_distance * 12;
    offer(best, score, here, here);
}

pub(super) struct Vein {
    key: u32,
    diagonal: bool,
    positions: HashSet<Pos>,
    mined: HashSet<Pos>,
    discovered: HashSet<Pos>,
    frontier: VecDeque<Pos>,
    queued: HashSet<Pos>,
    checked: HashSet<Pos>,
}

impl Vein {
    pub fn new(seed: Pos, key: u32, diagonal: bool) -> Self {
        Self {
            key,
            diagonal,
            positions: HashSet::new(),
            mined: HashSet::new(),
            discovered: HashSet::new(),
            frontier: VecDeque::from([seed]),
            queued: HashSet::from([seed]),
            checked: HashSet::new(),
        }
    }

    fn queue_neighbors(&mut self, p: Pos, refresh: bool) {
        for dx in -1i32..=1 {
            for dy in -1i32..=1 {
                for dz in -1i32..=1 {
                    let distance = dx.abs() + dy.abs() + dz.abs();
                    if distance == 0 || (!self.diagonal && distance != 1) {
                        continue;
                    }
                    let next = plus(p, [dx, dy, dz]);
                    if (refresh || !self.checked.contains(&next)) && self.queued.insert(next) {
                        self.frontier.push_back(next);
                    }
                }
            }
        }
    }

    pub fn refresh(&mut self, p: Pos) {
        self.queue_neighbors(p, true);
    }

    pub fn mark_mined(&mut self, p: Pos) {
        if self.positions.remove(&p) {
            self.mined.insert(p);
        }
    }

    pub fn remaining(&self) -> HashSet<Pos> {
        self.positions.clone()
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.positions.len() + self.mined.len(), self.mined.len())
    }

    pub fn advance(&mut self, budget: usize, mut get: impl FnMut(Pos) -> Cell) -> bool {
        for _ in 0..budget {
            let Some(pos) = self.frontier.pop_front() else { return true };
            self.queued.remove(&pos);
            self.checked.insert(pos);
            let c = get(pos);
            if !c.known || !c.target || c.vein == 0 || c.vein != self.key {
                self.positions.remove(&pos);
                continue;
            }
            if self.mined.contains(&pos) {
                continue;
            }
            let known_position = self.discovered.contains(&pos);
            if !known_position && self.discovered.len() == MAX_VEIN {
                continue;
            }
            self.positions.insert(pos);
            if !known_position {
                self.discovered.insert(pos);
                self.queue_neighbors(pos, false);
            }
        }
        self.frontier.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid() -> Cell {
        Cell { known: true, floor: true, ..Cell::default() }
    }

    fn air() -> Cell {
        Cell { known: true, clear: true, ..Cell::default() }
    }

    fn ore(vein: u32) -> Cell {
        Cell { target: true, vein, dig_cost: Some(20), ..solid() }
    }

    fn hall([x, y, z]: Pos) -> Cell {
        if (x == 2 || x == 3) && z == 0 && (y == 0 || y == 1) {
            Cell { dig_cost: Some(45), ..solid() }
        } else if x == 5 && y == 0 && z == 0 {
            ore(1)
        } else if y == -1 || y == 2 {
            solid()
        } else {
            air()
        }
    }

    fn finish(mut search: Search, mut get: impl FnMut(Pos) -> Cell) -> Option<Route> {
        for _ in 0..2_000 {
            match search.advance(30, &mut get) {
                SearchResult::Pending => (),
                SearchResult::Found(route) => return Some(route),
                SearchResult::Exhausted => return None,
            }
        }
        panic!("search exceeded its finite node budget");
    }

    #[test]
    fn ore_miner_prefers_a_walk_over_a_tunnel() {
        let route = finish(Search::with_options([0, 0, 0], 8, SearchOptions::default()), hall).unwrap();
        assert_eq!(route.target, [5, 0, 0]);
        assert!(route.steps.iter().all(|step| step.dig.iter().all(Option::is_none)));
    }

    #[test]
    fn ore_miner_tunnels_in_a_sealed_hall() {
        let route = finish(Search::with_options([0, 0, 0], 8, SearchOptions::default()), |pos| {
            if pos[2] == 0 { hall(pos) } else { solid() }
        }).unwrap();
        assert!(route.steps.iter().any(|step| step.dig.into_iter().flatten().collect::<Vec<_>>() ==
            vec![[2, 1, 0], [2, 0, 0]]));
    }

    #[test]
    fn ore_miner_wades_only_across_flat_shallow_water_without_digging() {
        let water = Cell { hazard: true, shallow_water: true, ..air() };
        let mut world = |p: Pos| {
            if p == [1,0,0] { water }
            else if p[1] == -1 { solid() }
            else { air() }
        };
        assert!(transition_with_fall(&mut world,[0,0,0],[1,0,0],false,1).is_some());
        assert!(transition_with_fall(&mut world,[1,0,0],[0,0,0],false,1).is_some());
        assert!(!can_break(&mut |p| if p == [1,0,0] { water } else { ore(2) }, [1,0,1]));
        let mut deep = |p: Pos| if p == [1,1,0] { water } else { world(p) };
        assert!(transition_with_fall(&mut deep,[0,0,0],[1,0,0],false,1).is_none());
        let mut ledge = |p: Pos| if p == [0,0,0] { water } else { world(p) };
        assert!(transition_with_fall(&mut ledge,[0,0,0],[1,-1,0],false,1).is_none());
    }

    #[test]
    fn ore_miner_checks_water_lava_and_falling_blocks() {
        let wall = [2, 1, 0];
        let fluid = Cell { known: true, hazard: true, ..Cell::default() };
        for neighbor in [[2, 2, 0], [2, 1, 1], [2, 0, 0]] {
            let mut with_water_or_lava = |p| if p == neighbor { fluid } else { hall(p) };
            assert!(!can_break(&mut with_water_or_lava, wall));
        }
        let mut with_gravel = |p| if p == [2, 2, 0] {
            Cell { falling: true, ..solid() }
        } else { hall(p) };
        assert!(!can_break(&mut with_gravel, wall));
        let mut unloaded = |p| if p == [1, 1, 1] { Cell::default() } else { hall(p) };
        assert!(transition_with_fall(&mut unloaded, [0, 0, 0], [1, 0, 0], true, 1).is_none());
    }

    #[test]
    fn ore_miner_requires_a_clear_landing_and_limits_fall_distance() {
        let from = [0, 0, 0];
        let to = [1, -2, 0];
        let mut drop = |p: Pos| if (p[0] == 1 && p[1] == -3) || p == [0, -1, 0] { solid() } else { air() };
        assert!(transition_with_fall(&mut drop, from, to, true, 1).is_none());
        assert!(transition_with_fall(&mut drop, from, to, true, 2).is_some());
        let mut hazard = |p| if p == [1, -1, 1] {
            Cell { hazard: true, ..air() }
        } else { drop(p) };
        assert!(transition_with_fall(&mut hazard, from, to, true, 2).is_none());
        let mut unknown_landing = |p| if p == [1, -3, 0] { Cell::default() } else { drop(p) };
        assert!(transition_with_fall(&mut unknown_landing, from, to, true, 2).is_none());
    }

    #[test]
    fn ore_miner_steps_up_only_under_clear_headroom() {
        let from = [0, 0, 0];
        let to = [1, 1, 0];
        let terrain = |p| if p == [1, 0, 0] { solid() } else { air() };
        assert!(transition_with_fall(&mut |p| terrain(p), from, to, true, 0).is_some());
        let mut blocked = |p| if p == [0, 2, 0] { solid() } else { terrain(p) };
        assert!(transition_with_fall(&mut blocked, from, to, true, 0).is_none());
    }

    #[test]
    fn ore_miner_continues_a_vein_when_a_closer_target_appears() {
        let first = [2, 0, 0];
        let neighbor = [3, 0, 0];
        let diagonal = [4, 1, 0];
        let closer = [-1, 0, 0];
        let mut cells = HashMap::from([(first, ore(1)), (neighbor, ore(1)), (diagonal, ore(1)), (closer, ore(2))]);
        let get = |p: Pos| cells.get(&p).copied().unwrap_or_else(|| if p[1] == -1 { solid() } else { air() });
        let mut vein = Vein::new(first, 1, false);
        assert!(!vein.advance(1, get));
        assert!(vein.advance(32, get));
        assert_eq!(vein.remaining(), HashSet::from([first, neighbor]));
        assert_eq!(vein.counts(), (2, 0));
        let mut all_directions = Vein::new(first, 1, true);
        assert!(all_directions.advance(128, get));
        assert_eq!(all_directions.remaining(), HashSet::from([first, neighbor, diagonal]));

        vein.mark_mined(first);
        vein.refresh(first);
        cells.remove(&first);
        cells.insert([2, 1, 0], ore(1));
        let get = |p: Pos| cells.get(&p).copied().unwrap_or_else(|| if p[1] == -1 { solid() } else { air() });
        assert!(vein.advance(32, get));
        assert_eq!(vein.counts(), (3, 1));
        let options = SearchOptions { vein: Some(vein.remaining()), ..SearchOptions::default() };
        let route = finish(Search::with_options([0, 0, 0], 8, options), get).unwrap();
        assert_ne!(route.target, closer);
        assert!(vein.remaining().contains(&route.target));
    }

    #[test]
    fn ore_miner_honors_priority_and_tries_another_approach() {
        let nearer = [-1, 0, 0];
        let preferred = [3, 0, 0];
        let get = |p: Pos| {
            if p == nearer { ore(1) }
            else if p == preferred { Cell { priority: 8, ..ore(2) } }
            else if p[1] == -1 { solid() }
            else { air() }
        };
        let options = SearchOptions {
            blocked_approaches: HashSet::from([([2, 0, 0], preferred)]),
            ..SearchOptions::default()
        };
        let route = finish(Search::with_options([0, 0, 0], 8, options), get).unwrap();
        assert_eq!(route.target, preferred);
        assert_ne!(route.steps.back().unwrap().to, [2, 0, 0]);
    }

    #[test]
    fn ore_miner_replans_around_a_blocked_edge() {
        let destination = [2, 0, 0];
        let options = SearchOptions {
            goal: Some(destination),
            blocked_edges: HashSet::from([([0, 0, 0], [1, 0, 0])]),
            ..SearchOptions::default()
        };
        let route = finish(Search::with_options([0, 0, 0], 8, options), |p| {
            if p[1] == -1 { solid() } else { air() }
        }).unwrap();
        assert_eq!(route.kind, RouteKind::Destination);
        assert_ne!(route.steps.front().unwrap().to, [1, 0, 0]);
        assert_eq!(route.steps.back().unwrap().to, destination);
    }

    #[test]
    fn ore_miner_explores_forward_but_can_turn_around() {
        let start = [0, 0, 0];
        let options = ExploreOptions { heading: [1, 0], ..ExploreOptions::default() };
        let route = finish(Search::explore(start, 8, SearchOptions::default(), options), |p| {
            if p[1] == -1 || p[2] != 0 { solid() } else { air() }
        }).unwrap();
        assert_eq!(route.kind, RouteKind::Explore);
        assert!(route.target[0] > 0);
        let options = ExploreOptions {
            heading: [1, 0],
            visited: HashMap::from([([0, 0, 0], 2), ([-1, 0, 0], 4)]),
            ..ExploreOptions::default()
        };
        let route = finish(Search::explore(start, 8, SearchOptions::default(), options), |p| {
            if p[1] == -1 || p[2] != 0 || p[0] > 0 { solid() } else { air() }
        }).unwrap();
        assert!(route.target[0] < -1);
    }

    #[test]
    fn ore_miner_supports_wide_tunnels_without_clearing_unselected_ores() {
        let mut get = |p: Pos| {
            if p[0] == 1 && p[1] >= 0 && p[2] == 1 {
                Cell { dig_cost: Some(20), ..solid() }
            } else if p[1] == -1 { solid() } else { air() }
        };
        let (_, narrow_cost) = transition_configured(&mut get, [0, 0, 0], [1, 0, 0], true, 1, 3, 1).unwrap();
        let (wide, wide_cost) = transition_configured(&mut get, [0, 0, 0], [1, 0, 0], true, 1, 3, 2).unwrap();
        assert_eq!(wide.dig.iter().flatten().count(), 3);
        assert!(wide_cost > narrow_cost);
        let mut with_ore = |p| if p == [1, 0, 1] { ore(1) } else { get(p) };
        assert!(transition_configured(&mut with_ore, [0, 0, 0], [1, 0, 0], true, 1, 3, 2).is_none());
    }

    #[test]
    fn ore_miner_ignores_targets_outside_the_cuboid_or_without_a_tool() {
        let options = SearchOptions { area: Some([[0, -1, 0], [4, 2, 2]]), ..SearchOptions::default() };
        let get = |p: Pos| {
            if p == [-1, 0, 0] { ore(1) }
            else if p == [2, 0, 0] { Cell { dig_cost: None, ..ore(2) } }
            else if p[1] == -1 { solid() }
            else { air() }
        };
        assert!(finish(Search::with_options([0, 0, 0], 8, options), get).is_none());
    }

    #[test]
    fn ore_miner_revalidates_a_path_when_hazards_change() {
        let from = [0, 0, 0];
        let to = [1, 0, 0];
        let options = SearchOptions { goal: Some(to), ..SearchOptions::default() };
        let original = |p: Pos| {
            if p[1] == -1 { solid() }
            else if p == [1, 1, 0] { Cell { dig_cost: Some(20), ..solid() } }
            else { air() }
        };
        let route = finish(Search::with_options(from, 8, options), original).unwrap();
        assert_eq!(route.steps.back().unwrap().to, to);
        let mut changed = |p: Pos| {
            if p == [1, 1, 1] { Cell { hazard: true, ..air() } }
            else { original(p) }
        };
        let first = route.steps.front().unwrap();
        // An established path still has to pass the current live safety checks.
        assert!(transition_with_fall(&mut changed, first.from, first.to, true, 1).is_none());
    }

    #[test]
    fn ore_miner_caps_connected_deposits_during_refresh() {
        let mut vein = Vein::new([0, 0, 0], 1, true);
        let solid_vein = |_p: Pos| ore(1);
        while !vein.advance(200, solid_vein) {}
        assert_eq!(vein.counts(), (MAX_VEIN, 0));
        let boundary = vein.remaining().into_iter().max().unwrap();
        vein.mark_mined(boundary);
        vein.refresh(boundary);
        assert!(vein.advance(200, solid_vein));
        assert_eq!(vein.counts(), (MAX_VEIN, 1));
        assert!(!vein.remaining().contains(&boundary));
    }

    #[test]
    fn ore_miner_checks_fluid_and_support_across_the_whole_tunnel() {
        let from = [0, 0, 0];
        let to = [1, 0, 0];
        let solid_floor = |p: Pos| if p[1] == -1 { solid() } else { air() };
        for p in [[1, -1, 1], [1, 3, 1], [1, 3, 2]] {
            let mut unknown_edge = |here: Pos| if here == p { Cell::default() } else { solid_floor(here) };
            assert!(transition_configured(&mut unknown_edge, from, to, true, 1, 4, 2).is_none());
        }
        let mut gravel_above_widening = |p: Pos| {
            if p == [1, 4, 1] { Cell { falling: true, ..solid() } }
            else if p == [1, 3, 1] { Cell { dig_cost: Some(10), ..solid() } }
            else { solid_floor(p) }
        };
        assert!(transition_configured(&mut gravel_above_widening, from, to, true, 1, 4, 2).is_none());
    }


    #[test]
    fn ore_miner_respects_the_target_search_radius_at_the_last_stand() {
        let get = |p: Pos| {
            if p == [9, 0, 0] { ore(1) }
            else if p[1] == -1 { solid() }
            else { air() }
        };
        let limited = Search::with_options([0, 0, 0], 8, SearchOptions::default());
        assert!(finish(limited, get).is_none());
        let sufficient = Search::with_options([0, 0, 0], 9, SearchOptions::default());
        assert_eq!(finish(sufficient, get).unwrap().target, [9, 0, 0]);
    }

}
