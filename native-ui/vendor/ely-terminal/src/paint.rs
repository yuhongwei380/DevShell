use std::{ops::Range, rc::Rc};

use alacritty_terminal::vte::ansi::CursorShape;
use gpui::{
    BorderStyle, Bounds, ElementInputHandler, Entity, Font, FontStyle, FontWeight, Hsla,
    IntoElement, Pixels, Size, StrikethroughStyle, Styled, TextAlign, TextRun, UnderlineStyle,
    canvas, fill, outline, point, size,
};

use super::{
    frame::{Frame, Lit, Style},
    links::Target,
    view::Terminal,
};
use crate::primitives::caret_bar;

/// How a terminal paints: font and cell, ink, and the colors of what it marks.
pub(crate) struct Look {
    pub font: Font,
    pub size: Pixels,
    pub cell: Size<Pixels>,
    pub focused: bool,
    pub caret: Hsla,
    pub caret_accent: Option<Hsla>,
    pub caret_width: Pixels,
    pub rule: Pixels,
    pub selection: Hsla,
    pub selection_foreground: Option<Hsla>,
    pub found: Hsla,
    pub current: Hsla,
    pub link: Hsla,
}

/// The face a style prints in.
fn font(style: &Style, base: &Font) -> Font {
    Font {
        weight: if style.bold {
            FontWeight::BOLD
        } else {
            base.weight
        },
        style: if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..base.clone()
    }
}

/// The grid, fitted to its box: it tells the terminal its size and origin, then paints each row.
pub(crate) fn grid(
    terminal: Entity<Terminal>,
    frame: Rc<Frame>,
    hovered: Option<(usize, Range<usize>, Target)>,
    look: Look,
) -> impl IntoElement {
    let (fit, typing) = (terminal.clone(), terminal);
    let cell = look.cell;
    canvas(
        move |bounds, window, cx| {
            let columns = (bounds.size.width / cell.width).floor() as usize;
            let lines = (bounds.size.height / cell.height).floor() as usize;
            fit.update(cx, |terminal, cx| {
                terminal.origin = bounds.origin;
                let before = terminal.size;
                terminal.resize(columns, lines);
                if terminal.size != before {
                    cx.notify();
                    window.request_animation_frame();
                }
            });
        },
        move |bounds, _, window, cx| {
            let focus = typing.read(cx).focus.clone();
            window.handle_input(&focus, ElementInputHandler::new(bounds, typing.clone()), cx);
            let at = |row: usize, column: usize| {
                bounds.origin + point(cell.width * column as f32, cell.height * row as f32)
            };
            for (ix, row) in frame.rows.iter().enumerate() {
                let mut runs: Vec<TextRun> = row
                    .runs
                    .iter()
                    .map(|(len, style)| TextRun {
                        len: *len,
                        font: font(style, &look.font),
                        color: style.fg,
                        background_color: style.bg,
                        underline: style.underline.then_some(UnderlineStyle {
                            thickness: look.rule,
                            color: Some(style.fg),
                            wavy: false,
                        }),
                        strikethrough: style.strike.then_some(StrikethroughStyle {
                            thickness: look.rule,
                            color: Some(style.fg),
                        }),
                    })
                    .collect();
                if let Some(color) = look.selection_foreground {
                    let selected = frame.lit.iter().filter(|(row,..)|*row==ix).filter_map(|(_,columns,why)|(*why==Lit::Selected).then_some(columns)).collect::<Vec<_>>();
                    if !selected.is_empty() {
                        let mut expanded=Vec::new();let mut run_index=0;let mut run_end=runs.first().map(|run|run.len).unwrap_or(0);
                        for (column,(offset,ch)) in row.text.char_indices().enumerate() {
                            while offset>=run_end && run_index+1<runs.len() {run_index+=1;run_end+=runs[run_index].len;}
                            let mut run=runs[run_index].clone();run.len=ch.len_utf8();
                            if selected.iter().any(|range|range.contains(&column)){run.color=color;}
                            expanded.push(run);
                        }
                        runs=expanded;
                    }
                }
                let shaped = window.text_system().shape_line(
                    row.text.clone().into(),
                    look.size,
                    &runs,
                    Some(cell.width),
                );
                let ground = shaped.paint_background(
                    at(ix, 0),
                    cell.height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                if let Err(error) = ground {
                    log::error!("terminal: row {ix}'s ground failed to paint: {error:#}");
                }
                for (_, columns, why) in frame.lit.iter().filter(|(row, ..)| *row == ix) {
                    let wash = match why {
                        Lit::Selected => look.selection,
                        Lit::Found => look.found,
                        Lit::Current => look.current,
                    };
                    let extent = size(cell.width * columns.len() as f32, cell.height);
                    window.paint_quad(fill(Bounds::new(at(ix, columns.start), extent), wash));
                }
                if let Err(error) =
                    shaped.paint(at(ix, 0), cell.height, TextAlign::Left, None, window, cx)
                {
                    log::error!("terminal: row {ix} failed to paint: {error:#}");
                }
                for (column, cluster, style) in &row.clusters {
                    let run = TextRun {
                        len: cluster.len(),
                        font: font(style, &look.font),
                        color: look.selection_foreground.filter(|_|frame.lit.iter().any(|(row,columns,why)|*row==ix&&*why==Lit::Selected&&columns.contains(column))).unwrap_or(style.fg),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let painted = window
                        .text_system()
                        .shape_line(cluster.clone().into(), look.size, &[run], None)
                        .paint(
                            at(ix, *column),
                            cell.height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    if let Err(error) = painted {
                        log::error!("terminal: a cluster in row {ix} failed to paint: {error:#}");
                    }
                }
            }
            if let Some((row, columns, _)) = &hovered {
                let under = at(*row, columns.start) + point(Pixels::ZERO, cell.height - look.rule);
                let extent = size(cell.width * columns.len() as f32, look.rule);
                window.paint_quad(fill(Bounds::new(under, extent), look.link));
            }
            if let Some((row, column, shape)) = frame.cursor {
                let origin = at(row, column);
                let quad = match (shape, look.focused) {
                    (CursorShape::HollowBlock, _) | (_, false) => {
                        outline(Bounds::new(origin, cell), look.caret, BorderStyle::Solid)
                    }
                    (CursorShape::Beam, true) => {
                        let scale = window.scale_factor();
                        let bar = caret_bar(origin, look.caret_width, cell.height, scale);
                        fill(bar, look.caret)
                    }
                    (CursorShape::Underline, true) => fill(
                        Bounds::new(
                            origin + point(Pixels::ZERO, cell.height - look.caret_width),
                            size(cell.width, look.caret_width),
                        ),
                        look.caret,
                    ),
                    (CursorShape::Block | CursorShape::Hidden, true) => fill(Bounds::new(origin, cell), look.caret),
                };
                window.paint_quad(quad);
                if shape==CursorShape::Block && look.focused {
                    if let Some(line)=frame.rows.get(row) {
                        if let Some(character)=line.cells().get(column) {
                            let offset=line.text.chars().take(column).map(char::len_utf8).sum::<usize>();let mut end=0;
                            let style=line.runs.iter().find_map(|(bytes,style)|{end+=bytes;(end>offset).then_some(style)});
                            if let Some(style)=style {
                                let run=TextRun{len:character.len(),font:font(style,&look.font),color:look.caret_accent.unwrap_or_else(gpui::black),..Default::default()};
                                let _=window.text_system().shape_line(character.clone().into(),look.size,&[run],None).paint(origin,cell.height,TextAlign::Left,None,window,cx);
                            }
                        }
                    }
                }
            }
        },
    )
    .size_full()
}
