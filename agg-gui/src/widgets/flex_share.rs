//! Main-axis space sharing for the flex layouts (`FlexColumn` in `flex.rs`,
//! `FlexRow` in `flex_row.rs`).
//!
//! Flex children split the space the fixed children leave in proportion to
//! their factors, each clamped to its own min/max size.  A child that hits
//! its cap (or floor) is frozen there and the space it can't use is shared
//! again among the rest — CSS flexbox's "resolve flexible lengths" loop — so
//! a `Spacer` capped at zero really takes nothing.

/// One flex child's inputs to [`share_flex_space`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlexItem {
    /// Flex factor; `0.0` (or a hidden child) takes no share.
    pub flex: f64,
    /// Main-axis min / max size.
    pub min: f64,
    pub max: f64,
}

/// Share `space` among `items` (main-axis sizes, written into `sizes` for
/// every item with `flex > 0`; other entries are left untouched).  Returns
/// the total given to the flex items, which is less than `space` when every
/// one of them is capped by its max size.
pub(crate) fn share_flex_space(items: &[FlexItem], sizes: &mut [f64], space: f64) -> f64 {
    let mut frozen: Vec<bool> = items.iter().map(|it| it.flex <= 0.0).collect();
    loop {
        let used: f64 = (0..items.len())
            .filter(|&i| frozen[i] && items[i].flex > 0.0)
            .map(|i| sizes[i])
            .sum();
        let open: Vec<usize> = (0..items.len()).filter(|&i| !frozen[i]).collect();
        let total_flex: f64 = open.iter().map(|&i| items[i].flex).sum();
        if open.is_empty() || total_flex <= 0.0 {
            break;
        }
        let unit = (space - used).max(0.0) / total_flex;
        // Clamp every open item, then freeze the violators of the dominant
        // kind: when the clamps grew the total, the floors (min) win; when
        // they shrank it, the caps (max) do.
        let mut violation = 0.0;
        for &i in &open {
            let raw = items[i].flex * unit;
            sizes[i] = raw.min(items[i].max).max(items[i].min);
            violation += sizes[i] - raw;
        }
        let mut froze_any = false;
        for &i in &open {
            let raw = items[i].flex * unit;
            let hit = if violation > 0.0 {
                sizes[i] > raw
            } else if violation < 0.0 {
                sizes[i] < raw
            } else {
                false
            };
            if hit {
                frozen[i] = true;
                froze_any = true;
            }
        }
        if !froze_any {
            break;
        }
    }
    (0..items.len())
        .filter(|&i| items[i].flex > 0.0)
        .map(|i| sizes[i])
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(flex: f64, max: f64) -> FlexItem {
        FlexItem {
            flex,
            min: 0.0,
            max,
        }
    }

    #[test]
    fn equal_factors_split_evenly() {
        let items = [item(1.0, f64::MAX), item(1.0, f64::MAX)];
        let mut sizes = [0.0; 2];
        assert_eq!(share_flex_space(&items, &mut sizes, 100.0), 100.0);
        assert_eq!(sizes, [50.0, 50.0]);
    }

    #[test]
    fn capped_items_pass_their_share_on() {
        let items = [
            item(1.0, 0.0),
            item(0.0, f64::MAX),
            item(1.0, 10.0),
            item(2.0, f64::MAX),
        ];
        let mut sizes = [0.0, 7.0, 0.0, 0.0];
        assert_eq!(share_flex_space(&items, &mut sizes, 100.0), 100.0);
        assert_eq!(sizes, [0.0, 7.0, 10.0, 90.0], "fixed entries untouched");
    }

    #[test]
    fn all_capped_leaves_space_over() {
        let items = [item(1.0, 0.0), item(1.0, 20.0)];
        let mut sizes = [0.0; 2];
        assert_eq!(share_flex_space(&items, &mut sizes, 100.0), 20.0);
    }

    #[test]
    fn floors_hold_and_the_rest_shrinks() {
        let items = [
            FlexItem {
                flex: 1.0,
                min: 80.0,
                max: f64::MAX,
            },
            item(1.0, f64::MAX),
        ];
        let mut sizes = [0.0; 2];
        share_flex_space(&items, &mut sizes, 100.0);
        assert_eq!(sizes, [80.0, 20.0]);
    }
}
