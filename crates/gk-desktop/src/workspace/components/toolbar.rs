//! Top toolbar: app title, remote commands (centered) + refresh.

use gk_core::remote::RemoteOp;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;

    h_flex()
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(border)
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("git-desktop"))
        .child(div().flex_1())
        // remote commands, centered between two equal spacers
        .child(
            h_flex()
                .gap_1()
                .child(remote_button(
                    "fetch",
                    "Fetch",
                    IconName::CloudDownload,
                    RemoteOp::Fetch,
                    ws,
                    cx,
                ))
                .child(remote_button("pull", "Pull", IconName::ArrowDown, RemoteOp::Pull, ws, cx))
                .child(remote_button("push", "Push", IconName::ArrowUp, RemoteOp::Push, ws, cx)),
        )
        .child(div().flex_1())
        .child(
            Button::new("refresh")
                .ghost()
                .small()
                .label("Refresh")
                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
        )
}

fn remote_button(
    id: &'static str,
    label: &'static str,
    icon: IconName,
    op: RemoteOp,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> Button {
    Button::new(id)
        .icon(icon)
        .label(label)
        .loading(ws.remote_busy == Some(id))
        .disabled(ws.remote_busy.is_some())
        .on_click(cx.listener(move |this, _, window, cx| {
            if op == RemoteOp::Push {
                // Push may need to set an upstream branch first.
                this.push(window, cx);
            } else {
                this.run_remote(op, cx);
            }
        }))
}
