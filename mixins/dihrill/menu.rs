use super::dihrill::{self as miner, Snapshot};
use super::dihrill_settings::{self as config, Goal, HudPosition, Pace, Profile, Settings, ShaftDirection, Tool};
use super::{Game, Screen};
use horizon_client::{hud::HudBatch, mod_api::{NativeMod, NativeScreenView}};
use std::cell::RefCell;
use std::time::{Duration, Instant};

const ID: &str = config::ID;
const MENU_KEY: &str = "key.keyboard.f8";
const PAUSE_KEY: &str = "key.keyboard.f9";
const UNBOUND: &str = "key.keyboard.unknown";
const TEXT: u32 = 0xf8f4ff;
const MUTED: u32 = 0xc3b8d6;
const AMBER: u32 = 0xffd391;
const ROW: f32 = 18.0;
type Rect = [f32; 4];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Modules,
    Targets, Mining, Safety, Inventory, Status, Shop, Avoidance, Vertical, Hud, Bindings,
}
const PAGES: [(Page, &str); 5] = [
    (Page::Targets,"Targets"), (Page::Mining,"Mining"), (Page::Safety,"Safety"),
    (Page::Inventory,"Inventory"), (Page::Status,"Status"),
];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filter { #[default] Ores, All, Selected }

#[derive(Clone, Copy)]
enum Field { Blocks, Items, Area, Storage, Protected, ShopCount, ShopInterval, RunTime, ShaftLength }

impl Field {
    fn hint(self) -> &'static str {
        match self {
            Self::Blocks => "Enter the number of selected blocks to mine",
            Self::Items => "Enter an item ID and count, e.g. diamond 16",
            Self::Area => "Enter two corners, e.g. 0 10 0 24 20 24",
            Self::Storage => "Enter container coordinates, e.g. 8 64 -3",
            Self::Protected => "Enter item IDs separated by spaces",
            Self::ShopCount => "Enter a minimum of 1 to 2304 diamonds",
            Self::ShopInterval => "Enter a wait of 1 to 3600 seconds",
            Self::RunTime => "Type any duration in minutes or use H:MM:SS",
            Self::ShaftLength => "Enter a shaft length from 1 to 64 blocks",
        }
    }
    fn value(self, settings: &Settings) -> String {
        match self {
            Self::Blocks => match &settings.goal { Goal::Blocks(count) => count.to_string(), _ => String::new() },
            Self::Items => match &settings.goal { Goal::Items(id,count) => format!("{id} {count}"), _ => String::new() },
            Self::Area => settings.area.map_or_else(String::new, |(a,b)| format!("{} {}", coords(a), coords(b))),
            Self::Storage => settings.storage.map_or_else(String::new, coords),
            Self::Protected => settings.protected.join(" "),
            Self::ShopCount => settings.shop_min_diamonds.to_string(),
            Self::ShopInterval => settings.shop_interval_seconds.to_string(),
            Self::RunTime => timer_input(settings.run_seconds),
            Self::ShaftLength => settings.shaft_length.to_string(),
        }
    }
}

struct Editor { field: Field, value: String, replace: bool }

#[derive(Clone, Copy)]
enum Number { Radius, ExploreLimit, BranchSpacing, Width, Height, MaxFall, Turn, Durability, MobRadius }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding { Menu, Pause, Hud, Stop }

impl Binding {
    const ALL: [Self; 4] = [Self::Menu, Self::Pause, Self::Hud, Self::Stop];

    fn entry(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Menu => ("menu", "Open module menu", MENU_KEY),
            Self::Pause => ("pause", "Pause or resume", PAUSE_KEY),
            Self::Hud => ("hud", "Toggle HUD", UNBOUND),
            Self::Stop => ("stop", "Stop the run", UNBOUND),
        }
    }

    fn key(self, game: &Game<'_>) -> String {
        let (id, _, default) = self.entry();
        game.native_mod_key(ID, id, default)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Module {
    Mining, Finder, Shaft, Inventory, Caves, Mobs, Shelter, Shop, Hud, Bindings, Status,
}

impl Module {
    fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Mining => ("Ore miner", "Mine selected blocks and collect their drops."),
            Self::Finder => ("Block finder", "Walk to a selected block and report its coordinates."),
            Self::Shaft => ("Vertical dig", "Dig a shaft one supported level at a time."),
            Self::Inventory => ("Inventory", "Manage junk, protected items and storage."),
            Self::Caves => ("Cave avoidance", "Route around large underground openings."),
            Self::Mobs => ("Mob avoidance", "Back away from nearby hostile mobs."),
            Self::Shelter => ("Auto shelter", "Build shelter after a completed run or a low-health stop."),
            Self::Shop => ("Diamond shop", "Sell diamonds using the configured server shop."),
            Self::Hud => ("Mining HUD", "Show run details during gameplay."),
            Self::Bindings => ("Keybinds", "Change the menu, pause, HUD and stop keys."),
            Self::Status => ("Run status", "See progress and the last stop or rescan reason."),
        }
    }
}

#[derive(Clone)]
enum Click {
    Inventory, Page(Page), Filter(Filter), Target(String), Priority(String,bool),
    AddQuery, ClearQuery, Previous, Next, Profile(Profile), Edit(Field),
    Apply, CancelEdit, TimerOff, Unlimited, ClearArea, Number(Number,i32), Explore, ContinuousExplore,
    Diagonals, Sprint, Pace, ShallowWater, Tool(Tool), Junk(&'static str), ClearJunk,
    Alerts, StorageLook, ClearStorage, LearnShop, DisableShop, AvoidCaves, AvoidMobs, Start, Pause, Resume, Stop, Close,
    StartShaft(ShaftDirection),
    Expand(Module), Find, Shelter, Barricade, EmergencyExit,
    HudVisible, HudPosition(HudPosition), HudCounters, HudTimer, HudDestination, HudBackground(i8), HudAmber,
    Capture(Binding), SetBinding(Binding, Option<String>),
}

struct Block { id: String, label: String, ore: bool }

#[derive(Default)]
struct Menu {
    page: Page,
    filter: Filter,
    catalog: Vec<Block>,
    search: String,
    matches: Vec<usize>,
    offset: usize,
    filter_dirty: bool,
    hits: Vec<(Rect, Click)>,
    secondary_hits: Vec<(Rect, Module)>,
    expanded: Option<Module>,
    capturing: Option<Binding>,
    pending: Option<Click>,
    editor: Option<Editor>,
    notice: Option<String>,
    open_until: Option<Instant>,
    looked_at: Option<[i32; 3]>,
}

#[derive(Default)]
struct Hud { refresh: Option<Instant>, text: String }

thread_local! {
    static MENU: RefCell<Menu> = RefCell::new(Menu::default());
    static HUD: RefCell<Hud> = RefCell::new(Hud::default());
}

fn inside([x,y,w,h]: Rect, [mx,my]: [f32; 2]) -> bool {
    (x..x+w).contains(&mx) && (y..y+h).contains(&my)
}

fn name(id: &str) -> &str { id.strip_prefix("minecraft:").unwrap_or(id) }
fn coords([x,y,z]: [i32; 3]) -> String { format!("{x} {y} {z}") }

fn profile_label(profile: Profile) -> &'static str {
    match profile {
        Profile::Hunter => "Ore Hunter",
        Profile::Tunnel => "Tunnel Miner",
        Profile::Branch => "Branch Miner",
        Profile::Cave => "Cave Miner",
        Profile::Area => "Area Miner",
        Profile::Quantity => "Quantity Miner",
    }
}

fn goal_label(goal: &Goal) -> String {
    match goal {
        Goal::Unlimited => "Unlimited".into(),
        Goal::Blocks(count) => format!("Mine {count} selected blocks"),
        Goal::Items(id,count) => format!("Collect {count} {}", name(id)),
    }
}

fn on_off(value: bool) -> &'static str { if value { "On" } else { "Off" } }

struct Paint<'a,'g,'b> {
    game: &'a Game<'g>,
    batch: &'b mut HudBatch,
    mouse: [f32; 2],
    hits: Vec<(Rect, Click)>,
}

impl Paint<'_,'_,'_> {
    fn fill(&mut self, rect: Rect, color: u32) { self.game.native_fill(self.batch,rect,color); }

    fn text(&mut self, text: &str, pos: [f32; 2], width: f32, color: u32) {
        if width < 6.0 { return; }
        if self.game.native_text_width(text) <= width {
            self.game.native_text(self.batch,text,pos,color,true);
            return;
        }
        let limit = width - self.game.native_text_width("...");
        let mut end = text.len();
        while end > 0 && self.game.native_text_width(&text[..end]) > limit {
            end -= 1;
            while end > 0 && !text.is_char_boundary(end) { end -= 1; }
        }
        self.game.native_text(self.batch,&format!("{}...",&text[..end]),pos,color,true);
    }

    fn button(&mut self, rect: Rect, title: &str, active: bool, enabled: bool, action: Click) {
        let hovered = enabled && inside(rect,self.mouse);
        self.fill(rect,match (active,hovered,enabled) {
            (_,_,false) => 0xff222027,
            (true,true,true) => 0xff794db3,
            (true,false,true) => 0xff583583,
            (false,true,true) => 0xff44394e,
            (false,false,true) => 0xff2b2532,
        });
        let [x,y,w,h] = rect;
        self.text(title,[x+5.0,y+(h-8.0)*0.5],w-9.0,if enabled { TEXT } else { MUTED });
        if enabled { self.hits.push((rect,action)); }
    }

    fn stepper(&mut self, rect: Rect, title: &str, value: &str, field: Number, step: i32) {
        let [x,y,w,h] = rect;
        self.fill(rect,0xff2b2439);
        self.text(title,[x+5.0,y+6.0],w-67.0,MUTED);
        self.button([x+w-64.0,y+2.0,15.0,h-4.0],"-",false,true,Click::Number(field,-step));
        self.text(value,[x+w-46.0,y+6.0],29.0,TEXT);
        self.button([x+w-16.0,y+2.0,14.0,h-4.0],"+",false,true,Click::Number(field,step));
    }

    fn chips<const N: usize>(&mut self, [x,y,w,h]: Rect, choices: [(&str,bool,bool,Click); N]) {
        let step = (w + 3.0) / N as f32;
        for (i,(title,active,enabled,action)) in choices.into_iter().enumerate() {
            self.button([x+step*i as f32,y,step-3.0,h],title,active,enabled,action);
        }
    }
}

pub(in crate::game) fn register(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: ID.into(),
        title: "Dihrill".into(),
        description: "Choose targets and start Dihrill from your inventory.".into(),
        keys: Binding::ALL.into_iter().map(|binding| {
            let (name, label, default) = binding.entry();
            [name.into(), label.into(), default.into()]
        }).collect(),
    });
}

fn open(game: &mut Game<'_>) {
    let looked_at = game.pick().map(|hit| hit.pos);
    if !game.native_mod_screen_open(ID) { return; }
    miner::load(game);
    MENU.with(|menu| {
        let mut menu = menu.borrow_mut();
        menu.hits.clear();
        menu.secondary_hits.clear();
        menu.offset = 0;
        menu.filter_dirty = true;
        menu.editor = None;
        menu.capturing = None;
        menu.notice = None;
        menu.looked_at = looked_at;
        if menu.catalog.is_empty() {
            menu.catalog = config::target_catalog(game).into_iter().map(|id| Block {
                label: name(&id).replace('_'," "),
                ore: id.ends_with("_ore") || id == "minecraft:ancient_debris",
                id,
            }).collect();
            menu.catalog.sort_unstable_by(|a,b| a.id.cmp(&b.id));
        }
    });
}

pub(super) fn request_open(game: &mut Game<'_>) {
    match game.screen {
        Screen::Playing => open(game),
        Screen::Chat => MENU.with(|menu| {
            menu.borrow_mut().open_until = Some(Instant::now() + Duration::from_secs(2));
        }),
        _ => (),
    }
}

pub(in crate::game) fn open_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || game.screen != Screen::Playing || game.title.is_some() {
        return;
    }
    if key == UNBOUND { return; }
    if key == Binding::Menu.key(game) {
        open(game);
    } else if key == Binding::Pause.key(game) {
        let run = miner::snapshot(game);
        if run.running {
            miner::pause_from_menu(game);
        } else if run.paused {
            miner::resume_from_menu(game);
        }
    } else if key == Binding::Hud.key(game) {
        miner::update_hud(game, |hud| hud.visible = !hud.visible);
    } else if key == Binding::Stop.key(game) {
        let run = miner::snapshot(game);
        if run.running || run.paused { miner::stop_from_menu(game); }
    }
}

fn personal(game: &Game<'_>) -> bool {
    game.title.is_none() && game.mode_id() == 0 && game.screen == Screen::Inventory
        && game.remote_menu().0 == 0 && game.container.is_none() && game.station.is_none()
}

fn rebuild_filter(menu: &mut Menu, settings: &Settings) {
    if !menu.filter_dirty { return; }
    menu.filter_dirty = false;
    menu.matches.clear();
    let query = menu.search.trim().to_ascii_lowercase().replace(' ',"_");
    match menu.filter {
        Filter::Selected => menu.matches.extend(settings.targets.iter().filter_map(|id|
            menu.catalog.binary_search_by(|block| block.id.cmp(id)).ok()
        )),
        filter => menu.matches.extend(menu.catalog.iter().enumerate().filter_map(|(index,block)|
            ((filter == Filter::All || block.ore) && block.id.contains(&query)).then_some(index)
        )),
    }
}

fn targets(p: &mut Paint<'_,'_,'_>, menu: &mut Menu, settings: &Settings, [x,y,w,h]: Rect) {
    let filter = menu.filter;
    p.chips([x,y,w,16.0], [
        ("Ores",filter == Filter::Ores,true,Click::Filter(Filter::Ores)),
        ("All blocks",filter == Filter::All,true,Click::Filter(Filter::All)),
        ("Priority order",filter == Filter::Selected,true,Click::Filter(Filter::Selected)),
    ]);
    if filter != Filter::Selected {
        p.fill([x,y+20.0,w-102.0,18.0],0xff2b2338);
        let empty = menu.search.is_empty();
        let value = if empty { "Type a block, group, or #tag" } else { &menu.search };
        p.text(value,[x+5.0,y+25.0],w-110.0,if empty { MUTED } else { TEXT });
        p.button([x+w-100.0,y+20.0,73.0,18.0],"Add ID / tag",false,!empty,Click::AddQuery);
        p.button([x+w-24.0,y+20.0,24.0,18.0],"x",false,!empty,Click::ClearQuery);
    } else {
        p.text("Highest priority first. Use the arrows to reorder.",[x+3.0,y+25.0],w-6.0,MUTED);
    }
    rebuild_filter(menu,settings);
    let list_y = y + 42.0;
    let rows = (((h-58.0)/ROW).floor() as usize).max(1);
    menu.offset = menu.offset.min(menu.matches.len().saturating_sub(rows));
    p.fill([x,list_y,w-21.0,rows as f32*ROW],0xff221b2d);
    if menu.matches.is_empty() {
        let hint = if filter == Filter::Selected { "Select blocks from Ores or All blocks." } else { "No matching blocks." };
        p.text(hint,[x+7.0,list_y+6.0],w-35.0,MUTED);
    }
    for (row,&index) in menu.matches.iter().skip(menu.offset).take(rows).enumerate() {
        let block = &menu.catalog[index];
        let priority = settings.targets.iter().position(|id| id == &block.id);
        let row_y = list_y + row as f32*ROW;
        let button_w = w - if filter == Filter::Selected { 59.0 } else { 21.0 };
        p.button([x,row_y,button_w,ROW-1.0],"",priority.is_some(),
            priority.is_some() || settings.targets.len() < config::MAX_TARGETS,Click::Target(block.id.clone()));
        p.game.native_item(p.batch,&block.id,[x+5.0,row_y+2.0],13.0);
        p.text(&block.label,[x+24.0,row_y+5.0],button_w-56.0,TEXT);
        if let Some(priority) = priority {
            p.text(&format!("#{}",priority+1),[x+button_w-29.0,row_y+5.0],26.0,AMBER);
            if filter == Filter::Selected {
                p.button([x+button_w+2.0,row_y,17.0,17.0],"^",false,priority>0,Click::Priority(block.id.clone(),true));
                p.button([x+button_w+21.0,row_y,17.0,17.0],"v",false,priority+1<settings.targets.len(),Click::Priority(block.id.clone(),false));
            }
        }
    }
    p.button([x+w-18.0,list_y,18.0,17.0],"^",false,menu.offset>0,Click::Previous);
    p.button([x+w-18.0,list_y+19.0,18.0,17.0],"v",false,menu.offset+rows<menu.matches.len(),Click::Next);
    p.text(&format!("{} selected  /  {} matches",settings.targets.len(),menu.matches.len()),[x+2.0,y+h-11.0],w-4.0,MUTED);
}

fn mining(p: &mut Paint<'_,'_,'_>, s: &Settings, [x,y,w,_]: Rect) {
    let choice = |profile| (profile_label(profile),s.profile==profile,true,Click::Profile(profile));
    p.chips([x,y,w,17.0],[choice(Profile::Hunter),choice(Profile::Tunnel),choice(Profile::Branch)]);
    p.chips([x,y+20.0,w,17.0],[choice(Profile::Cave),choice(Profile::Area),choice(Profile::Quantity)]);
    let title_width = w * 0.55;
    p.text(&goal_label(&s.goal),[x+2.0,y+43.0],title_width-4.0,AMBER);
    let time = if s.run_seconds == 0 { "Run timer  Off".into() }
        else { format!("Run timer  {}",duration_label(s.run_seconds)) };
    p.button([x+title_width,y+38.0,w-title_width,16.0],&time,s.run_seconds>0,true,Click::Edit(Field::RunTime));
    p.chips([x,y+55.0,w,17.0], [
        ("Unlimited",matches!(s.goal,Goal::Unlimited),true,Click::Unlimited),
        ("Block count",matches!(s.goal,Goal::Blocks(_)),true,Click::Edit(Field::Blocks)),
        ("Item count",matches!(s.goal,Goal::Items(_, _)),true,Click::Edit(Field::Items)),
    ]);
    if s.profile == Profile::Area {
        let summary = s.area.map_or_else(|| "Set two corners to bound the mining area.".into(),
            |(a,b)| format!("Area  {}  to  {}",coords(a),coords(b)));
        p.text(&summary,[x+2.0,y+83.0],w,AMBER);
        p.chips([x,y+96.0,w,17.0], [
            ("Set coordinates",false,true,Click::Edit(Field::Area)),
            ("Clear area",false,s.area.is_some(),Click::ClearArea),
        ]);
        return;
    }
    let half = (w-3.0)*0.5;
    p.stepper([x,y+77.0,half,19.0],"Search radius",&format!("{} b",s.radius),Number::Radius,4);
    p.button([x+half+3.0,y+77.0,half,19.0],&format!("Explore  {}",on_off(s.explore)),s.explore,true,Click::Explore);
    p.stepper([x,y+98.0,half,19.0],"Explore limit",&format!("{} b",s.explore_limit),Number::ExploreLimit,8);
    p.stepper([x+half+3.0,y+98.0,half,19.0],"Branch spacing",&format!("{} b",s.branch_spacing),Number::BranchSpacing,1);
    p.stepper([x,y+119.0,half,19.0],"Tunnel width",&format!("{} b",s.tunnel_width),Number::Width,1);
    p.stepper([x+half+3.0,y+119.0,half,19.0],"Tunnel height",&format!("{} b",s.tunnel_height),Number::Height,1);
    let label = format!("Continuous exploration {}",on_off(s.continuous_explore));
    p.button([x,y+140.0,w,17.0],&label,s.continuous_explore,s.explore,Click::ContinuousExplore);
}

fn safety(p: &mut Paint<'_,'_,'_>, s: &Settings, [x,y,w,_]: Rect) {
    p.text("Lava, falling blocks and unsafe footing are avoided.",[x+2.0,y+3.0],w,MUTED);
    let half = (w-3.0)*0.5;
    p.stepper([x,y+17.0,half,20.0],"Maximum fall",&format!("{} b",s.max_fall),Number::MaxFall,1);
    p.stepper([x+half+3.0,y+17.0,half,20.0],"Turn speed",&format!("{} /t",s.turn_speed),Number::Turn,3);
    p.stepper([x,y+40.0,half,20.0],"Tool reserve",&s.durability.to_string(),Number::Durability,5);
    p.button([x+half+3.0,y+40.0,half,20.0],&format!("Sprint  {}",on_off(s.sprint)),s.sprint,true,Click::Sprint);
    p.button([x,y+63.0,half,20.0],&format!("Diagonal veins  {}",on_off(s.diagonals)),s.diagonals,true,Click::Diagonals);
    let pace = match s.pace { Pace::Calm => "Pace  Calm", Pace::Normal => "Pace  Normal" };
    p.button([x+half+3.0,y+63.0,half,20.0],pace,false,true,Click::Pace);
    p.button([x,y+86.0,half,18.0],&format!("Shallow wading  {}",on_off(s.shallow_water)),
        s.shallow_water,true,Click::ShallowWater);
    p.text("Tool preference",[x+half+7.0,y+91.0],half-7.0,MUTED);
    p.chips([x,y+109.0,w,18.0], [
        ("Fastest",s.tool==Tool::Fast,true,Click::Tool(Tool::Fast)),
        ("Fortune",s.tool==Tool::Fortune,true,Click::Tool(Tool::Fortune)),
        ("Silk Touch",s.tool==Tool::Silk,true,Click::Tool(Tool::Silk)),
    ]);
    p.button([x,y+131.0,w,15.0],"Cave / mob avoidance",true,true,Click::Page(Page::Avoidance));
    p.button([x,y+149.0,w,15.0],"Vertical digging",false,true,Click::Page(Page::Vertical));
}

fn vertical(p: &mut Paint<'_,'_,'_>, s: &Settings, run: &Snapshot, [x,y,w,_]: Rect) {
    p.text("Stand near the center of a full block.",[x+2.0,y+3.0],w-4.0,TEXT);
    p.text("Dig down lands on solid ground one block at a time.",[x+2.0,y+20.0],w-4.0,MUTED);
    p.text("Dig up needs cobblestone or cobbled deepslate in the hotbar.",[x+2.0,y+35.0],w-4.0,MUTED);
    p.button([x,y+53.0,w,17.0],&format!("Shaft length  {} blocks",s.shaft_length),
        false,true,Click::Edit(Field::ShaftLength));
    let ready = !run.running && !run.paused;
    p.chips([x,y+77.0,w,21.0], [
        ("Dig down",false,ready,Click::StartShaft(ShaftDirection::Down)),
        ("Dig up",false,ready,Click::StartShaft(ShaftDirection::Up)),
    ]);
    p.button([x,y+104.0,w,17.0],"Back to Safety",false,true,Click::Page(Page::Safety));
}

fn avoidance(p: &mut Paint<'_,'_,'_>, s: &Settings, [x,y,w,_]: Rect) {
    p.chips([x,y+4.0,w,19.0], [
        (&format!("Cave avoidance  {}",on_off(s.avoid_caves)),s.avoid_caves,true,Click::AvoidCaves),
        (&format!("Mob avoidance  {}",on_off(s.avoid_mobs)),s.avoid_mobs,true,Click::AvoidMobs),
    ]);
    p.text("Avoid large underground openings and cave breakthroughs.",[x+2.0,y+31.0],w-4.0,TEXT);
    p.text("Detour around hostile mobs and resume automatically.",[x+2.0,y+45.0],w-4.0,TEXT);
    p.stepper([x,y+59.0,w,20.0],"Mob detection radius",&format!("{} blocks",s.mob_radius),Number::MobRadius,2);
    p.chips([x,y+85.0,w,18.0], [
        ("Auto shelter",s.shelter_on_stop,true,Click::Shelter),
        ("Retreat wall",s.retreat_barricade,true,Click::Barricade),
        ("Emergency exit",s.emergency_exit,true,Click::EmergencyExit),
    ]);
    p.text("Shelter and retreat walls use ordinary hotbar cobble.",[x+2.0,y+112.0],w-4.0,MUTED);
}

fn module_action(module: Module, s: &Settings, run: &Snapshot) -> (bool, bool, Click) {
    let idle = !run.running && !run.paused;
    match module {
        Module::Mining if !run.finding && run.running => (true, true, Click::Pause),
        Module::Mining if !run.finding && run.paused => (true, true, Click::Resume),
        Module::Mining => (false, idle && !s.targets.is_empty(), Click::Start),
        Module::Finder if run.finding && run.running => (true, true, Click::Pause),
        Module::Finder if run.finding && run.paused => (true, true, Click::Resume),
        Module::Finder => (false, idle && !s.targets.is_empty(), Click::Find),
        Module::Shaft => (run.shaft.is_some(), true, Click::Page(Page::Vertical)),
        Module::Inventory => (!s.junk.is_empty() || s.storage.is_some(), true, Click::Page(Page::Inventory)),
        Module::Caves => (s.avoid_caves, true, Click::AvoidCaves),
        Module::Mobs => (s.avoid_mobs, true, Click::AvoidMobs),
        Module::Shelter => (s.shelter_on_stop, true, Click::Shelter),
        Module::Shop => (s.diamond_shop.is_some(), true, Click::Page(Page::Shop)),
        Module::Hud => (s.hud.visible, true, Click::HudVisible),
        Module::Bindings => (false, true, Click::Page(Page::Bindings)),
        Module::Status => (run.running || run.paused, true, Click::Page(Page::Status)),
    }
}

fn modules(p: &mut Paint<'_,'_,'_>, menu: &mut Menu, s: &Settings, run: &Snapshot, [x,y,w,h]: Rect) {
    const COLUMNS: [(&str, &[Module]); 3] = [
        ("Mining", &[Module::Mining, Module::Finder, Module::Shaft, Module::Inventory]),
        ("Safety", &[Module::Caves, Module::Mobs, Module::Shelter, Module::Shop]),
        ("Display", &[Module::Hud, Module::Bindings, Module::Status]),
    ];
    let gap = 5.0;
    let column_width = (w - 2.0 * gap) / 3.0;
    let mut hint = "Right-click a module to show its settings.";
    for (column, (title, list)) in COLUMNS.into_iter().enumerate() {
        let left = x + column as f32 * (column_width + gap);
        p.fill([left,y,column_width,h-14.0],0xd9131119);
        p.fill([left,y,column_width,19.0],0xff21192c);
        p.text(title,[left+5.0,y+5.0],column_width-9.0,TEXT);
        let mut row = y + 21.0;
        for &module in list {
            let (title, description) = module.label();
            let rect = [left+2.0,row,column_width-4.0,17.0];
            let (active, enabled, action) = module_action(module,s,run);
            p.button(rect,title,active,enabled,action);
            menu.secondary_hits.push((rect,module));
            if inside(rect,p.mouse) { hint = description; }
            row += 19.0;
            if menu.expanded != Some(module) { continue; }
            let details: &[(&str, Click, bool)] = match module {
                Module::Mining => &[("Targets",Click::Page(Page::Targets),true),("Mining settings",Click::Page(Page::Mining),true)],
                Module::Finder => &[("Targets",Click::Page(Page::Targets),true),("Search settings",Click::Page(Page::Mining),true)],
                Module::Shaft => &[("Shaft settings",Click::Page(Page::Vertical),true)],
                Module::Inventory => &[("Junk and storage",Click::Page(Page::Inventory),true)],
                Module::Caves => &[("Safety settings",Click::Page(Page::Safety),true)],
                Module::Mobs => &[("Detection radius",Click::Page(Page::Avoidance),true),("Retreat wall",Click::Barricade,s.retreat_barricade)],
                Module::Shelter => &[("Safety options",Click::Page(Page::Avoidance),true)],
                Module::Shop => &[("Shop settings",Click::Page(Page::Shop),true)],
                Module::Hud => &[("HUD settings",Click::Page(Page::Hud),true),("Counters",Click::HudCounters,s.hud.counters)],
                Module::Bindings => &[("Edit bindings",Click::Page(Page::Bindings),true)],
                Module::Status => &[("Run details",Click::Page(Page::Status),true)],
            };
            for (label, action, active) in details {
                p.button([left+8.0,row,column_width-10.0,14.0],label,*active,true,action.clone());
                row += 16.0;
            }
        }
    }
    p.text(hint,[x+2.0,y+h-9.0],w-4.0,MUTED);
}

fn hud_page(p: &mut Paint<'_,'_,'_>, s: &Settings, [x,y,w,_]: Rect) {
    let hud = s.hud;
    p.chips([x,y,w,18.0], [
        ("HUD",hud.visible,true,Click::HudVisible),
        ("Counters",hud.counters,true,Click::HudCounters),
        ("Timer",hud.timer,true,Click::HudTimer),
        ("Destination",hud.destination,true,Click::HudDestination),
    ]);
    let position = |title, anchor| (title,hud.position==anchor,true,Click::HudPosition(anchor));
    p.text("Position",[x+2.0,y+25.0],w-4.0,MUTED);
    p.chips([x,y+38.0,w,18.0], [
        position("Hotbar",HudPosition::AboveHotbar),
        position("Top left",HudPosition::TopLeft),
        position("Top right",HudPosition::TopRight),
    ]);
    p.text(&format!("Background {}%",hud.background),[x+2.0,y+72.0],w-50.0,TEXT);
    p.button([x+w-45.0,y+65.0,20.0,18.0],"-",false,hud.background>0,Click::HudBackground(-10));
    p.button([x+w-22.0,y+65.0,20.0,18.0],"+",false,hud.background<100,Click::HudBackground(10));
    p.button([x,y+92.0,w,18.0],if hud.amber { "Text color  Amber" } else { "Text color  White" },
        false,true,Click::HudAmber);
}

fn binding_label(key: &str) -> String {
    if key == UNBOUND || key.is_empty() { return "Unbound".into(); }
    key.strip_prefix("key.keyboard.").unwrap_or(key).replace('.', " ").to_uppercase()
}

fn bindings(p: &mut Paint<'_,'_,'_>, menu: &Menu, [x,y,w,_]: Rect) {
    for (i,binding) in Binding::ALL.into_iter().enumerate() {
        let (_,title,default) = binding.entry();
        let row = y + 22.0*i as f32;
        let key = binding.key(p.game);
        let capturing = menu.capturing==Some(binding);
        let label = if capturing { "Press a key...".into() } else { binding_label(&key) };
        p.text(title,[x+2.0,row+5.0],w*0.48-3.0,TEXT);
        p.button([x+w*0.49,row,w*0.51-53.0,18.0],&label,capturing,true,Click::Capture(binding));
        p.button([x+w-49.0,row,49.0,18.0],"Reset",false,key!=default,Click::SetBinding(binding,None));
    }
    p.text("While binding, Backspace clears and Esc cancels.",[x+2.0,y+94.0],w-4.0,MUTED);
}

fn inventory(p: &mut Paint<'_,'_,'_>, menu: &Menu, s: &Settings, [x,y,w,_]: Rect) {
    p.text("When full, drop every stack of checked junk.",[x+2.0,y+2.0],w,MUTED);
    let cell = (w+3.0)/3.0;
    for (index,&id) in config::JUNK.iter().enumerate() {
        let checked = s.junk.iter().any(|item| item==id);
        let label = format!("{} {}",if checked { "[x]" } else { "[ ]" },name(id).replace('_'," "));
        p.button([x+(index%3) as f32*cell,y+14.0+(index/3) as f32*15.0,cell-3.0,14.0],
            &label,checked,true,Click::Junk(id));
    }
    let controls_y = y + 17.0 + config::JUNK.len().div_ceil(3) as f32*15.0;
    p.chips([x,controls_y,w,17.0], [
        ("Clear junk list",false,!s.junk.is_empty(),Click::ClearJunk),
        ("Protected items",false,true,Click::Edit(Field::Protected)),
        (if s.alert_inventory { "Alerts  On" } else { "Alerts  Off" },s.alert_inventory,true,Click::Alerts),
    ]);
    let storage = s.storage.map_or_else(|| "Storage off".into(),|pos| format!("Storage  {}",coords(pos)));
    p.text(&storage,[x+2.0,controls_y+23.0],w,AMBER);
    p.chips([x,controls_y+35.0,w,18.0], [
        ("Set storage",false,true,Click::Edit(Field::Storage)),
        ("Use looked-at block",false,menu.looked_at.is_some(),Click::StorageLook),
        ("Disable storage",false,s.storage.is_some(),Click::ClearStorage),
    ]);
    p.button([x,controls_y+57.0,w,17.0],"Diamond shop",s.diamond_shop.is_some(),true,Click::Page(Page::Shop));
}

fn shop(p: &mut Paint<'_,'_,'_>, s: &Settings, [x,y,w,_]: Rect) {
    let server = super::dihrill_shop::server(p.game);
    let label = match &s.diamond_shop {
        Some(menu) => format!("Enabled on {}",menu.address),
        None => "Automatic diamond selling is off".into(),
    };
    for (row,line) in [label.as_str(), "Sells up to two stacks, including partial stacks.",
        "Unsold diamonds stay in your inventory.",
    ].into_iter().enumerate() {
        p.text(line,[x+2.0,y+4.0+row as f32*13.0],w-4.0,if row==0 { AMBER } else { TEXT });
    }
    p.chips([x,y+49.0,w,18.0], [
        (&format!("Minimum: {} diamonds",s.shop_min_diamonds),false,true,Click::Edit(Field::ShopCount)),
        (&format!("Wait: {} s",s.shop_interval_seconds),false,true,Click::Edit(Field::ShopInterval)),
    ]);
    p.chips([x,y+74.0,w,18.0], [
        ("Open /shop and enable",true,server.is_some(),Click::LearnShop),
        ("Turn off",false,s.diamond_shop.is_some(),Click::DisableShop),
    ]);
    let help = if server.is_some() { "Setup opens the shop without clicking." }
        else { "Connect to the shop server to set it up." };
    p.text(help,[x+2.0,y+102.0],w-4.0,MUTED);
    p.button([x,y+117.0,w,17.0],"Back to Inventory",false,true,Click::Page(Page::Inventory));
}

fn status(p: &mut Paint<'_,'_,'_>, s: &Settings, run: &Snapshot, [x,y,w,_]: Rect) {
    let details = [
        format!("{}  /  {}",profile_label(s.profile),run.state),
        format!("Destination  {}",run.destination.map_or_else(|| "—".into(),coords)),
        format!("Vein  {}",run.vein.map_or_else(|| "—".into(),|(mined,known)| format!("{mined}/{known} blocks"))),
        format!("{} targets mined  /  {} filler cleared",run.mined,run.tunneled),
        format!("{} items collected  /  {} junk stacks dropped",run.collected,run.discarded),
        format!("{:.1} blocks traveled",run.distance),
        format!("Tool durability  {}",run.durability.map_or_else(|| "—".into(),|(left,max)| format!("{left}/{max}"))),
        goal_label(&s.goal),
        run.remaining.map_or_else(|| "Run timer  —".into(), |seconds| {
            format!("Run timer  {} left",duration_label(seconds))
        }),
        run.search_checked.map_or_else(String::new, |count| format!("Route search  {count} positions checked")),
        run.shaft.map_or_else(String::new, |(done, limit)| format!("Vertical travel  {done}/{limit} blocks")),
    ];
    for (row,line) in details.iter().enumerate() {
        p.text(line,[x+2.0,y+row as f32*13.0],w-4.0,if row==0 { AMBER } else { TEXT });
    }
}

fn editor(p: &mut Paint<'_,'_,'_>, edit: &Editor, [x,y,w,_]: Rect) {
    p.text(edit.field.hint(),[x+3.0,y+7.0],w-6.0,AMBER);
    p.fill([x+2.0,y+23.0,w-4.0,20.0],if edit.replace { 0xff4d3870 } else { 0xff362b45 });
    let mut start = 0;
    let end = edit.value.len();
    while start < end && p.game.native_text_width(&edit.value[start..]) > w-14.0 {
        start += edit.value[start..].chars().next().map_or(0, char::len_utf8);
    }
    p.text(&edit.value[start..],[x+7.0,y+29.0],w-14.0,TEXT);
    if matches!(edit.field, Field::RunTime) {
        let duration = run_duration(&edit.value);
        let preview = match duration {
            Some(0) => "Timer off".into(),
            Some(seconds) => format!("Stop after {} of active play", duration_label(seconds)),
            None => "Enter a valid duration".into(),
        };
        p.text(&preview,[x+3.0,y+52.0],w-6.0,AMBER);
        p.text("25 min   /   8:45   /   2:15:30   /   3h5m   /   17s",
            [x+3.0,y+67.0],w-6.0,MUTED);
        p.chips([x+2.0,y+84.0,w-4.0,18.0], [
            ("Apply",true,duration.is_some(),Click::Apply),
            ("Turn off",false,true,Click::TimerOff),
            ("Cancel",false,true,Click::CancelEdit),
        ]);
    } else {
        p.chips([x+2.0,y+50.0,w-4.0,18.0], [
            ("Apply",true,true,Click::Apply),
            ("Cancel",false,true,Click::CancelEdit),
        ]);
    }
}

pub(in crate::game) fn render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    let owned = game.native_mod_screen() == Some(ID);
    if !owned && !personal(game) { return; }
    MENU.with(|menu| {
        let mut menu = menu.borrow_mut();
        let [vw,vh] = [batch.width,batch.height];
        let mut p = Paint { game, batch, mouse:screen.mouse, hits:std::mem::take(&mut menu.hits) };
        p.hits.clear();
        menu.secondary_hits.clear();
        if !owned {
            let [left,top,_,_] = screen.rect;
            let (x,y) = if left >= 91.0 && top > 15.0 { (left-87.0,top) } else { (6.0,6.0) };
            p.button([x,y,82.0,16.0],"Dihrill",true,true,Click::Inventory);
            menu.hits = p.hits;
            return;
        }
        let settings = miner::settings();
        let run = miner::snapshot(game);
        let max_width = if menu.page == Page::Modules { 500.0 } else { 388.0 };
        let w = (vw-12.0).clamp(246.0,max_width);
        let min_height = if menu.page == Page::Mining { 242.0 } else { 232.0 };
        let h = (vh-8.0).clamp(min_height,280.0);
        let x = (vw-w)*0.5;
        let y = (vh-h)*0.5;
        let end = y+h;
        p.fill([x-2.0,y-2.0,w+4.0,h+4.0],0x8308050c);
        p.fill([x,y,w,h],0xee15121b);
        p.fill([x,y,w,2.0],0xff8950c7);
        if menu.page == Page::Modules {
            p.text("DIHRILL",[x+9.0,y+10.0],w-131.0,AMBER);
        } else {
            p.button([x+9.0,y+6.0,64.0,15.0],"Modules",false,true,Click::Page(Page::Modules));
        }
        p.text(run.state,[x+w-118.0,y+10.0],90.0,MUTED);
        p.button([x+w-23.0,y+5.0,17.0,16.0],"x",false,true,Click::Close);
        let area = if menu.page == Page::Modules {
            [x+9.0,y+27.0,w-18.0,h-69.0]
        } else {
            [x+9.0,y+47.0,w-18.0,h-89.0]
        };
        if let Some(edit) = &menu.editor {
            editor(&mut p,edit,area);
        } else {
            if menu.page != Page::Modules {
                p.chips([x+9.0,y+26.0,w-18.0,16.0],PAGES.map(|(page,title)| {
                    (title,menu.page==page,true,Click::Page(page))
                }));
            }
            match menu.page {
                Page::Modules => modules(&mut p,&mut menu,&settings,&run,area),
                Page::Targets => targets(&mut p,&mut menu,&settings,area),
                Page::Mining => mining(&mut p,&settings,area),
                Page::Safety => safety(&mut p,&settings,area),
                Page::Inventory => inventory(&mut p,&menu,&settings,area),
                Page::Status => status(&mut p,&settings,&run,area),
                Page::Shop => shop(&mut p,&settings,area),
                Page::Avoidance => avoidance(&mut p,&settings,area),
                Page::Vertical => vertical(&mut p,&settings,&run,area),
                Page::Hud => hud_page(&mut p,&settings,area),
                Page::Bindings => bindings(&mut p,&menu,area),
            }
            p.chips([x+9.0,end-38.0,w-18.0,19.0], [
                ("Start",!run.running&&!run.paused,menu.page!=Page::Vertical&&!settings.targets.is_empty()&&!run.running&&!run.paused,Click::Start),
                ("Pause",false,run.running,Click::Pause),
                ("Resume",run.paused,run.paused,Click::Resume),
                ("Stop",false,run.running||run.paused,Click::Stop),
            ]);
        }
        p.fill([x+9.0,end-16.0,w-18.0,1.0],0xff4d3b62);
        p.text(menu.notice.as_deref().unwrap_or(&run.message),[x+9.0,end-12.0],w-18.0,AMBER);
        menu.hits = p.hits;
    });
}

pub(in crate::game) fn render_hud(game: &Game<'_>, batch: &mut HudBatch) {
    if game.screen != Screen::Playing || game.player.is_none() { return; }
    let config = miner::hud_settings();
    if !config.visible { return; }
    HUD.with(|hud| {
        let mut hud = hud.borrow_mut();
        let now = Instant::now();
        if hud.refresh.is_none_or(|time| now>=time) {
            let run = miner::snapshot(game);
            hud.text = if run.running || run.paused {
                let mut text = format!("Dihrill  {}", run.state);
                if config.counters && !run.finding {
                    text.push_str(&format!("  /  {} mined", run.mined));
                }
                if config.timer && let Some(seconds) = run.remaining {
                    text.push_str(&format!("  /  {} left", duration_label(seconds)));
                }
                if config.destination && let Some(pos) = run.destination {
                    text.push_str(&format!("  /  {}", coords(pos)));
                }
                text
            } else {
                String::new()
            };
            hud.refresh = Some(now+Duration::from_millis(220));
        }
        if !hud.text.is_empty() {
            let width = (game.native_text_width(&hud.text)+12.0).min(batch.width-10.0);
            let (x,y) = match config.position {
                HudPosition::AboveHotbar => ((batch.width-width)*0.5,(batch.height-43.0).max(0.0)),
                HudPosition::TopLeft => (5.0,5.0),
                HudPosition::TopRight => (batch.width-width-5.0,5.0),
            };
            let alpha = u32::from(config.background)*255/100;
            game.native_fill(batch,[x,y,width,14.0],(alpha<<24)|0x17131d);
            let mut p = Paint { game, batch, mouse:[0.0; 2], hits:Vec::new() };
            p.text(&hud.text,[x+6.0,y+3.0],width-12.0,if config.amber { AMBER } else { TEXT });
        }
    });
}

pub(in crate::game) fn click(game: &Game<'_>, point: [f32; 2], button: i32, canceled: &mut bool) {
    if *canceled || !matches!(button, 0 | 1) { return; }
    let owned = game.native_mod_screen() == Some(ID);
    if !owned && !personal(game) { return; }
    MENU.with(|menu| {
        let mut menu = menu.borrow_mut();
        if button == 1 {
            if owned && let Some(module) = menu.secondary_hits.iter()
                .find_map(|(rect,module)| inside(*rect,point).then_some(*module))
            {
                menu.pending = Some(Click::Expand(module));
                *canceled = true;
            }
            return;
        }
        let action = menu.hits.iter().find_map(|(rect,action)| {
            (inside(*rect,point) && (owned != matches!(action,Click::Inventory))).then(|| action.clone())
        });
        if let Some(action) = action {
            menu.pending = Some(action);
            *canceled = true;
        }
    });
}

fn number(settings: &mut Settings, field: Number, delta: i32) {
    match field {
        Number::Radius => settings.radius += delta,
        Number::ExploreLimit => settings.explore_limit = settings.explore_limit.saturating_add_signed(delta),
        Number::BranchSpacing => settings.branch_spacing = settings.branch_spacing.saturating_add_signed(delta),
        Number::Width => settings.tunnel_width = settings.tunnel_width.saturating_add_signed(delta as i8),
        Number::Height => settings.tunnel_height = settings.tunnel_height.saturating_add_signed(delta as i8),
        Number::MaxFall => settings.max_fall = settings.max_fall.saturating_add_signed(delta as i8),
        Number::Turn => settings.turn_speed = settings.turn_speed.saturating_add_signed(delta as i8),
        Number::Durability => settings.durability = settings.durability.saturating_add_signed(delta),
        Number::MobRadius => settings.mob_radius = settings.mob_radius.saturating_add_signed(delta as i8),
    }
}

fn duration_label(seconds: u32) -> String {
    if seconds >= 3600 {
        format!("{}h {:02}m {:02}s",seconds / 3600,seconds / 60 % 60,seconds % 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s",seconds / 60,seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn timer_input(seconds: u32) -> String {
    format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
}

fn run_duration(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("off") {
        return Some(0);
    }
    if let Ok(minutes) = value.parse::<u32>() { return minutes.checked_mul(60); }
    if value.contains(':') {
        let mut parts = value.split(':');
        let first = parts.next()?.parse::<u32>().ok()?;
        let second = parts.next()?.parse::<u32>().ok().filter(|&n| n < 60)?;
        return match parts.next() {
            None => first.checked_mul(60)?.checked_add(second),
            Some(part) if parts.next().is_none() => {
                let third = part.parse::<u32>().ok().filter(|&n| n < 60)?;
                first.checked_mul(3600)?.checked_add(second * 60 + third)
            }
            Some(_) => None,
        };
    }
    let mut remaining = value;
    let mut seconds = 0u32;
    let mut previous_unit = u32::MAX;
    while !remaining.is_empty() {
        let digits = remaining.bytes().take_while(u8::is_ascii_digit).count();
        let count = remaining[..digits].parse::<u32>().ok()?;
        remaining = &remaining[digits..];
        let unit = match remaining.as_bytes().first()?.to_ascii_lowercase() {
            b'd' => 86400,
            b'h' => 3600,
            b'm' => 60,
            b's' => 1,
            _ => return None,
        };
        if unit >= previous_unit { return None; }
        previous_unit = unit;
        seconds = seconds.checked_add(count.checked_mul(unit)?)?;
        remaining = remaining[1..].trim_start();
    }
    (!value.is_empty()).then_some(seconds)
}

fn quantity(token: Option<&str>) -> Result<u32, String> {
    token.and_then(|text| text.parse::<u32>().ok()).filter(|count| (1..=100_000).contains(count))
        .ok_or_else(|| "Enter a count from 1 to 100000.".into())
}

fn parse_coords<const N: usize>(value: &str) -> Result<[i32; N], String> {
    let input = value.split(|ch: char| ch.is_ascii_whitespace() || ch == ',').filter(|part| !part.is_empty());
    let values: Result<Vec<i32>, _> = input.map(str::parse::<i32>).collect();
    values.ok().and_then(|values| values.try_into().ok())
        .ok_or_else(|| format!("Enter {N} whole-number coordinates."))
}

fn apply_editor(game: &mut Game<'_>, edit: &Editor) -> Result<(), String> {
    match edit.field {
        Field::Blocks => {
            let count = quantity(Some(edit.value.trim()))?;
            miner::update_settings(game, |settings| settings.goal = Goal::Blocks(count));
        }
        Field::Items => {
            let mut words = edit.value.split_ascii_whitespace();
            let id = words.next().ok_or("Enter an item ID and a count.")?;
            let item = config::valid_item(game, id)?;
            let count = quantity(words.next())?;
            if words.next().is_some() { return Err("Enter one item ID and one count.".into()); }
            miner::update_settings(game, |settings| settings.goal = Goal::Items(item,count));
        }
        Field::Area => {
            let [x1,y1,z1,x2,y2,z2] = parse_coords(&edit.value)?;
            miner::update_settings(game, |settings| settings.area = Some(([x1,y1,z1],[x2,y2,z2])));
        }
        Field::Storage => {
            let pos = parse_coords(&edit.value)?;
            miner::update_settings(game, |settings| settings.storage = Some(pos));
        }
        Field::RunTime => {
            let seconds = run_duration(&edit.value)
                .ok_or("Enter minutes, a clock time, or a duration with units.")?;
            miner::set_run_timer(game, seconds);
        }
        Field::ShaftLength => {
            let length = edit.value.trim().parse::<u32>().ok()
                .filter(|count| (1..=config::MAX_SHAFT_LENGTH).contains(count))
                .ok_or("Enter a shaft length from 1 to 64 blocks.")?;
            miner::update_settings(game, |settings| settings.shaft_length = length);
        }
        Field::ShopCount | Field::ShopInterval => {
            let maximum = if matches!(edit.field, Field::ShopCount) {
                config::MAX_SHOP_DIAMONDS
            } else {
                config::MAX_SHOP_INTERVAL
            };
            let value = edit.value.trim().parse::<u32>().ok()
                .filter(|count| (1..=maximum).contains(count))
                .ok_or_else(|| format!("Enter a whole number from 1 to {maximum}."))?;
            miner::update_settings(game, |settings| match edit.field {
                Field::ShopCount => settings.shop_min_diamonds = value,
                _ => settings.shop_interval_seconds = value,
            });
        }
        Field::Protected => {
            let words = edit.value.split(|ch: char| ch.is_ascii_whitespace() || ch == ',').filter(|part| !part.is_empty());
            let mut items = Vec::new();
            for word in words {
                let item = config::valid_item(game,word)?;
                if !items.contains(&item) { items.push(item); }
                if items.len() > config::MAX_TARGETS {
                    return Err("Protect up to 64 item IDs.".into());
                }
            }
            miner::update_settings(game, |settings| settings.protected = items);
        }
    }
    Ok(())
}

fn add_query(game: &mut Game<'_>, query: &str) -> Result<(), String> {
    let input: Vec<_> = query.split_ascii_whitespace().collect();
    let additions = config::parse_targets(game,&input)?;
    let mut targets = miner::settings().targets;
    for id in additions {
        if !targets.contains(&id) { targets.push(id); }
    }
    if targets.len() > config::MAX_TARGETS { return Err("Select up to 64 block IDs.".into()); }
    miner::update_settings(game, |settings| settings.targets = targets);
    Ok(())
}

fn apply_action(game: &mut Game<'_>, menu: &mut Menu, action: Click) {
    menu.notice = None;
    match action {
        Click::Page(page) => {
            menu.capturing = None;
            menu.expanded = None;
            menu.page = page;
        }
        Click::Expand(module) => {
            menu.expanded = (menu.expanded!=Some(module)).then_some(module);
        }
        Click::Capture(binding) => menu.capturing = Some(binding),
        Click::SetBinding(binding, key) => {
            let (id, _, default) = binding.entry();
            let chosen = key.as_deref().unwrap_or(default);
            if chosen != UNBOUND && Binding::ALL.into_iter().any(|other| other!=binding && other.key(game)==chosen) {
                menu.notice = Some("That key is already used by Dihrill.".into());
                return;
            }
            game.native_set_mod_key(ID,id,key.as_deref());
            menu.capturing = None;
            menu.notice = Some(format!("Binding set to {}.",binding_label(chosen)));
        }
        Click::Filter(filter) => {
            menu.filter = filter;
            menu.filter_dirty = true;
            menu.offset = 0;
        }
        Click::Target(id) => {
            miner::update_settings(game, |settings| {
                if let Some(index) = settings.targets.iter().position(|value| value==&id) {
                    settings.targets.remove(index);
                } else if settings.targets.len() < config::MAX_TARGETS {
                    settings.targets.push(id);
                }
            });
            menu.filter_dirty = true;
        }
        Click::Priority(id,up) => {
            miner::update_settings(game, |settings| {
                if let Some(index) = settings.targets.iter().position(|value| value==&id) {
                    let adjacent = if up { index.checked_sub(1) } else { Some(index+1) };
                    if let Some(adjacent) = adjacent.filter(|&index| index < settings.targets.len()) {
                        settings.targets.swap(index,adjacent);
                    }
                }
            });
            menu.filter_dirty = true;
        }
        Click::AddQuery => match add_query(game,&menu.search) {
            Ok(()) => {
                menu.filter = Filter::Selected;
                menu.filter_dirty = true;
                menu.offset = 0;
            }
            Err(error) => menu.notice = Some(error),
        },
        Click::ClearQuery => {
            menu.search.clear();
            menu.filter_dirty = true;
            menu.offset = 0;
        }
        Click::Previous => menu.offset = menu.offset.saturating_sub(4),
        Click::Next => menu.offset = menu.offset.saturating_add(4),
        Click::Profile(profile) => miner::update_settings(game, |settings| settings.profile = profile),
        Click::Edit(field) => {
            menu.editor = Some(Editor { field, value:field.value(&miner::settings()), replace:true });
        }
        Click::Apply => {
            if let Some(edit) = &menu.editor {
                match apply_editor(game,edit) {
                    Ok(()) => menu.editor = None,
                    Err(error) => menu.notice = Some(error),
                }
            }
        }
        Click::CancelEdit => menu.editor = None,
        Click::TimerOff => {
            miner::set_run_timer(game, 0);
            menu.editor = None;
        }
        Click::Unlimited => miner::update_settings(game, |settings| settings.goal = Goal::Unlimited),
        Click::ClearArea => miner::update_settings(game, |settings| settings.area = None),
        Click::Number(field,delta) => miner::update_settings(game, |settings| number(settings,field,delta)),
        Click::Explore => miner::update_settings(game, |settings| settings.explore = !settings.explore),
        Click::ContinuousExplore => miner::update_settings(game, |settings| settings.continuous_explore = !settings.continuous_explore),
        Click::ShallowWater => miner::update_settings(game, |s| s.shallow_water = !s.shallow_water),
        Click::Diagonals => miner::update_settings(game, |settings| settings.diagonals = !settings.diagonals),
        Click::Sprint => miner::update_settings(game, |settings| settings.sprint = !settings.sprint),
        Click::Pace => miner::update_settings(game, |settings| {
            settings.pace = if settings.pace == Pace::Calm { Pace::Normal } else { Pace::Calm };
        }),
        Click::Tool(tool) => miner::update_settings(game, |settings| settings.tool = tool),
        Click::Junk(id) => miner::update_settings(game, |settings| {
            if let Some(index) = settings.junk.iter().position(|item| item==id) {
                settings.junk.remove(index);
            } else {
                settings.junk.push(id.into());
            }
        }),
        Click::ClearJunk => miner::update_settings(game, |settings| settings.junk.clear()),
        Click::Alerts => miner::update_settings(game, |settings| settings.alert_inventory = !settings.alert_inventory),
        Click::StorageLook => {
            if let Some(pos) = menu.looked_at {
                miner::update_settings(game, |settings| settings.storage = Some(pos));
            }
        }
        Click::ClearStorage => miner::update_settings(game, |settings| settings.storage = None),
        Click::AvoidCaves => miner::update_settings(game, |settings| settings.avoid_caves = !settings.avoid_caves),
        Click::AvoidMobs => miner::update_settings(game, |settings| settings.avoid_mobs = !settings.avoid_mobs),
        Click::Shelter => miner::update_settings(game, |s| s.shelter_on_stop = !s.shelter_on_stop),
        Click::Barricade => miner::update_settings(game, |s| s.retreat_barricade = !s.retreat_barricade),
        Click::EmergencyExit => miner::update_settings(game, |s| s.emergency_exit = !s.emergency_exit),
        Click::HudVisible => miner::update_hud(game, |hud| hud.visible = !hud.visible),
        Click::HudPosition(position) => miner::update_hud(game, |hud| hud.position = position),
        Click::HudCounters => miner::update_hud(game, |hud| hud.counters = !hud.counters),
        Click::HudTimer => miner::update_hud(game, |hud| hud.timer = !hud.timer),
        Click::HudDestination => miner::update_hud(game, |hud| hud.destination = !hud.destination),
        Click::HudBackground(delta) => miner::update_hud(game, |hud| hud.background = hud.background.saturating_add_signed(delta)),
        Click::HudAmber => miner::update_hud(game, |hud| hud.amber = !hud.amber),
        Click::LearnShop => menu.notice = Some(super::dihrill_shop::start_setup(game)),
        Click::DisableShop => miner::update_settings(game, |settings| settings.diamond_shop = None),
        Click::Start => menu.notice = Some(miner::run_from_menu(game)),
        Click::Find => menu.notice = Some(miner::find_from_menu(game)),
        Click::StartShaft(direction) => menu.notice = Some(miner::run_shaft_from_menu(game, direction)),
        Click::Pause => menu.notice = Some(miner::pause_from_menu(game)),
        Click::Resume => menu.notice = Some(miner::resume_from_menu(game)),
        Click::Stop => menu.notice = Some(miner::stop_from_menu(game)),
        Click::Close => game.native_mod_screen_close(ID),
        Click::Inventory => (),
    }
}

pub(in crate::game) fn frame(game: &mut Game<'_>) {
    let deferred = MENU.with(|menu| {
        let mut menu = menu.borrow_mut();
        let Some(deadline) = menu.open_until else { return false };
        if Instant::now()>deadline || game.title.is_some() {
            menu.open_until = None;
            return false;
        }
        if game.screen == Screen::Playing {
            menu.open_until = None;
            return true;
        }
        false
    });
    if deferred { open(game); }
    let action = MENU.with(|menu| menu.borrow_mut().pending.take());
    let Some(action) = action else { return };
    if matches!(action,Click::Inventory) && personal(game) && game.carried.is_none() {
        game.screen = Screen::Playing;
        open(game);
        if game.native_mod_screen()!=Some(ID) { game.screen = Screen::Inventory; }
        return;
    }
    if game.native_mod_screen()==Some(ID) {
        MENU.with(|menu| apply_action(game,&mut menu.borrow_mut(),action));
    }
}

pub(in crate::game) fn scroll(game: &Game<'_>, amount: f64, canceled: &mut bool) {
    if *canceled || game.native_mod_screen()!=Some(ID) || amount==0.0 || !amount.is_finite() { return; }
    MENU.with(|menu| {
        let mut menu = menu.borrow_mut();
        if menu.page==Page::Targets && menu.editor.is_none() {
            menu.offset = if amount<0.0 { menu.offset.saturating_add(3) } else { menu.offset.saturating_sub(3) };
            *canceled = true;
        }
    });
}

pub(in crate::game) fn character(game: &Game<'_>, ch: char, canceled: &mut bool) {
    if *canceled || game.native_mod_screen()!=Some(ID) { return; }
    if ch.is_ascii_alphanumeric() || matches!(ch,'#'|'_'|'-'|':'|' '|'.'|'/'|',') {
        MENU.with(|menu| {
            let mut menu = menu.borrow_mut();
            if let Some(edit) = &mut menu.editor {
                if std::mem::take(&mut edit.replace) { edit.value.clear(); }
                if edit.value.len()<1024 { edit.value.push(ch); }
            } else if menu.page==Page::Targets && menu.filter!=Filter::Selected && menu.search.len()<120 {
                menu.search.push(ch);
                menu.offset = 0;
                menu.filter_dirty = true;
            }
        });
    }
    *canceled = true;
}

pub(in crate::game) fn key(game: &Game<'_>, key: &str, repeat: bool, canceled: &mut bool) {
    if *canceled || game.native_mod_screen()!=Some(ID) { return; }
    MENU.with(|menu| {
        let mut menu = menu.borrow_mut();
        if let Some(binding) = menu.capturing {
            *canceled = true;
            if repeat { return; }
            match key {
                "key.keyboard.escape" => menu.capturing = None,
                "key.keyboard.backspace" => menu.pending = Some(Click::SetBinding(binding,Some(UNBOUND.into()))),
                key if key.starts_with("key.keyboard.") && key != UNBOUND => {
                    menu.pending = Some(Click::SetBinding(binding,Some(key.into())));
                }
                _ => (),
            }
            return;
        }
        match key {
            "key.keyboard.backspace" => {
                if let Some(edit) = &mut menu.editor {
                    if std::mem::take(&mut edit.replace) { edit.value.clear(); } else { edit.value.pop(); }
                } else if menu.page==Page::Targets {
                    menu.search.pop();
                    menu.offset = 0;
                    menu.filter_dirty = true;
                }
                *canceled = true;
            }
            "key.keyboard.enter" | "key.keyboard.keypad.enter" if !repeat => {
                if menu.editor.is_some() {
                    menu.pending = Some(Click::Apply);
                } else if menu.page==Page::Targets && menu.filter!=Filter::Selected && !menu.search.trim().is_empty() {
                    menu.pending = Some(Click::AddQuery);
                }
                *canceled = true;
            }
            "key.keyboard.escape" if !repeat && menu.editor.is_some() => {
                menu.pending = Some(Click::CancelEdit);
                *canceled = true;
            }
            _ if !repeat && key==game.native_mod_key(ID,"menu",MENU_KEY) => {
                menu.pending = Some(Click::Close);
                *canceled = true;
            }
            _ => (),
        }
    });
}
