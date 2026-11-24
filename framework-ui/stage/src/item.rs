use crate::rules::PanelRules;
use gpui::{AnyView, App, AppContext, Context, Entity, FocusHandle, Global, Pixels, SharedString, Size, Subscription};
use nova_look::tokens::{BAR_GAP, PAD, RADIUS_PANEL};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct Pill {
    pub(crate) content: AnyView,
    pub(crate) panel: Option<Panel>,
    pub(crate) opens_on_click: bool,
    pub(crate) shown: Option<Shown>,
}

impl Pill {
    pub fn new(content: impl Into<AnyView>) -> Self {
        Self { content: content.into(), panel: None, opens_on_click: true, shown: None }
    }

    pub fn panel(mut self, panel: Panel) -> Self {
        self.panel = Some(panel);
        self
    }

    pub fn opens_on_click(mut self, opens: bool) -> Self {
        self.opens_on_click = opens;
        self
    }

    pub fn shown_while(mut self, shown: Shown) -> Self {
        self.shown = Some(shown);
        self
    }
}

#[derive(Debug, Clone)]
pub struct Shown(Rc<Flag>);

#[derive(Debug)]
enum Flag {
    Own(Cell<bool>),
    Any(RefCell<Vec<Shown>>),
}

struct Changes(Entity<()>);

impl Global for Changes {}

fn changes(cx: &mut App) -> Entity<()> {
    if let Some(Changes(changes)) = cx.try_global::<Changes>() {
        return changes.clone();
    }
    let changes = cx.new(|_| ());
    cx.set_global(Changes(changes.clone()));
    changes
}

fn tell(cx: &mut App) {
    changes(cx).update(cx, |_, cx| cx.notify());
}

impl Shown {
    pub fn new(shown: bool) -> Self {
        Self(Rc::new(Flag::Own(Cell::new(shown))))
    }

    pub fn any(flags: Vec<Shown>) -> Self {
        Self(Rc::new(Flag::Any(RefCell::new(flags))))
    }

    pub fn set(&self, shown: bool, cx: &mut App) -> bool {
        let changed = self.put(shown);
        if changed {
            tell(cx);
        }
        changed
    }

    pub fn follow(&self, flags: Vec<Shown>, cx: &mut App) {
        if self.refollow(flags) {
            tell(cx);
        }
    }

    fn put(&self, shown: bool) -> bool {
        match &*self.0 {
            Flag::Own(flag) => flag.replace(shown) != shown,
            Flag::Any(_) => false,
        }
    }

    fn refollow(&self, flags: Vec<Shown>) -> bool {
        let Flag::Any(followed) = &*self.0 else { return false };
        let before = self.get();
        followed.replace(flags);
        self.get() != before
    }

    pub fn get(&self) -> bool {
        match &*self.0 {
            Flag::Own(flag) => flag.get(),
            Flag::Any(flags) => flags.borrow().iter().any(Shown::get),
        }
    }

    pub fn observe<V: 'static>(
        cx: &mut Context<V>,
        mut changed: impl FnMut(&mut V, &mut Context<V>) + 'static,
    ) -> Subscription {
        let changes = changes(cx);
        cx.observe(&changes, move |view, _, cx| changed(view, cx))
    }
}

pub struct Part {
    pub(crate) view: AnyView,
    pub(crate) inset: Pixels,
    pub(crate) shown: Option<Shown>,
}

impl Part {
    pub fn new(view: impl Into<AnyView>) -> Self {
        Self { view: view.into(), inset: PAD, shown: None }
    }

    pub fn inset(mut self, inset: Pixels) -> Self {
        self.inset = inset;
        self
    }

    pub fn shown_while(mut self, shown: Shown) -> Self {
        self.shown = Some(shown);
        self
    }

    pub fn is_shown(&self) -> bool {
        self.shown.as_ref().is_none_or(Shown::get)
    }

    pub fn shown(&self) -> Shown {
        self.shown.clone().unwrap_or_else(|| Shown::new(true))
    }

    pub fn view(&self) -> &AnyView {
        &self.view
    }

    pub fn end_inset(&self) -> Pixels {
        self.inset
    }
}

impl<V: gpui::Render> From<gpui::Entity<V>> for Part {
    fn from(view: gpui::Entity<V>) -> Self {
        Self::new(view)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    Top,
    RightEdge,
    RightTab,
    WholeScreen,
}

impl Placement {
    pub(crate) fn on_strip(self) -> bool {
        self == Self::Top
    }
}

#[derive(Clone)]
pub struct Panel {
    pub(crate) name: SharedString,
    pub(crate) size: Size<Pixels>,
    pub(crate) content: AnyView,
    pub(crate) radius: Pixels,
    pub(crate) top: Pixels,
    pub(crate) placement: Placement,
    pub(crate) from: Option<SharedString>,
    pub(crate) focus: Option<FocusHandle>,
    pub(crate) exclusive: bool,
    pub(crate) wants_keyboard: bool,
    pub(crate) closes_on_click_outside: bool,
    pub(crate) in_place: bool,
}

impl Panel {
    pub fn new(name: impl Into<SharedString>, size: Size<Pixels>, content: impl Into<AnyView>) -> Self {
        Self {
            name: name.into(),
            size,
            content: content.into(),
            radius: RADIUS_PANEL,
            top: BAR_GAP,
            placement: Placement::Top,
            from: None,
            focus: None,
            exclusive: true,
            wants_keyboard: false,
            closes_on_click_outside: true,
            in_place: false,
        }
    }

    pub fn from(mut self, pill_view: impl Into<SharedString>) -> Self {
        self.from = Some(pill_view.into());
        self
    }

    pub fn top(mut self, top: Pixels) -> Self {
        self.top = top;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    pub fn takes_typing(mut self, focus: FocusHandle) -> Self {
        self.wants_keyboard = true;
        self.focus = Some(focus);
        self
    }

    pub fn exclusive(mut self, exclusive: bool) -> Self {
        self.exclusive = exclusive;
        self
    }

    pub fn closes_on_click_outside(mut self, closes: bool) -> Self {
        self.closes_on_click_outside = closes;
        self
    }

    pub fn in_place(mut self) -> Self {
        self.in_place = true;
        self
    }

    pub(crate) fn rules(&self) -> PanelRules {
        PanelRules {
            name: self.name.clone(),
            placement: self.placement,
            exclusive: self.exclusive,
            wants_keyboard: self.wants_keyboard,
            closes_on_click_outside: self.closes_on_click_outside,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pill_shown_with_its_parts_follows_them() {
        let (tray, buttons) = (Shown::new(false), Shown::new(false));
        let pill = Shown::any(vec![tray.clone(), buttons.clone()]);
        assert!(!pill.get());
        assert!(tray.put(true));
        assert!(pill.get());
        assert!(!pill.put(false), "it follows its flags and isn't set");
        assert!(pill.refollow(vec![buttons]), "it stops being shown");
        assert!(!pill.get());
        assert!(!pill.refollow(Vec::new()), "still not shown");
    }
}
