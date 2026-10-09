//! The blocks that ship with the crate.
//!
//! The four materials from `misc-graphics/mineblocks.png` come first; the rest
//! exist to show that the same primitives reach past speckled rubble. Palettes
//! are measured off that image, unshaded, so they describe a top face.

use crate::color::{ColorSpec, Ramp};
use crate::pattern::Pattern;
use crate::recipe::{BlockRecipe, FaceRecipe};

/// Every builtin, in a stable order.
pub fn all() -> Vec<BlockRecipe> {
    vec![stone(), sand(), dirt(), grass(), planks(), bricks()]
}

/// The builtin named `name`, if there is one.
pub fn get(name: &str) -> Option<BlockRecipe> {
    all().into_iter().find(|r| r.name == name)
}

/// The names of every builtin.
pub fn names() -> Vec<String> {
    all().into_iter().map(|r| r.name).collect()
}

/// The brown of bare earth, which both dirt and the sides of grass are made of.
fn earth() -> Ramp {
    Ramp::rgb([[83, 57, 38], [113, 79, 54], [140, 101, 69], [172, 124, 86]])
}

/// The green of a grass top and of the fringe hanging over its sides.
fn turf() -> Ramp {
    Ramp::rgb([[60, 92, 52], [67, 102, 59], [74, 112, 64], [83, 126, 72]])
}

/// Grey rubble: uniform grain across a narrow range of greys.
pub fn stone() -> BlockRecipe {
    let face = FaceRecipe::new(
        Ramp::rgb([
            [97, 97, 97],
            [108, 108, 108],
            [118, 118, 118],
            [133, 133, 133],
        ]),
        Pattern::Speckle { contrast: 1.0 },
    );
    BlockRecipe::uniform("stone", face)
}

/// Pale grit. Correlated noise rather than per-texel grain, so the surface
/// reads as drifted rather than crushed.
pub fn sand() -> BlockRecipe {
    let face = FaceRecipe::new(
        Ramp::rgb([
            [205, 182, 135],
            [209, 192, 147],
            [214, 203, 160],
            [222, 215, 172],
        ]),
        Pattern::Blotch {
            scale: 5.0,
            contrast: 1.0,
            octaves: 3,
        },
    );
    BlockRecipe::uniform("sand", face)
}

/// Earth with grey pebbles worked through it.
pub fn dirt() -> BlockRecipe {
    BlockRecipe::uniform(
        "dirt",
        FaceRecipe::new(earth(), Pattern::Layer(earth_layers())),
    )
}

/// Grain and pebbles, the two layers earth is made of, so grass can put its
/// fringe on top of them without nesting one layer inside another.
fn earth_layers() -> Vec<Pattern> {
    vec![
        Pattern::Speckle { contrast: 1.0 },
        Pattern::Scatter {
            cell: 5,
            density: 0.35,
            size: 1,
            color: ColorSpec::Rgb([116, 116, 124]),
        },
    ]
}

/// Turf over earth: a green top, and sides of dirt under a band of grass whose
/// lower edge drips a texel or two.
pub fn grass() -> BlockRecipe {
    let top = FaceRecipe::new(turf(), Pattern::Speckle { contrast: 1.0 });
    let mut layers = earth_layers();
    layers.push(Pattern::Fringe {
        depth: 3,
        jitter: 2,
        span: 2,
        ramp: turf(),
        pattern: Box::new(Pattern::Speckle { contrast: 1.0 }),
    });
    BlockRecipe::new(
        "grass",
        top,
        FaceRecipe::new(earth(), Pattern::Layer(layers)),
    )
}

/// Sawn timber: horizontal seams with the grain running between them.
pub fn planks() -> BlockRecipe {
    let ramp = Ramp::rgb([
        [120, 88, 50],
        [141, 105, 60],
        [157, 118, 68],
        [173, 132, 78],
    ]);
    let grain = Pattern::Blotch {
        scale: 6.0,
        contrast: 1.0,
        octaves: 4,
    };
    let seams = Pattern::Lattice {
        rows: 4,
        cols: 0,
        stagger: false,
        mortar: ColorSpec::Rgb([84, 61, 34]),
    };
    let boards = Pattern::Layer(vec![grain, seams]);
    BlockRecipe::uniform("planks", FaceRecipe::new(ramp, boards))
}

/// Fired clay in staggered courses, with mortar between them.
pub fn bricks() -> BlockRecipe {
    let ramp = Ramp::rgb([[118, 60, 47], [134, 70, 55], [148, 79, 62], [162, 90, 71]]);
    let clay = Pattern::Speckle { contrast: 0.6 };
    let courses = Pattern::Lattice {
        rows: 4,
        cols: 4,
        stagger: true,
        mortar: ColorSpec::Rgb([160, 152, 142]),
    };
    BlockRecipe::new(
        "bricks",
        FaceRecipe::new(ramp.clone(), clay.clone()),
        FaceRecipe::new(ramp, Pattern::Layer(vec![clay, courses])),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iso::BlockSize;

    #[test]
    fn every_builtin_is_named_once_and_findable() {
        let names = names();
        let unique: std::collections::HashSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "duplicate builtin name");
        for name in &names {
            assert_eq!(get(name).map(|r| r.name).as_ref(), Some(name));
        }
        assert_eq!(get("obsidian"), None);
    }

    #[test]
    fn every_builtin_bakes() {
        for recipe in all() {
            recipe
                .bake(1, BlockSize::new(16, 2))
                .unwrap_or_else(|e| panic!("{} failed to bake: {e}", recipe.name));
        }
    }

    #[test]
    fn the_reference_materials_are_distinguishable() {
        // Each of the four should occupy its own part of the color space, or
        // the palettes have been copied wrong.
        let size = BlockSize::new(16, 2);
        let hue = |recipe: BlockRecipe| {
            let img = recipe.bake(3, size).expect("bake").to_image();
            let q = size.quarter();
            let p = img.get_pixel(2 * q, q).0;
            (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]))
        };
        let (sr, sg, sb) = hue(stone());
        assert!(
            (sr - sg).abs() < 4 && (sg - sb).abs() < 4,
            "stone is not grey"
        );
        let (nr, _, nb) = hue(sand());
        assert!(nr > nb + 40 && nr > 190, "sand is not a pale warm tone");
        let (dr, dg, db) = hue(dirt());
        assert!(dr > dg && dg > db, "dirt is not brown");
        let (gr, gg, gb) = hue(grass());
        assert!(gg > gr && gg > gb, "the grass top is not green");
    }

    #[test]
    fn grass_wears_green_on_top_and_earth_below() {
        let size = BlockSize::new(16, 4);
        let img = grass().bake(7, size).expect("bake").to_image();
        let q = size.quarter();
        let green = |x, y| {
            let p = img.get_pixel(x, y).0;
            p[1] > p[0] && p[1] > p[2]
        };
        assert!(green(2 * q, q), "the top face should be green");

        // How far the fringe hangs down each column of the left face, whose
        // top edge runs from (0, q) to (2q, 2q). A single column may carry no
        // fringe at all, so the shape is checked over the whole face.
        let texel = 2 * q / size.texels();
        let depth = |x| (1..2 * q).take_while(|dy| green(x, q + x / 2 + dy)).count() as u32;
        let depths: Vec<u32> = (0..2 * q).map(depth).collect();
        let deepest = depths.iter().copied().max().unwrap_or(0);
        // The recipe asks for three texels give or take two, so five is the
        // most it may reach; anything deeper means the band has run away.
        assert!(deepest <= 5 * texel, "the fringe hangs {deepest} deep");
        assert!(
            deepest >= 2 * texel,
            "the fringe is barely there at {deepest} deep"
        );
        let dressed = depths.iter().filter(|&&d| d > 0).count();
        assert!(
            dressed > depths.len() / 2,
            "only {dressed} columns carry any fringe"
        );
    }

    #[test]
    fn a_lattice_block_shows_its_mortar_on_the_sides_only() {
        // Courses belong on the walls; the top of a brick block is plain clay.
        let size = BlockSize::new(16, 4);
        let img = bricks().bake(2, size).expect("bake").to_image();
        let q = size.quarter();
        let mortar = crate::color::shade([160, 152, 142], 0.80);
        let row: Vec<[u8; 4]> = (0..q).map(|x| img.get_pixel(x + q / 2, 3 * q).0).collect();
        assert!(
            row.iter().any(|p| p[..3] == mortar[..]),
            "no mortar on the left face"
        );
    }
}
