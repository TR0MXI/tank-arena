//! 两个文件库共用卡片网格、空列表提示和分页控件。

use super::*;

const PER_PAGE: usize = 6;

pub(super) fn last_page(count: usize) -> usize {
    count.saturating_sub(1) / PER_PAGE
}

pub(super) fn page_items<T>(items: &[T], page: usize) -> &[T] {
    items.chunks(PER_PAGE).nth(page).unwrap_or_default()
}

pub(super) struct Layout<'a> {
    pub title: &'static str,
    pub empty: &'static str,
    pub notice: Option<&'a str>,
    pub count: usize,
    pub page: usize,
}

impl Layout<'_> {
    pub fn show<B: Bundle>(
        &self,
        p: &mut ChildSpawnerCommands,
        cards: impl FnOnce(&mut ChildSpawnerCommands),
        go: impl Fn(usize) -> B,
    ) {
        p.spawn(page_body()).with_children(|p| {
            p.spawn(page_header()).with_children(|p| {
                p.spawn((PageIntro, headline(&tr(self.title), 30.)));
            });
            if let Some(note) = self.notice {
                p.spawn(label(note, 15., quiet()));
            }
            if self.count == 0 {
                p.spawn((
                    Node {
                        width: percent(100),
                        height: px(342),
                        border_radius: BorderRadius::all(px(18)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(229, 235, 222)),
                    children![headline(&tr(self.empty), 22.)],
                ));
                return;
            }

            p.spawn(Node {
                width: percent(100),
                column_gap: px(16),
                row_gap: px(14),
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(cards);
            pager(p, self.page, self.count.div_ceil(PER_PAGE), go);
        });
    }
}

pub(super) fn file_card() -> impl Bundle {
    (
        Node {
            width: px(327),
            padding: UiRect::all(px(12)),
            border_radius: BorderRadius::all(px(16)),
            ..column(7.)
        },
        BackgroundColor(Color::srgb_u8(251, 252, 246)),
    )
}

pub(super) fn thumbnail() -> impl Bundle {
    (
        Node {
            width: percent(100),
            height: px(126),
            border_radius: BorderRadius::all(px(9)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(ink()),
    )
}

pub(super) fn title(value: &str) -> impl Bundle {
    (
        Node {
            width: percent(100),
            overflow: Overflow::clip(),
            ..default()
        },
        children![(headline(value, 16.), TextLayout::no_wrap())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_full_and_partial_pages_have_consistent_bounds() {
        let items = [0; PER_PAGE + 1];
        for (count, last, remaining) in [(0, 0, 0), (PER_PAGE, 0, PER_PAGE), (PER_PAGE + 1, 1, 1)] {
            let items = &items[..count];
            assert_eq!(last_page(count), last);
            assert_eq!(page_items(items, last).len(), remaining);
            assert!(page_items(items, last + 1).is_empty());
        }
    }
}
