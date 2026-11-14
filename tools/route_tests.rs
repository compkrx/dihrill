#[path = "../mixins/dihrill/route.rs"]
mod route;

#[test]
fn avoids_a_junk_pile_when_a_detour_is_available() {
    use route::{Cell, Search, SearchOptions, SearchResult};
    use std::collections::HashSet;

    let start = [0, 0, 0];
    let goal = [4, 0, 0];
    let pile = [2, 0, 0];
    for detour in [true, false] {
        let options = SearchOptions {
            goal: Some(goal),
            tunnel: false,
            junk_pickup_risk: HashSet::from([pile]),
            ..SearchOptions::default()
        };
        let mut search = Search::with_options(start, 8, options);
        let path = loop {
            match search.advance(30, |[_, y, z]| {
                let clear = (0..=1).contains(&y) && (z == 0 || detour && z == 1);
                Cell { known: true, floor: !clear, clear, ..Cell::default() }
            }) {
                SearchResult::Pending => (),
                SearchResult::Found(path) => break path,
                SearchResult::Exhausted => panic!("test corridor connects to the goal"),
            }
        };
        assert_eq!(path.steps.back().unwrap().to, goal);
        assert_eq!(path.steps.iter().any(|step| step.to == pile), !detour);
    }
}
