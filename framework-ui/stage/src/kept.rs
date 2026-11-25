use gpui::SharedString;

pub struct Kept<T> {
    items: Vec<(SharedString, Option<T>)>,
}

impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> Kept<T> {
    pub fn sync(&mut self, wanted: &[SharedString], mut build: impl FnMut(&SharedString) -> Option<T>) -> bool {
        let before: Vec<SharedString> = self.items.iter().map(|(view, _)| view.clone()).collect();
        let mut old = std::mem::take(&mut self.items);
        for view in wanted {
            let kept = old.iter().position(|(id, _)| id == view).map(|at| old.remove(at));
            self.items.push(kept.unwrap_or_else(|| (view.clone(), build(view))));
        }
        before != wanted
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter().filter_map(|(_, item)| item.as_ref())
    }

    pub fn entries(&self) -> impl Iterator<Item = (&SharedString, Option<&T>)> {
        self.items.iter().map(|(view, item)| (view, item.as_ref()))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut().filter_map(|(_, item)| item.as_mut())
    }

    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(views: &[&'static str]) -> Vec<SharedString> {
        views.iter().map(|view| SharedString::from(*view)).collect()
    }

    #[test]
    fn a_view_whose_slot_stays_is_not_built_again() {
        let mut built = 0;
        let mut kept = Kept::default();
        let mut build = |view: &SharedString| {
            built += 1;
            Some(format!("{view} #{built}"))
        };
        assert!(kept.sync(&ids(&["a", "b"]), &mut build));
        assert!(kept.sync(&ids(&["c", "b"]), &mut build));
        assert_eq!(kept.iter().collect::<Vec<_>>(), ["c #3", "b #2"]);
        assert!(!kept.sync(&ids(&["c", "b"]), &mut build));
    }

    #[test]
    fn a_view_nothing_registered_is_asked_for_once() {
        let mut asked = 0;
        let mut kept: Kept<()> = Kept::default();
        for _ in 0..3 {
            kept.sync(&ids(&["missing"]), |_| {
                asked += 1;
                None
            });
        }
        assert_eq!(asked, 1);
        assert!(kept.is_empty());
    }
}
