use gpui::{
    AnyWindowHandle, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled,
    TestAppContext, Window, WindowHandle, div, px, size,
};
use gpui_kit::component::input::{Textarea, TextareaState};

struct CaretHarness(Entity<TextareaState>);

impl Render for CaretHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(Textarea::new(&self.0).h(px(64.)))
    }
}

fn input(
    cx: &mut TestAppContext,
    block: bool,
    text: &str,
) -> (Entity<TextareaState>, WindowHandle<CaretHarness>) {
    let mut input = None;
    let window = cx.open_window(size(px(240.), px(100.)), |window, cx| {
        let state = cx.new(|cx| {
            let state = TextareaState::new(window, cx).default_value(text);
            if block {
                state.caret_style(px(4.), px(2.))
            } else {
                state
            }
        });
        input = Some(state.clone());
        CaretHarness(state)
    });
    (input.unwrap(), window)
}

#[gpui::test]
fn native_caret_is_per_input_and_keeps_the_insertion_offset(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (bar, bar_window) = input(cx, false, "AT+VERSION");
    let (block, block_window) = input(cx, true, "AT+VERSION");
    let mut bounds = Vec::new();
    for (state, window) in [(&bar, bar_window), (&block, block_window)] {
        AnyWindowHandle::from(window)
            .update(cx, |_, window, cx| {
                state.update(cx, |state, cx| {
                    state.set_selected_range(2..2, cx);
                    state.focus(window, cx);
                });
                window.draw(cx).clear(cx);
                let state = state.read(cx);
                assert_eq!(state.cursor(), 2);
                assert_eq!(state.value(), "AT+VERSION");
                bounds.push(state.prepaint_caret_geometry().unwrap().cursor_bounds());
            })
            .unwrap();
    }
    assert_eq!(bounds[0].size.width, px(1.));
    assert_eq!(bounds[1].size.width, px(4.));
    assert_eq!(bounds[1].left() - bounds[0].left(), px(2.));
}

#[gpui::test]
fn native_block_caret_follows_unicode_wrapping_and_scroll(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let text = format!("{}\n末尾", "串口测试abc ".repeat(30));
    let (state, window) = input(cx, true, &text);
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.draw(cx).clear(cx);
            state.update(cx, |state, cx| {
                state.set_selected_range(text.len()..text.len(), cx);
                state.focus(window, cx);
            });
            window.draw(cx).clear(cx);
            let input = state.read(cx);
            assert_eq!(input.value(), text);
            let geometry = input.prepaint_caret_geometry().unwrap();
            let mut caret = geometry.cursor_bounds();
            caret.origin.y += geometry.scroll_offset().y;
            let viewport = geometry.input_bounds();
            assert_eq!(caret.size.width, px(4.));
            assert!(caret.left() >= viewport.left());
            assert!(caret.right() <= viewport.right());
            assert!(caret.top() >= viewport.top());
            assert!(caret.bottom() <= viewport.bottom());
        })
        .unwrap();
}
