use super::{Game, KeyCode};
use super::dihrill as miner;
use super::dihrill_menu as menu;
use super::dihrill_settings::{self as settings, GROUPS, Goal, Profile};
use std::cell::RefCell;

const USAGE: &str = "#dihrill <blocks, groups, or tags> [count] | pause | resume | stop | status | settings";
const ACTIONS: &[&str] = &["help", "pause", "resume", "stop", "status", "settings", "items", "forever"];

#[derive(Default)]
struct Completion {
    prefix: String,
    options: Vec<String>,
    next: usize,
    last: String,
}

thread_local! {
    static COMPLETION: RefCell<Completion> = RefCell::new(Completion::default());
}

pub(in crate::game) fn hash_command(game: &mut Game<'_>, message: &str) -> bool {
    let line = message.trim();
    let mut words = line.splitn(2,char::is_whitespace);
    if words.next() != Some("#dihrill") { return false; }
    execute(game, words.next().unwrap_or(""));
    true
}

pub(in crate::game) fn slash_command(game: &mut Game<'_>, message: &str) -> bool {
    let mut words = message.trim().splitn(2,char::is_whitespace);
    if words.next() != Some("dihrill") { return false; }
    let arguments = words.next().unwrap_or("").trim();
    execute(game, arguments);
    true
}

fn show(game: &mut Game<'_>, message: &str) {
    game.native_chat(&format!("[Dihrill] {message}"));
}

fn execute(game: &mut Game<'_>, arguments: &str) {
    if !game.native_window() || game.player.is_none() || game.title.is_some() || game.remote_loading() {
        show(game,"Enter a loaded Survival world first.");
        return;
    }
    miner::load(game);
    let words = arguments.split_whitespace().collect::<Vec<_>>();
    let reply = match words.as_slice() {
        [] | ["help"] => USAGE.into(),
        ["status"] => status(game),
        ["settings"] => {
            menu::request_open(game);
            return;
        }
        ["pause"] => miner::pause_from_menu(game),
        ["resume"] => miner::resume_from_menu(game),
        ["stop"] => miner::stop_from_menu(game),
        ["items", item, count, targets @ ..] => {
            match settings::valid_item(game,item).and_then(|id| quantity(count).map(|n| Goal::Items(id,n))) {
                Ok(goal) => start(game, targets, goal),
                Err(error) => error,
            }
        }
        ["items", ..] => "Use #dihrill items <item> <count> [blocks].".into(),
        ["forever", targets @ ..] => start(game, targets, Goal::Unlimited),
        _ => {
            let (last, preceding) = words.split_last().unwrap();
            if last.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+') {
                match quantity(last) {
                    Ok(n) => start(game, preceding, Goal::Blocks(n)),
                    Err(error) => error,
                }
            } else {
                start(game, &words, Goal::Unlimited)
            }
        }
    };
    show(game, &reply);
}

fn quantity(count: &str) -> Result<u32, String> {
    count.parse::<u32>().ok().filter(|&n| (1..=100_000).contains(&n))
        .ok_or_else(|| "Choose a quantity from 1 to 100000.".into())
}

fn start(game: &mut Game<'_>, targets: &[&str], goal: Goal) -> String {
    let targets = if targets.is_empty() { miner::settings().targets } else {
        match settings::parse_targets(game, targets) {
            Ok(targets) => targets,
            Err(error) => return error,
        }
    };
    if targets.is_empty() { return "Choose target blocks in the menu or after #dihrill.".into(); }
    miner::update_settings(game, |config| {
        config.targets = targets;
        if goal != Goal::Unlimited {
            config.profile = Profile::Quantity;
        } else if config.profile == Profile::Quantity {
            config.profile = Profile::Hunter;
        }
        config.goal = goal;
    });
    miner::run_from_menu(game)
}

fn status(game: &Game<'_>) -> String {
    let current = miner::snapshot(game);
    let config = miner::settings();
    let countdown = current.remaining.map_or_else(String::new, |seconds| format!(" | {seconds}s left"));
    let search = current.search_checked.map_or_else(String::new, |count| format!(" | {count} positions checked"));
    format!("{} | {} target blocks mined | {} targets selected{}{} | {}",
            current.state, current.mined, config.targets.len(), countdown, search, current.message)
}

pub(in crate::game) fn tab(game: &mut Game<'_>, key: KeyCode) -> bool {
    if key != KeyCode::Tab { return false; }
    let Some((input,cursor)) = game.native_chat_input() else { return false; };
    if cursor != input.chars().count() { return false; }
    let command = if input.starts_with("#dihrill ") { "#dihrill " }
        else if input.starts_with("/dihrill ") { "/dihrill " }
        else { return false; };
    if !game.native_window() || game.player.is_none() || game.title.is_some() || game.remote_loading() { return false; }

    COMPLETION.with(|state| {
        let mut state = state.borrow_mut();
        if input != state.last || state.options.is_empty() {
            let prefix_end = input.rfind(char::is_whitespace).unwrap_or(command.len()-1)+1;
            let typed = &input[prefix_end..];
            let tokens = input[command.len()..prefix_end].split_whitespace().collect::<Vec<_>>();
            let mut options = Vec::new();
            if tokens.is_empty() {
                options.extend(ACTIONS.iter().map(|s| s.to_string()));
            }
            if typed.starts_with('#') {
                options.extend(game.res.items().block_tags.keys().map(|id| {
                    let name = if typed.contains(':') { id } else {
                        id.strip_prefix("minecraft:").unwrap_or(id)
                    };
                    format!("#{name}")
                }));
            } else if tokens.as_slice() == ["items"] {
                options.extend(game.ui.registries.items.iter().map(|id| {
                    if typed.contains(':') { return id.clone(); }
                    id.strip_prefix("minecraft:").unwrap_or(id).to_owned()
                }));
            } else {
                options.extend(GROUPS.iter().map(|(s,_)| s.to_string()));
                options.push("ores".into());
                options.extend(settings::target_catalog(game).into_iter().map(|id| {
                    if typed.contains(':') { return id; }
                    id.strip_prefix("minecraft:").unwrap_or(&id).to_owned()
                }));
            }
            let typed = typed.to_ascii_lowercase();
            options.retain(|item| item.starts_with(&typed));
            options.sort_unstable();
            options.dedup();
            *state = Completion {
                prefix: input[..prefix_end].into(),
                options,
                ..Completion::default()
            };
        }
        if state.options.is_empty() { return false; }
        let next = format!("{}{}", state.prefix, state.options[state.next]);
        state.next = (state.next+1) % state.options.len();
        state.last = next.clone();
        game.native_set_chat_input(&next, next.chars().count()).is_ok()
    })
}
