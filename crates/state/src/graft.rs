use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Which list a local file is grafted into: a local album into an artist's
/// releases, or a local song into an album's tracklist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stock {
    Discography,
    Tracklist,
}

/// One local item placed into a streamed list, at its index in the spliced list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graft {
    pub id: String,
    pub at: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Grafts {
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    discographies: HashMap<String, Vec<Graft>>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    tracklists: HashMap<String, Vec<Graft>>,
}

impl Grafts {
    pub(crate) fn is_empty(&self) -> bool {
        self.discographies.is_empty() && self.tracklists.is_empty()
    }

    fn stock(&self, stock: Stock) -> &HashMap<String, Vec<Graft>> {
        match stock {
            Stock::Discography => &self.discographies,
            Stock::Tracklist => &self.tracklists,
        }
    }

    fn stock_mut(&mut self, stock: Stock) -> &mut HashMap<String, Vec<Graft>> {
        match stock {
            Stock::Discography => &mut self.discographies,
            Stock::Tracklist => &mut self.tracklists,
        }
    }

    pub(crate) fn of(&self, stock: Stock, host: &str) -> &[Graft] {
        self.stock(stock).get(host).map_or(&[], Vec::as_slice)
    }

    /// Places `id` at `at`, moving it there if the host already holds it.
    pub(crate) fn place(&mut self, stock: Stock, host: &str, id: &str, at: usize) -> bool {
        let held = self.stock_mut(stock).entry(host.to_owned()).or_default();
        let placed = Graft {
            id: id.to_owned(),
            at,
        };
        if held.contains(&placed) {
            return false;
        }
        held.retain(|graft| graft.id != id);
        held.push(placed);
        true
    }

    pub(crate) fn remove(&mut self, stock: Stock, host: &str, id: &str) -> bool {
        let grafts = self.stock_mut(stock);
        let Some(held) = grafts.get_mut(host) else {
            return false;
        };
        let before = held.len();
        held.retain(|graft| graft.id != id);
        let removed = held.len() != before;
        if held.is_empty() {
            grafts.remove(host);
        }
        removed
    }
}

/// Lays grafted items into a native list, each at its own index, in index order
/// so an earlier one never pushes a later one off its place. An index past the
/// end lands at the end.
pub fn splice<T: Clone>(native: &[T], grafted: impl IntoIterator<Item = (usize, T)>) -> Vec<T> {
    let mut grafted: Vec<(usize, T)> = grafted.into_iter().collect();
    grafted.sort_by_key(|(at, _)| *at);

    let mut spliced = native.to_vec();
    for (at, item) in grafted {
        spliced.insert(at.min(spliced.len()), item);
    }
    spliced
}
