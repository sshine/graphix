//! An interactive terminal editor for a planet's parameters.
//!
//! `pixel-planets --editor` shows the live, rotating planet on the left and a
//! column of parameter knobs on the right. Navigate the knobs with up/down and
//! turn each one with left/right; the planet re-bakes and updates immediately.
//!
//! Built on [`promptui_core`] (the [`Input`] key abstraction, the [`Theme`]
//! palette, and the [`BorderBox`] panel chrome) over ratatui and crossterm.
//! promptui has no gauge widget, so the 0..1 bars are drawn from block glyphs.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use promptui_core::input::Input;
use promptui_core::theme::Theme;
use promptui_core::widgets::border_box::{BorderBox, BorderStyle};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use crate::clouds::CloudMode;
use crate::planet::PlanetParams;
use crate::render::{Renderer, ZoomRamp};
use crate::{Error, random_seed};

/// Rotation period used inside the editor, in seconds.
const PERIOD: f32 = 12.0;
/// Width of a knob bar, in cells.
const BAR_WIDTH: usize = 20;
/// Eighth-block glyphs for sub-cell bar precision (1/8 .. 7/8 full).
const EIGHTHS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];

/// One adjustable parameter, wired to a getter/setter on [`PlanetParams`].
struct Knob {
    label: &'static str,
    min: f32,
    max: f32,
    step: f32,
    get: fn(&PlanetParams) -> f32,
    set: fn(&mut PlanetParams, f32),
    fmt: fn(f32) -> String,
}

impl Knob {
    /// The current value.
    fn value(&self, p: &PlanetParams) -> f32 {
        (self.get)(p)
    }

    /// The value as a `0..=1` fraction of the knob's range (for the bar).
    fn fraction(&self, p: &PlanetParams) -> f32 {
        ((self.value(p) - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// Turn the knob by `steps` increments, clamped to its range.
    fn turn(&self, p: &mut PlanetParams, steps: f32) {
        let v = (self.value(p) + steps * self.step).clamp(self.min, self.max);
        (self.set)(p, v);
    }
}

fn fmt_unit(v: f32) -> String {
    format!("{v:.2}")
}
fn fmt_signed(v: f32) -> String {
    format!("{v:+.2}")
}
fn fmt_deg(v: f32) -> String {
    format!("{v:+.0}\u{00B0}")
}

/// The full set of knobs, in display order.
fn knobs() -> Vec<Knob> {
    let unit = |label, get: fn(&PlanetParams) -> f32, set: fn(&mut PlanetParams, f32)| Knob {
        label,
        min: 0.0,
        max: 1.0,
        step: 0.05,
        get,
        set,
        fmt: fmt_unit,
    };
    vec![
        unit("water", |p| p.water, |p, v| p.water = v),
        Knob {
            label: "temp",
            min: -1.0,
            max: 1.0,
            step: 0.05,
            get: |p| p.temp,
            set: |p, v| p.temp = v,
            fmt: fmt_signed,
        },
        unit("humidity", |p| p.humidity, |p, v| p.humidity = v),
        unit("vegetation", |p| p.vegetation, |p, v| p.vegetation = v),
        unit("ice", |p| p.ice, |p, v| p.ice = v),
        unit("cloudiness", |p| p.cloudiness, |p, v| p.cloudiness = v),
        unit("atmosphere", |p| p.atmosphere, |p, v| p.atmosphere = v),
        unit("bloom", |p| p.bloom, |p, v| p.bloom = v),
        unit("glow", |p| p.glow, |p, v| p.glow = v),
        Knob {
            label: "tilt",
            min: -90.0,
            max: 90.0,
            step: 5.0,
            get: |p| p.tilt_deg(),
            set: |p, v| p.tilt = Some(v),
            fmt: fmt_deg,
        },
    ]
}

/// The editor's mutable state.
struct Editor {
    params: PlanetParams,
    cloud_mode: CloudMode,
    size: u32,
    mode: graphix::Mode,
    knobs: Vec<Knob>,
    selected: usize,
    renderer: Renderer,
    theme: Theme,
    start: Instant,
}

impl Editor {
    fn new(params: PlanetParams, cloud_mode: CloudMode, size: u32, mode: graphix::Mode) -> Self {
        let renderer = build_renderer(&params, cloud_mode, size);
        Editor {
            params,
            cloud_mode,
            size,
            mode,
            knobs: knobs(),
            selected: 0,
            renderer,
            theme: Theme::default(),
            start: Instant::now(),
        }
    }

    /// Re-bake the planet after a parameter, cloud-mode, or seed change.
    fn rebuild(&mut self) {
        self.renderer = build_renderer(&self.params, self.cloud_mode, self.size);
    }

    fn select(&mut self, delta: isize) {
        let n = self.knobs.len() as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }

    fn turn_selected(&mut self, steps: f32) {
        self.knobs[self.selected].turn(&mut self.params, steps);
        self.rebuild();
    }

    fn cycle_clouds(&mut self) {
        self.cloud_mode = match self.cloud_mode {
            CloudMode::Realistic => CloudMode::Cartoon,
            CloudMode::Cartoon => CloudMode::None,
            CloudMode::None => CloudMode::Realistic,
        };
        self.rebuild();
    }

    fn new_seed(&mut self) {
        self.params.seed = random_seed();
        self.rebuild();
    }
}

fn build_renderer(params: &PlanetParams, cloud_mode: CloudMode, size: u32) -> Renderer {
    Renderer::new(
        params.clone(),
        cloud_mode,
        size,
        PERIOD,
        ZoomRamp::fixed(1.0),
    )
}

/// A ratatui widget that blits a graphix cell grid into the buffer, centered.
struct PlanetView<'a> {
    grid: &'a [Vec<graphix::Cell>],
}

impl Widget for PlanetView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rows = self.grid.len() as u16;
        let cols = self.grid.first().map_or(0, Vec::len) as u16;
        let ox = area.x + area.width.saturating_sub(cols) / 2;
        let oy = area.y + area.height.saturating_sub(rows) / 2;
        for (ry, row) in self.grid.iter().enumerate() {
            for (rx, cell) in row.iter().enumerate() {
                let (x, y) = (ox + rx as u16, oy + ry as u16);
                if x >= area.right() || y >= area.bottom() {
                    continue;
                }
                if let Some(target) = buf.cell_mut((x, y)) {
                    target.set_symbol(&cell.ch.to_string());
                    target.set_fg(Color::Rgb(cell.fg.r, cell.fg.g, cell.fg.b));
                    target.set_bg(Color::Rgb(cell.bg.r, cell.bg.g, cell.bg.b));
                }
            }
        }
    }
}

/// A knob bar as full/partial/empty block glyphs.
fn bar(fraction: f32) -> String {
    let eighths = (fraction.clamp(0.0, 1.0) * (BAR_WIDTH * 8) as f32).round() as usize;
    let full = eighths / 8;
    let rem = eighths % 8;
    let mut s = String::with_capacity(BAR_WIDTH);
    for _ in 0..full {
        s.push('\u{2588}'); // █
    }
    if full < BAR_WIDTH && rem > 0 {
        s.push(EIGHTHS[rem - 1]);
    }
    while s.chars().count() < BAR_WIDTH {
        s.push('\u{00B7}'); // · empty track
    }
    s
}

/// Draw the whole editor UI into `buf` for time `t` (seconds).
fn draw(buf: &mut Buffer, area: Rect, editor: &Editor, t: f32) {
    let theme = &editor.theme;
    Block::default()
        .style(Style::default().bg(theme.bg))
        .render(area, buf);

    let cols =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area);
    draw_planet(buf, cols[0], editor, t);
    draw_knobs(buf, cols[1], editor);
}

fn draw_planet(buf: &mut Buffer, area: Rect, editor: &Editor, t: f32) {
    let chrome = BorderBox::new(editor.theme, BorderStyle::Rounded)
        .border_color(editor.theme.border)
        .title(format!("planet (seed {})", editor.params.seed));
    chrome.render_chrome(area, buf);
    let inner = chrome.inner_area(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let img = editor.renderer.frame(t);
    let (c, r) = graphix::fit_grid(
        img.width(),
        img.height(),
        u32::from(inner.width),
        u32::from(inner.height),
    );
    let grid = graphix::render_cells(&img, c, r, editor.mode);
    PlanetView { grid: &grid }.render(inner, buf);
}

fn draw_knobs(buf: &mut Buffer, area: Rect, editor: &Editor) {
    let theme = &editor.theme;
    let chrome = BorderBox::new(*theme, BorderStyle::Rounded)
        .border_color(theme.border)
        .title(format!("knobs ({})", cloud_label(editor.cloud_mode)));
    chrome.render_chrome(area, buf);
    let inner = chrome.inner_area(area);

    let mut lines = Vec::with_capacity(editor.knobs.len() + 2);
    for (i, knob) in editor.knobs.iter().enumerate() {
        let selected = i == editor.selected;
        let marker = if selected { "\u{203A} " } else { "  " };
        let label = format!("{:<11}", knob.label);
        let label_style = if selected {
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        let bar_style = Style::default().fg(if selected {
            theme.secondary
        } else {
            theme.text_muted
        });
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(theme.primary)),
            Span::styled(label, label_style),
            Span::styled(bar(knob.fraction(&editor.params)), bar_style),
            Span::styled(
                format!(" {}", (knob.fmt)(knob.value(&editor.params))),
                Style::default().fg(theme.text_muted),
            ),
        ]));
    }
    // Two short footer lines so the hints fit even in a narrow panel.
    let hint = Style::default().fg(theme.text_muted);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "\u{2191}/\u{2193} select   \u{2190}/\u{2192} adjust",
        hint,
    )));
    lines.push(Line::from(Span::styled("c clouds   n seed   q quit", hint)));
    Paragraph::new(lines).render(inner, buf);
}

fn cloud_label(mode: CloudMode) -> &'static str {
    match mode {
        CloudMode::Realistic => "realistic clouds",
        CloudMode::Cartoon => "cartoon clouds",
        CloudMode::None => "no clouds",
    }
}

/// Restores the terminal (raw mode, alternate screen, cursor) on drop, so the
/// shell is left usable even if the editor errors or panics.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

/// Run the interactive editor until the user quits.
pub fn run(
    params: PlanetParams,
    cloud_mode: CloudMode,
    size: u32,
    mode: graphix::Mode,
) -> Result<(), Error> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
    let _guard = TerminalGuard;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal: Terminal<CrosstermBackend<Stdout>> = Terminal::new(backend)?;
    let mut editor = Editor::new(params, cloud_mode, size, mode);

    loop {
        let t = editor.start.elapsed().as_secs_f32();
        terminal.draw(|frame| {
            let area = frame.area();
            draw(frame.buffer_mut(), area, &editor, t);
        })?;

        if event::poll(Duration::from_millis(33))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match Input::from(key) {
                Input::Up => editor.select(-1),
                Input::Down => editor.select(1),
                Input::Left => editor.turn_selected(-1.0),
                Input::Right => editor.turn_selected(1.0),
                Input::Char('c') => editor.cycle_clouds(),
                Input::Char('n') => editor.new_seed(),
                Input::Char('q') | Input::Esc => break,
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        Editor::new(
            PlanetParams::default(),
            CloudMode::Realistic,
            64,
            graphix::Mode::Octant,
        )
    }

    #[test]
    fn turning_a_knob_clamps_to_its_range() {
        let mut e = editor();
        // "water" is knob 0, range 0..1, step 0.05.
        for _ in 0..100 {
            e.turn_selected(1.0);
        }
        assert_eq!(e.params.water, 1.0);
        for _ in 0..100 {
            e.turn_selected(-1.0);
        }
        assert_eq!(e.params.water, 0.0);
    }

    #[test]
    fn selection_wraps_both_ways() {
        let mut e = editor();
        let n = e.knobs.len();
        e.select(-1);
        assert_eq!(e.selected, n - 1);
        e.select(1);
        assert_eq!(e.selected, 0);
    }

    #[test]
    fn tilt_knob_sets_an_override() {
        let mut e = editor();
        e.selected = e
            .knobs
            .iter()
            .position(|k| k.label == "tilt")
            .expect("tilt knob");
        assert!(e.params.tilt.is_none());
        e.turn_selected(1.0);
        assert!(e.params.tilt.is_some());
    }

    #[test]
    fn bar_is_empty_full_and_partial() {
        assert!(bar(0.0).chars().all(|c| c == '\u{00B7}'));
        assert!(bar(1.0).chars().all(|c| c == '\u{2588}'));
        assert_eq!(bar(0.5).chars().count(), BAR_WIDTH);
        assert!(bar(0.5).contains('\u{2588}'));
    }

    #[test]
    fn cloud_cycle_is_three_way() {
        let mut e = editor();
        assert_eq!(e.cloud_mode, CloudMode::Realistic);
        e.cycle_clouds();
        assert_eq!(e.cloud_mode, CloudMode::Cartoon);
        e.cycle_clouds();
        assert_eq!(e.cloud_mode, CloudMode::None);
        e.cycle_clouds();
        assert_eq!(e.cloud_mode, CloudMode::Realistic);
    }

    #[test]
    fn draw_fills_the_buffer_with_labels() {
        let e = editor();
        let area = Rect::new(0, 0, 100, 30);
        let mut buf = Buffer::empty(area);
        draw(&mut buf, area, &e, 0.0);
        let text: String = buf
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(text.contains("water"));
        assert!(text.contains("cloudiness"));
        assert!(text.contains("quit"));
    }
}
