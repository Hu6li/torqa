//! A spatial index held compactly: what lies in each square cell, all in one array sorted by
//! cell. A list per cell grown one item at a time keeps up to half its room spare, which with
//! the copies of the streets' points it held came to hundreds of megabytes over a long route's
//! streets (#542).

use std::collections::HashMap;

/// Items by cell, each cell's in the order they were handed in.
pub(crate) struct CellIndex<T> {
    /// Each cell's items: where they start and end in `items`.
    ranges: HashMap<(i64, i64), (u32, u32)>,
    items: Vec<T>,
}

impl<T: Copy + Default> CellIndex<T> {
    /// The index of the items `fill` hands to its argument with their cells. It is called
    /// twice, to count each cell's items and then to place them, so building the index takes
    /// no more room than it keeps.
    pub(crate) fn build(fill: impl Fn(&mut dyn FnMut((i64, i64), T))) -> Self {
        let mut ranges: HashMap<(i64, i64), (u32, u32)> = HashMap::new();
        fill(&mut |cell, _| ranges.entry(cell).or_default().1 += 1);
        let mut next = 0_u32;
        for range in ranges.values_mut() {
            let count = range.1;
            *range = (next, next);
            next = next.checked_add(count).expect("fewer than 2^32 items");
        }
        let mut items = vec![T::default(); next as usize];
        fill(&mut |cell, item| {
            let range = ranges.get_mut(&cell).expect("counted cell");
            items[range.1 as usize] = item;
            range.1 += 1;
        });
        Self { ranges, items }
    }

    /// The items in `cell`.
    pub(crate) fn get(&self, cell: (i64, i64)) -> &[T] {
        self.ranges.get(&cell).map_or(&[], |&(start, end)| {
            &self.items[start as usize..end as usize]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::CellIndex;

    #[test]
    fn keeps_each_cells_items_in_the_order_handed_in() {
        let items = [
            ((0, 0), 3),
            ((1, -2), 7),
            ((0, 0), 1),
            ((1, -2), 5),
            ((0, 0), 2),
        ];
        let index = CellIndex::build(|push| {
            for &(cell, item) in &items {
                push(cell, item);
            }
        });
        assert_eq!(index.get((0, 0)), [3, 1, 2]);
        assert_eq!(index.get((1, -2)), [7, 5]);
        assert_eq!(index.get((5, 5)), [0; 0]);
    }

    #[test]
    fn an_index_of_nothing_finds_nothing() {
        let index = CellIndex::<u32>::build(|_| {});
        assert_eq!(index.get((0, 0)), [0; 0]);
    }
}
