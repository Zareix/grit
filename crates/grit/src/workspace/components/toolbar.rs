//! Top toolbar: app title, remote commands (centered) + refresh.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use gpui_kit::*;
use grit_core::remote::RemoteOp;

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
        .child(
            div().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(
                ws.repo.as_ref().and_then(|repo| repo.name()).unwrap_or_else(|| "Grit".to_string()),
            ),
        )
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
        // app launchers, next to the remote group: GitHub + IDE + terminal
        .child(
            h_flex()
                .gap_1()
                .border_l_1()
                .border_color(border.opacity(0.6))
                .pl_2()
                .child(
                    Button::new("open-github")
                        .ghost()
                        .small()
                        .icon(IconName::Github)
                        .label("GitHub")
                        .on_click(
                            cx.listener(|_, _, _, cx| crate::settings::open_settings_window(cx)),
                        ),
                )
                .child(
                    Button::new("open-ide")
                        .ghost()
                        .small()
                        .icon(IconName::Code)
                        .label("IDE")
                        .loading(ws.remote_busy == Some("ide"))
                        .disabled(ws.remote_busy.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.open_in_ide(cx))),
                )
                .child(
                    Button::new("open-terminal")
                        .ghost()
                        .small()
                        .icon(IconName::SquareTerminal)
                        .label("Terminal")
                        .loading(ws.remote_busy == Some("terminal"))
                        .disabled(ws.remote_busy.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.open_in_terminal(cx))),
                )
                .child(
                    Button::new("open-agent")
                        .ghost()
                        .small()
                        .icon(IconName::Bot)
                        .label("Agent")
                        .loading(ws.remote_busy == Some("agent"))
                        .disabled(ws.remote_busy.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.open_agent(cx))),
                ),
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
