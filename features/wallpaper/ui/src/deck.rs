//! The carousel's deck without any drawing: which image is picked, where
//! every card sits around it, what each screen would show, and the
//! `set_wallpaper` calls that keep the picks. Today's `carousel.js` and the
//! state `WallpaperCarousel.qml` keeps, tested alone.
//!
//! The deck loops: past the last image comes the first again. Its position
//! is a number that runs on without wrapping (each step adds or takes one),
//! so a step across the ends moves like any other; which image that is, and
//! where each card sits around it, wrap.

use nova_wallpaper_api::{WallpaperFolder, Wallpapers};
use std::collections::BTreeMap;

/// What each screen shows or would show, by output name ("" for none).
pub type ByScreen = BTreeMap<String, String>;

/// `index` as one of `count` images (0 to count - 1), however far it went.
pub fn wrap(index: i64, count: usize) -> usize {
    let count = count.max(1) as i64;
    index.rem_euclid(count) as usize
}

/// How many cards image `index` sits right (positive) or left (negative)
/// of the one at `position`, the short way round: never more than half the
/// images either way.
pub fn offset(index: usize, position: f32, count: usize) -> f32 {
    let count = count.max(1) as f32;
    // JavaScript's `%`: the sign of the dividend.
    let mut away = (index as f32 - position) % count;
    if away > count / 2.0 {
        away -= count;
    } else if away <= -count / 2.0 {
        away += count;
    }
    away
}

/// How much of a card `away` from the middle shows (0 to 1): `sides` cards
/// show either side, and each image once. One fades out before it would
/// pass halfway round, where it jumps to the other side, so none flies
/// across; in an even loop the one at rest exactly halfway shows on the
/// right (where [`offset`] puts it), so two images show both.
pub fn shown(away: f32, count: usize, sides: usize) -> f32 {
    let half = count as f32 / 2.0;
    let near = (sides as f32 + 0.5 - away.abs()) * 2.0;
    let resting = away.fract() == 0.0 && away == half;
    let before_halfway = if resting { 1.0 } else { (half - away.abs()) * 2.0 };
    near.min(before_halfway).clamp(0.0, 1.0)
}

/// A path's file name.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// What each of `names` shows now, from novad's `wallpapers` answer: its
/// own image, else every screen's, else "".
pub fn showing(names: &[String], shown: &Wallpapers) -> ByScreen {
    names
        .iter()
        .map(|name| {
            let path = shown.outputs.get(name).or(shown.path.as_ref()).cloned().unwrap_or_default();
            (name.clone(), path)
        })
        .collect()
}

/// The screen picked for after `choosing_for` (`None` for every screen),
/// `by` one forward or back through every screen and then `names`, round
/// past either end.
pub fn next_screen(choosing_for: Option<&str>, names: &[String], by: i64) -> Option<String> {
    let at = choosing_for.and_then(|name| names.iter().position(|each| each == name)).map_or(0, |at| at + 1);
    let next = wrap(at as i64 + by, names.len() + 1);
    next.checked_sub(1).map(|index| names[index].clone())
}

/// One `set_wallpaper` call: `path` on `output`, or on every screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub path: String,
    pub output: Option<String>,
}

/// The calls that keep `picks` over what `showing` shows: one for every
/// screen when they all get the same image, which also drops screens' own
/// images; otherwise one for each screen that changes. None when nothing
/// does.
pub fn requests(picks: &ByScreen, showing: &ByScreen) -> Vec<Request> {
    let changed: Vec<&String> = picks
        .iter()
        .filter(|(name, path)| !path.is_empty() && showing.get(*name) != Some(*path))
        .map(|(name, _)| name)
        .collect();
    if changed.is_empty() {
        return Vec::new();
    }
    let mut all = picks.values();
    if let Some(first) = all.next()
        && all.all(|path| path == first)
    {
        return vec![Request { path: first.clone(), output: None }];
    }
    changed.into_iter().map(|name| Request { path: picks[name].clone(), output: Some(name.clone()) }).collect()
}

/// The deck on one screen: the folder's images, what every screen shows
/// and would show, which screen it picks for, and where the picked card is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Deck {
    /// This screen's output name.
    pub here: String,
    /// Every screen's name, left to right; never empty (this one alone
    /// when no screen is known).
    pub screens: Vec<String>,
    pub images: Vec<String>,
    pub folder: String,
    pub showing: ByScreen,
    pub picks: ByScreen,
    /// The screen picked for, `None` for every one.
    pub choosing_for: Option<String>,
    /// Where the picked card is, counted without wrapping.
    pub target: i64,
}

impl Deck {
    /// An empty deck for `here`, among `screens` (left to right).
    pub fn new(here: &str, screens: Vec<String>) -> Self {
        let mut deck = Self { here: here.to_owned(), ..Self::default() };
        deck.set_screens(screens);
        deck
    }

    pub fn set_screens(&mut self, screens: Vec<String>) {
        self.screens = if screens.is_empty() { vec![self.here.clone()] } else { screens };
        if self.choosing_for.as_ref().is_some_and(|name| !self.screens.contains(name)) {
            self.choosing_for = None;
        }
    }

    pub fn count(&self) -> usize {
        self.images.len()
    }

    /// The screens moving the deck picks for.
    pub fn covered(&self) -> Vec<&str> {
        match &self.choosing_for {
            Some(name) => vec![name.as_str()],
            None => self.screens.iter().map(String::as_str).collect(),
        }
    }

    /// The picked image's index, while there are images.
    pub fn picked(&self) -> Option<usize> {
        (!self.images.is_empty()).then(|| wrap(self.target, self.count()))
    }

    pub fn picked_path(&self) -> &str {
        self.picked().map_or("", |index| &self.images[index])
    }

    /// What this screen shows now.
    pub fn current(&self) -> &str {
        self.showing.get(&self.here).map_or("", String::as_str)
    }

    /// Opened: the folder's listing and what is shown now. The deck starts
    /// on this screen's image when it is in the folder, else the first.
    pub fn load(&mut self, listing: WallpaperFolder, shown: &Wallpapers) {
        self.folder = listing.folder;
        self.showing = showing(&self.screens, shown);
        self.choosing_for = None;
        self.target = listing.images.iter().position(|image| image == self.current()).unwrap_or(0) as i64;
        self.images = listing.images;
        self.picks = self.showing.clone();
    }

    /// Closed and faded: its images go.
    pub fn clear(&mut self) {
        self.images.clear();
        self.showing.clear();
        self.picks.clear();
        self.choosing_for = None;
    }

    /// What Enter would keep: the picks, and the middle card on a screen it
    /// picks for that shows nothing from the folder yet.
    pub fn kept(&self) -> ByScreen {
        let mut kept = self.picks.clone();
        let picked = self.picked_path();
        if !picked.is_empty() {
            for name in self.covered() {
                let in_folder = kept.get(name).is_some_and(|path| self.images.contains(path));
                if !in_folder {
                    kept.insert(name.to_owned(), picked.to_owned());
                }
            }
        }
        kept
    }

    /// What this screen previews: its pick, or its own image until the
    /// folder is listed.
    pub fn preview_path(&self) -> String {
        self.kept().get(&self.here).filter(|path| !path.is_empty()).cloned().unwrap_or_else(|| self.current().into())
    }

    /// What the other screens preview, where it would change.
    pub fn previews(&self) -> ByScreen {
        let showing = &self.showing;
        self.kept()
            .into_iter()
            .filter(|(name, path)| *name != self.here && !path.is_empty() && showing.get(name) != Some(path))
            .collect()
    }

    /// Moves the deck `by` cards, round past either end. Returns whether
    /// it moved.
    pub fn step(&mut self, by: i64) -> bool {
        if by == 0 || self.images.is_empty() {
            return false;
        }
        self.target += by;
        let picked = self.picked_path().to_owned();
        for name in self.covered().into_iter().map(str::to_owned).collect::<Vec<_>>() {
            self.picks.insert(name, picked.clone());
        }
        true
    }

    /// Picks for screen `name` from now on, `None` for every one: the deck
    /// moves round to what it would show. Returns whether it changed.
    pub fn choose_for(&mut self, name: Option<String>) -> bool {
        if self.screens.len() < 2 || name.as_ref().is_some_and(|name| !self.screens.contains(name)) {
            return false;
        }
        self.choosing_for = name;
        let first = self.covered()[0].to_owned();
        let at = self.picks.get(&first).and_then(|pick| self.images.iter().position(|image| image == pick));
        if let Some(at) = at {
            self.target += offset(at, self.target as f32, self.count()).round() as i64;
        }
        true
    }

    /// `done` were set: the screens they name show them now.
    pub fn shown_now(&mut self, done: &[Request]) {
        for request in done {
            match &request.output {
                Some(name) => {
                    self.showing.insert(name.clone(), request.path.clone());
                }
                None => self.showing.values_mut().for_each(|path| path.clone_from(&request.path)),
            }
        }
    }

    /// The screens other than the ones the deck picks for that pick the
    /// image `path`: their names show on its card.
    pub fn marks(&self, path: &str) -> Vec<&str> {
        let covered = self.covered();
        self.screens
            .iter()
            .map(String::as_str)
            .filter(|name| !covered.contains(name) && self.picks.get(*name).is_some_and(|pick| pick == path))
            .collect()
    }

    /// Whether `path` is what the first screen the deck picks for shows
    /// now: its card says so.
    pub fn is_now(&self, path: &str) -> bool {
        self.covered().first().and_then(|name| self.showing.get(*name)).is_some_and(|shown| shown == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    fn by_screen(pairs: &[(&str, &str)]) -> ByScreen {
        pairs.iter().map(|(name, path)| ((*name).to_owned(), (*path).to_owned())).collect()
    }

    fn deck(screens: &[&str], images: &[&str], shown: Wallpapers) -> Deck {
        let mut deck = Deck::new(screens[0], names(screens));
        deck.load(WallpaperFolder { folder: "/w".into(), images: names(images) }, &shown);
        deck
    }

    fn everywhere(path: &str) -> Wallpapers {
        Wallpapers { path: Some(path.into()), outputs: BTreeMap::new() }
    }

    #[test]
    fn wrapping_goes_round_either_end() {
        assert_eq!(wrap(-1, 5), 4);
        assert_eq!(wrap(7, 5), 2);
        assert_eq!(wrap(0, 5), 0);
    }

    #[test]
    fn offsets_take_the_short_way_round() {
        assert_eq!(offset(4, 0.0, 5), -1.0);
        assert_eq!(offset(1, 0.0, 5), 1.0);
        assert_eq!(offset(0, 4.5, 5), 0.5);
        // In an even loop the one exactly halfway sits on the right.
        assert_eq!(offset(2, 0.0, 4), 2.0);
        assert_eq!(offset(0, 2.0, 4), 2.0);
    }

    #[test]
    fn three_cards_show_either_side_and_each_image_once() {
        assert_eq!(shown(0.0, 20, 3), 1.0);
        assert_eq!(shown(3.0, 20, 3), 1.0);
        assert_eq!(shown(3.25, 20, 3), 0.5);
        assert_eq!(shown(4.0, 20, 3), 0.0);
        // Of four, the one halfway round shows on one side only.
        assert_eq!(shown(2.0, 4, 3), 1.0);
        assert_eq!(shown(1.75, 4, 3), 0.5);
        assert_eq!(shown(-1.75, 4, 3), 0.5);
    }

    #[test]
    fn it_starts_on_the_screen_s_own_image() {
        let deck = deck(&["DP-1"], &["/w/a.png", "/w/b.png"], everywhere("/w/b.png"));
        assert_eq!(deck.picked_path(), "/w/b.png");
        let elsewhere = self::deck(&["DP-1"], &["/w/a.png", "/w/b.png"], everywhere("/other.png"));
        assert_eq!(elsewhere.picked_path(), "/w/a.png");
        // Its own image until it moves: the folder doesn't hold it.
        assert_eq!(elsewhere.preview_path(), "/w/a.png");
    }

    #[test]
    fn stepping_picks_for_every_screen_and_loops() {
        let mut deck = deck(&["DP-1", "DP-2"], &["/w/a", "/w/b", "/w/c"], everywhere("/w/a"));
        assert!(deck.step(-1));
        assert_eq!(deck.picked_path(), "/w/c");
        assert_eq!(deck.picks, by_screen(&[("DP-1", "/w/c"), ("DP-2", "/w/c")]));
        assert_eq!(deck.previews(), by_screen(&[("DP-2", "/w/c")]));
        assert_eq!(requests(&deck.kept(), &deck.showing), vec![Request { path: "/w/c".into(), output: None }]);
    }

    #[test]
    fn picking_for_one_screen_keeps_the_other_s_image() {
        let mut deck = deck(&["DP-1", "DP-2"], &["/w/a", "/w/b", "/w/c"], everywhere("/w/a"));
        assert_eq!(next_screen(None, &deck.screens, 1).as_deref(), Some("DP-1"));
        assert!(deck.choose_for(Some("DP-2".into())));
        deck.step(1);
        assert_eq!(deck.picks, by_screen(&[("DP-1", "/w/a"), ("DP-2", "/w/b")]));
        assert_eq!(deck.marks("/w/a"), vec!["DP-1"]);
        assert!(deck.is_now("/w/a"));
        assert_eq!(deck.previews(), by_screen(&[("DP-2", "/w/b")]));
        assert_eq!(deck.preview_path(), "/w/a");
        assert_eq!(
            requests(&deck.kept(), &deck.showing),
            vec![Request { path: "/w/b".into(), output: Some("DP-2".into()) }]
        );
        // Back to every screen: the deck goes round to DP-1's pick.
        assert!(deck.choose_for(None));
        assert_eq!(deck.picked_path(), "/w/a");
    }

    #[test]
    fn one_screen_has_no_screen_to_pick_for() {
        let mut deck = deck(&["DP-1"], &["/w/a"], everywhere("/w/a"));
        assert!(!deck.choose_for(Some("DP-1".into())));
        assert_eq!(next_screen(Some("DP-2"), &names(&["DP-1", "DP-2"]), 1), None);
        assert_eq!(next_screen(None, &names(&["DP-1", "DP-2"]), -1).as_deref(), Some("DP-2"));
    }

    #[test]
    fn what_was_set_before_a_refusal_shows_now() {
        let mut deck = deck(&["DP-1", "DP-2"], &["/w/a", "/w/b"], everywhere("/w/a"));
        deck.choose_for(Some("DP-1".into()));
        deck.step(1);
        deck.shown_now(&[Request { path: "/w/b".into(), output: Some("DP-1".into()) }]);
        assert_eq!(deck.showing, by_screen(&[("DP-1", "/w/b"), ("DP-2", "/w/a")]));
        assert!(requests(&deck.kept(), &deck.showing).is_empty());
        deck.shown_now(&[Request { path: "/w/a".into(), output: None }]);
        assert_eq!(deck.showing, by_screen(&[("DP-1", "/w/a"), ("DP-2", "/w/a")]));
    }

    #[test]
    fn nothing_changed_asks_for_nothing() {
        let deck = deck(&["DP-1"], &["/w/a"], everywhere("/w/a"));
        assert!(requests(&deck.kept(), &deck.showing).is_empty());
    }

    #[test]
    fn a_screen_showing_nothing_from_the_folder_gets_the_middle_card() {
        let shown = Wallpapers { path: Some("/w/a".into()), outputs: by_screen(&[("DP-2", "/elsewhere.png")]) };
        let deck = deck(&["DP-1", "DP-2"], &["/w/a", "/w/b"], shown);
        assert_eq!(deck.showing, by_screen(&[("DP-1", "/w/a"), ("DP-2", "/elsewhere.png")]));
        assert_eq!(deck.kept(), by_screen(&[("DP-1", "/w/a"), ("DP-2", "/w/a")]));
        assert_eq!(requests(&deck.kept(), &deck.showing), vec![Request { path: "/w/a".into(), output: None }]);
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("/w/sea.jpg"), "sea.jpg");
        assert_eq!(file_name("sea.jpg"), "sea.jpg");
    }
}
