use crate::{root::Root, theme::Palette};
use gpui::{AnyElement, div, prelude::*, px};

// Same faceted fox as src/renderer/TerminalPlaceholder.tsx.
const FOX: &str = r#"
     ###                                                 ###
     #####                                             #####
     #::#####                                       #####::#
    ##::::#####                                   #####::::##
    ##::::::#####                               #####::::::##
    ##::::::::#####                           #####::::::::##
    ##::::::::::######                     ######::::::::::##
   ###::::::::::::######                 ######::::::::::::###
   ###::::::::::::::##+*******************+##::::::::::::::###
   ###::::::::::####++*********************++####::::::::::###
  ####:::::::#### +++***********************+++ ####:::::::####
  ####:::#####  ++++*************************++++  #####:::####
   #########  ++++++*************************++++++  #########
    #####   +++++++***************************+++++++   #####
    ##    ++++++++*****************************++++++++    ##
        +++++++++*******************************+++++++++
      +++++@@@@@@*******************************@@@@@@+++++
    +++++### @@@@@@@@@@*******************@@@@@@@@@@ ###+++++
  ###########     # @@@*******************@@@ #     ###########
    ###########   #   ====*************====   #   ###########
      ###########::: :========*****========: :::###########
        =#########::::::=================::::::#########=
          =#######::::::::=============::::::::#######=
            =#####::::::::=============::::::::#####=
              =###:::::::::===========:::::::::###=
                =##::::::::===========::::::::##=
                  =::::::::===========::::::::=
                    :::::::::::@@@:::::::::::
                      ::::::::::|::::::::::
                        ::::::::|::::::::
                          :::.._:_..:::
                            :::::::::
"#;

impl Root {
    pub(crate) fn empty_workspace(&self, palette: Palette, window: &gpui::Window) -> AnyElement {
        let names = window.text_system().all_font_names();
        let family = ["Consolas", "DejaVu Sans Mono", "Menlo", "Liberation Mono"]
            .into_iter()
            .find(|family| names.iter().any(|name| name == family))
            .unwrap_or(".SystemUIFont");
        let base_font = gpui::font(family);
        let color = palette.accent.opacity(if palette.dark { 0.6 } else { 1. });
        let label = ">. Shellfox";
        let run = gpui::TextRun {
            len: label.len(),
            font: base_font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let width = window
            .text_system()
            .shape_line(label.into(), px(13.), &[run], None)
            .width
            + px(label.chars().count() as f32 * 2.);
        let label = gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let mut origin = bounds.origin;
                for character in ">. Shellfox".chars() {
                    let text = character.to_string();
                    let run = gpui::TextRun {
                        len: text.len(),
                        font: base_font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let line = window
                        .text_system()
                        .shape_line(text.into(), px(13.), &[run], None);
                    if let Err(error) = line.paint(origin, px(20.), window, cx) {
                        eprintln!("Shellfox brand paint: {error}");
                        return;
                    }
                    origin.x += line.width + px(2.);
                }
            },
        )
        .w(width)
        .h(px(20.));
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .font_family(family)
                    .text_size(px(8.))
                    .line_height(px(9.))
                    .text_color(palette.accent.opacity(if palette.dark { 0.72 } else { 1. }))
                    .whitespace_nowrap()
                    .mb(px(26.))
                    // HTML pre does not allocate GPUI's extra final empty line.
                    .child(FOX.strip_suffix('\n').unwrap_or(FOX)),
            )
            .child(label)
            .into_any_element()
    }
}
