use std::f32::consts::TAU;

use gpui::{App, AppContext as _, Hsla, ImgResourceLoader, Rgba, SharedString, Task};

use crate::artwork::resource;

const BINS: usize = 24;
const SAMPLES: usize = 6000;
const MIN_ALPHA: u8 = 128;
const MIN_SATURATION: f32 = 0.14;
const MIN_LIGHTNESS: f32 = 0.20;
const MAX_LIGHTNESS: f32 = 0.94;
const MIN_SHARE: f32 = 0.05;
const SECOND_SHARE: f32 = 0.15;
const PALETTE: usize = 5;

pub fn tint(url: impl Into<SharedString>, cx: &mut App) -> Task<Option<Hsla>> {
    read(url, cx, dominant)
}

/// The colour a cover carries after its main one. Artwork with only one colour
/// in it answers with that colour rather than nothing.
pub fn secondary(url: impl Into<SharedString>, cx: &mut App) -> Task<Option<Hsla>> {
    read(url, cx, second)
}

/// The colours a picture is made of, most common first, up to `PALETTE`. Unlike `tint`
/// this does not look at hue alone: a portrait in warm tones would collapse into one
/// brown, so the pixels are split by median cut and dark, light and skin stay apart.
pub fn palette(url: impl Into<SharedString>, cx: &mut App) -> Task<Vec<Hsla>> {
    let found = read(url, cx, |pixels| Some(median_cut(pixels, PALETTE)));
    cx.spawn(async move |_| found.await.unwrap_or_default())
}

fn read<T: Send + 'static>(
    url: impl Into<SharedString>,
    cx: &mut App,
    pick: fn(&[u8]) -> Option<T>,
) -> Task<Option<T>> {
    let (load, _) = cx.fetch_asset::<ImgResourceLoader>(&resource(url));

    cx.spawn(async move |cx| {
        let image = load.await.ok()?;
        cx.background_spawn(async move { pick(image.as_bytes(0)?) })
            .await
    })
}

#[derive(Clone, Copy, Default)]
struct Bin {
    weight: f32,
    x: f32,
    y: f32,
    saturation: f32,
    lightness: f32,
}

impl Bin {
    fn add(&mut self, color: Hsla, weight: f32) {
        let angle = color.h * TAU;
        self.weight += weight;
        self.x += angle.cos() * weight;
        self.y += angle.sin() * weight;
        self.saturation += color.s * weight;
        self.lightness += color.l * weight;
    }

    fn merge(&mut self, other: &Self) {
        self.weight += other.weight;
        self.x += other.x;
        self.y += other.y;
        self.saturation += other.saturation;
        self.lightness += other.lightness;
    }

    fn colour(&self) -> Option<Hsla> {
        (self.weight > 0.).then(|| Hsla {
            h: self.y.atan2(self.x).rem_euclid(TAU) / TAU,
            s: (self.saturation / self.weight).clamp(0., 1.),
            l: (self.lightness / self.weight).clamp(0., 1.),
            a: 1.,
        })
    }
}

fn dominant(pixels: &[u8]) -> Option<Hsla> {
    clusters(pixels, 1).first().copied()
}

fn second(pixels: &[u8]) -> Option<Hsla> {
    clusters(pixels, 2).last().copied()
}

/// The strongest hue clusters an image holds, strongest first. A cluster takes
/// the bins on either side of its peak with it and leaves them empty, so the
/// runner-up is a colour of its own rather than the shoulder of the winner.
///
/// The first has to carry its share of the whole image; every one after answers
/// to the winner instead, because a second colour is only ever a minority of a
/// cover and measuring it against the image again would reject them all.
fn clusters(pixels: &[u8], wanted: usize) -> Vec<Hsla> {
    let (mut bins, sampled) = binned(pixels);
    let mut found = Vec::new();
    let mut floor = sampled * MIN_SHARE;

    for _ in 0..wanted {
        let Some(peak) = (0..BINS).max_by(|&a, &b| score(&bins, a).total_cmp(&score(&bins, b)))
        else {
            break;
        };
        let weight = score(&bins, peak);
        if weight < floor {
            break;
        }

        let mut cluster = bins[peak];
        for side in [BINS - 1, 1] {
            cluster.merge(&bins[(peak + side) % BINS]);
        }
        let Some(colour) = cluster.colour() else {
            break;
        };
        found.push(colour);
        floor = weight * SECOND_SHARE;

        for offset in [BINS - 1, 0, 1] {
            bins[(peak + offset) % BINS] = Bin::default();
        }
    }
    found
}

/// Splits the sampled pixels in two across their widest channel, again and again, always
/// cutting the box that spans the most, then answers each box's mean by size.
fn median_cut(pixels: &[u8], wanted: usize) -> Vec<Hsla> {
    let stride = (pixels.len() / 4 / SAMPLES).max(1);
    let sampled: Vec<[u8; 3]> = pixels
        .chunks_exact(4)
        .step_by(stride)
        .filter(|pixel| pixel[3] >= MIN_ALPHA)
        .map(|pixel| [pixel[2], pixel[1], pixel[0]])
        .collect();

    let spread = |pixels: &[[u8; 3]]| {
        (0..3)
            .map(|channel| {
                let values = pixels.iter().map(|pixel| pixel[channel]);
                let (low, high) = values.fold((255, 0), |(low, high), value| {
                    (low.min(value), high.max(value))
                });
                (high.saturating_sub(low), channel)
            })
            .max()
            .unwrap_or_default()
    };

    let mut boxes = vec![sampled];
    while boxes.len() < wanted {
        let Some((index, (width, channel))) = boxes
            .iter()
            .map(|pixels| spread(pixels))
            .enumerate()
            .max_by_key(|(_, spread)| spread.0)
        else {
            break;
        };
        if width == 0 {
            break;
        }
        let mut cut = boxes.swap_remove(index);
        cut.sort_unstable_by_key(|pixel| pixel[channel]);
        // At the middle of the range, not of the count: a big patch of one colour
        // must not drag the cut into it and smear two colours into one mean.
        let middle = cut[0][channel] + width / 2;
        let rest = cut.split_off(cut.partition_point(|pixel| pixel[channel] <= middle));
        boxes.extend([cut, rest]);
    }

    boxes.retain(|pixels| !pixels.is_empty());
    boxes.sort_by_key(|pixels| std::cmp::Reverse(pixels.len()));
    boxes
        .iter()
        .map(|pixels| {
            let sum = pixels.iter().fold([0f32; 3], |sum, pixel| {
                [0, 1, 2].map(|channel| sum[channel] + pixel[channel] as f32)
            });
            let [r, g, b] = sum.map(|sum| sum / pixels.len() as f32 / 255.);
            Hsla::from(Rgba { r, g, b, a: 1. })
        })
        .collect()
}

fn binned(pixels: &[u8]) -> ([Bin; BINS], f32) {
    let stride = (pixels.len() / 4 / SAMPLES).max(1);
    let mut bins = [Bin::default(); BINS];
    let mut sampled = 0.;

    for pixel in pixels.chunks_exact(4).step_by(stride) {
        let [blue, green, red, alpha] = [pixel[0], pixel[1], pixel[2], pixel[3]];
        if alpha < MIN_ALPHA {
            continue;
        }
        sampled += 1.;

        let colour = Hsla::from(Rgba {
            r: red as f32 / 255.,
            g: green as f32 / 255.,
            b: blue as f32 / 255.,
            a: 1.,
        });
        if colour.s < MIN_SATURATION || colour.l < MIN_LIGHTNESS || colour.l > MAX_LIGHTNESS {
            continue;
        }

        let index = ((colour.h * BINS as f32) as usize).min(BINS - 1);
        bins[index].add(colour, colour.s * (1. - (colour.l - 0.5).abs()));
    }

    (bins, sampled)
}

fn score(bins: &[Bin; BINS], index: usize) -> f32 {
    bins[index].weight
        + (bins[(index + BINS - 1) % BINS].weight + bins[(index + 1) % BINS].weight) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(colours: &[([u8; 3], usize)]) -> Vec<u8> {
        colours
            .iter()
            .flat_map(|&([red, green, blue], count)| {
                std::iter::repeat_n([blue, green, red, 255], count).flatten()
            })
            .collect()
    }

    #[test]
    fn finds_the_dominant_hue() {
        let pixels = image(&[([204, 34, 34], 900), ([32, 32, 32], 100)]);
        let colour = dominant(&pixels).expect("a red image is colourful");

        assert!(colour.h < 0.02 || colour.h > 0.98, "hue was {}", colour.h);
        assert!(colour.s > 0.5, "saturation was {}", colour.s);
    }

    #[test]
    fn averages_across_the_bin_boundary() {
        let pixels = image(&[([255, 0, 60], 500), ([255, 60, 0], 500)]);
        let colour = dominant(&pixels).expect("a red image is colourful");

        assert!(colour.h < 0.02 || colour.h > 0.98, "hue was {}", colour.h);
    }

    #[test]
    fn ignores_greyscale_artwork() {
        let pixels = image(&[([18, 18, 18], 500), ([200, 200, 200], 500)]);

        assert!(dominant(&pixels).is_none());
    }

    #[test]
    fn ignores_a_small_splash_of_colour() {
        let pixels = image(&[([120, 120, 120], 990), ([0, 180, 255], 10)]);

        assert!(dominant(&pixels).is_none());
    }

    #[test]
    fn a_warm_portrait_keeps_its_darks_and_lights_apart() {
        let pixels = image(&[
            ([120, 60, 40], 400),
            ([15, 12, 12], 300),
            ([235, 200, 195], 300),
        ]);
        let found = median_cut(&pixels, 3);

        assert_eq!(found.len(), 3);
        assert!(found.iter().any(|colour| colour.l < 0.1));
        assert!(found.iter().any(|colour| colour.l > 0.8));
    }

    #[test]
    fn a_flat_picture_gives_one_colour() {
        let pixels = image(&[([90, 90, 90], 100)]);

        assert_eq!(median_cut(&pixels, 5).len(), 1);
    }
}
