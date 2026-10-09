//! The scene file format: a legend, then one character grid per level.
//!
//! ```text
//! s stone
//! g grass
//!
//! # z = 0
//! sss
//! sss
//!
//! # z = 1
//! .g.
//! ggg
//! ```
//!
//! Everything before the first blank line is the legend, one `char name` pair
//! per line. Each blank-line-separated group after it is one level, stacked
//! upward from `z = 0`; within a level, rows run along `y` and columns along
//! `x`. A `.` or a space is empty, and whole lines starting with `#` are
//! comments.
//!
//! One thing to watch when placing blocks: `(1, 1, 1)` is the direction the
//! view looks down, so two positions a whole number of those steps apart land
//! on exactly the same pixels. The nearer one then hides the further one
//! completely, which is right but reads as though it had been substituted into
//! the structure rather than floating in front of it. A block meant to look
//! like it hangs in the air wants to be clear of the others' view rays.

use crate::Error;
use crate::iso::BlockSize;
use crate::recipe::BlockRecipe;
use crate::scene::Scene;

/// Characters that mean no block.
const AIR: [char; 2] = ['.', ' '];

/// A parsed scene file: which named block sits at each grid position.
///
/// Names are not resolved here, because the parser has no catalog to resolve
/// them against; [`Layout::build`] does that and reports the legend line of
/// anything it cannot find.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    names: Vec<String>,
    lines: Vec<usize>,
    cells: Vec<(i32, i32, i32, usize)>,
}

impl Layout {
    /// The distinct block names the legend declares, in its own order.
    pub fn names(&self) -> Vec<&str> {
        self.names.iter().map(String::as_str).collect()
    }

    /// How many blocks the layout places.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the layout places no blocks.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Bake each named block once and place every instance of it.
    ///
    /// `recipes` looks a name up in whatever catalog the caller has. Baking
    /// happens once per name however many times the block appears, which is
    /// the point of separating a recipe from a compiled block.
    pub fn build(
        &self,
        size: BlockSize,
        seed: u64,
        recipes: impl Fn(&str) -> Option<BlockRecipe>,
    ) -> Result<Scene, Error> {
        let mut scene = Scene::new();
        let mut handles = Vec::with_capacity(self.names.len());
        for (name, &line) in self.names.iter().zip(&self.lines) {
            let recipe = recipes(name).ok_or_else(|| Error::Scene {
                line,
                reason: format!("no block named {name}"),
            })?;
            handles.push(scene.add(recipe.bake(seed, size)?));
        }
        for &(x, y, z, i) in &self.cells {
            if let Some(&handle) = handles.get(i) {
                scene.place(x, y, z, handle);
            }
        }
        Ok(scene)
    }
}

/// Read a scene file.
pub fn parse(text: &str) -> Result<Layout, Error> {
    // Comments drop out entirely, but the line numbers they occupied stay so
    // errors point at the right place in the file the user wrote.
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim_end()))
        .filter(|(_, l)| !l.trim_start().starts_with('#'))
        .collect();

    let mut groups: Vec<Vec<(usize, &str)>> = vec![Vec::new()];
    for (line, text) in lines {
        match (text.trim().is_empty(), groups.last_mut()) {
            // Runs of blank lines separate one group, not several.
            (true, Some(group)) if !group.is_empty() => groups.push(Vec::new()),
            (false, Some(group)) => group.push((line, text)),
            _ => {}
        }
    }
    groups.retain(|g| !g.is_empty());

    let mut layout = Layout::default();
    let Some((legend, levels)) = groups.split_first() else {
        return Ok(layout);
    };

    let mut chars: Vec<(char, usize)> = Vec::new();
    for &(line, text) in legend {
        let (symbol, name) = read_legend(line, text)?;
        if AIR.contains(&symbol) {
            // `. air` is a common thing to write; empty is already empty.
            continue;
        }
        if chars.iter().any(|&(c, _)| c == symbol) {
            return Err(Error::Scene {
                line,
                reason: format!("{symbol} is already in the legend"),
            });
        }
        chars.push((symbol, layout.names.len()));
        layout.names.push(name.to_string());
        layout.lines.push(line);
    }

    for (z, level) in levels.iter().enumerate() {
        let width = level.first().map_or(0, |(_, t)| t.chars().count());
        for (y, &(line, text)) in level.iter().enumerate() {
            if text.chars().count() != width {
                return Err(Error::Scene {
                    line,
                    reason: format!("row is {} wide, the level is {width}", text.chars().count()),
                });
            }
            for (x, symbol) in text.chars().enumerate() {
                if AIR.contains(&symbol) {
                    continue;
                }
                let Some(&(_, i)) = chars.iter().find(|&&(c, _)| c == symbol) else {
                    return Err(Error::Scene {
                        line,
                        reason: format!("{symbol} is not in the legend"),
                    });
                };
                layout.cells.push((x as i32, y as i32, z as i32, i));
            }
        }
    }
    Ok(layout)
}

/// Split one legend line into its symbol and the name it stands for.
fn read_legend(line: usize, text: &str) -> Result<(char, &str), Error> {
    let mut chars = text.chars();
    let bad = |reason: &str| Error::Scene {
        line,
        reason: reason.to_string(),
    };
    let symbol = chars.next().ok_or_else(|| bad("empty legend line"))?;
    let name = chars.as_str().trim();
    if !chars.as_str().starts_with(char::is_whitespace) {
        return Err(bad("a legend line is one character, a space, then a name"));
    }
    if name.is_empty() {
        return Err(bad("legend entry has no name"));
    }
    Ok((symbol, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin;

    const HOUSE: &str = "\
# legend
s stone
g grass

# z = 0
sss
sss

# z = 1
.g.
ggg
";

    fn build(text: &str) -> Result<Scene, Error> {
        parse(text)?.build(BlockSize::new(4, 2), 1, builtin::get)
    }

    #[test]
    fn a_scene_file_places_every_marked_cell() {
        let layout = parse(HOUSE).expect("the example must parse");
        assert_eq!(layout.names(), vec!["stone", "grass"]);
        // Six stone on the ground, four grass above it.
        assert_eq!(layout.len(), 10);
        let scene = layout
            .build(BlockSize::new(4, 2), 1, builtin::get)
            .expect("both blocks are builtins");
        assert_eq!(scene.len(), 10);
    }

    /// The scenes under `scenes/`, embedded so they cannot go missing.
    const SHIPPED: [(&str, &str); 2] = [
        ("arch", include_str!("../scenes/arch.txt")),
        ("stairs", include_str!("../scenes/stairs.txt")),
    ];

    #[test]
    fn the_shipped_scenes_parse_and_build() {
        for (name, text) in SHIPPED {
            let layout = parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!layout.is_empty(), "{name} places no blocks");
            layout
                .build(BlockSize::new(4, 1), 0, builtin::get)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn the_arch_scene_actually_hangs_in_the_air() {
        // The example exists to show blocks that rest on nothing, so it is
        // worth checking it still does after an edit.
        let layout = parse(SHIPPED[0].1).expect("arch must parse");
        let filled: std::collections::HashSet<(i32, i32, i32)> =
            layout.cells.iter().map(|&(x, y, z, _)| (x, y, z)).collect();
        let unsupported = filled
            .iter()
            .filter(|&&(x, y, z)| z > 0 && !filled.contains(&(x, y, z - 1)))
            .count();
        assert!(unsupported > 0, "every block in the arch rests on another");
    }

    #[test]
    fn columns_run_along_x_rows_along_y_and_levels_stack_upward() {
        // Two levels of one row each: `a` then `b`, and `b` then `a` above it.
        let layout = parse("a stone\nb grass\n\nab\n\nba\n").expect("parse");
        let mut cells = layout.cells.clone();
        cells.sort();
        assert_eq!(
            cells,
            vec![
                (0, 0, 0, 0), // x=0, y=0, z=0 is `a`, name 0
                (0, 0, 1, 1), // directly above it is `b`, name 1
                (1, 0, 0, 1),
                (1, 0, 1, 0),
            ]
        );
    }

    #[test]
    fn air_needs_no_legend_entry() {
        let layout = parse(". air\ns stone\n\n.s.\n").expect("parse");
        assert_eq!(layout.names(), vec!["stone"]);
        assert_eq!(layout.len(), 1);
        // A space is empty too, so a level can be indented raggedly on purpose.
        assert_eq!(parse("s stone\n\n s \n").expect("parse").len(), 1);
    }

    #[test]
    fn comments_and_blank_lines_do_not_shift_anything() {
        let terse = parse("s stone\n\nss\n").expect("parse");
        let padded = parse("# hello\ns stone\n\n\n# a level\nss\n").expect("parse");
        assert_eq!(terse.cells, padded.cells);
    }

    #[test]
    fn an_empty_file_is_an_empty_layout() {
        assert!(parse("").expect("parse").is_empty());
        assert!(parse("# nothing here\n").expect("parse").is_empty());
        // A legend with no levels places nothing but still declares its names.
        assert_eq!(parse("s stone\n").expect("parse").names(), vec!["stone"]);
    }

    #[test]
    fn a_symbol_missing_from_the_legend_is_reported_with_its_line() {
        let err = parse("s stone\n\nsx\n").expect_err("x is undeclared");
        let Error::Scene { line, reason } = err else {
            panic!("expected a scene error");
        };
        assert_eq!(line, 3);
        assert!(reason.contains('x'), "unhelpful message: {reason}");
    }

    #[test]
    fn a_ragged_level_is_reported_rather_than_padded() {
        let err = parse("s stone\n\nsss\nss\n").expect_err("the second row is short");
        assert!(matches!(err, Error::Scene { line: 4, .. }));
    }

    #[test]
    fn a_malformed_legend_is_reported() {
        for text in ["stone\n\ns\n", "s\n\ns\n", "s \n\ns\n"] {
            assert!(
                matches!(parse(text), Err(Error::Scene { line: 1, .. })),
                "{text:?} parsed"
            );
        }
        // The same symbol twice is a typo worth catching.
        assert!(matches!(
            parse("s stone\ns sand\n\ns\n"),
            Err(Error::Scene { line: 2, .. })
        ));
    }

    #[test]
    fn an_unknown_block_name_is_reported_at_its_legend_line() {
        let err = build("s stone\nq quartz\n\nsq\n").expect_err("quartz is not a builtin");
        let Error::Scene { line, reason } = err else {
            panic!("expected a scene error");
        };
        assert_eq!(line, 2);
        assert!(reason.contains("quartz"), "unhelpful message: {reason}");
    }

    #[test]
    fn a_block_named_twice_is_baked_once() {
        // Every cell of a solid level shares one compiled block, which is the
        // whole reason a recipe and a sprite are different things.
        let layout = parse("s stone\n\nssss\nssss\n").expect("parse");
        assert_eq!(layout.names().len(), 1);
        assert_eq!(layout.len(), 8);
    }
}
