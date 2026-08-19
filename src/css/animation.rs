//! `transition` and `@keyframes` — values that change over time.
//!
//! Both come down to the same thing: two CSS values and a fraction of the way
//! between them. Rather than teach every property how to be half-way to
//! somewhere, the interpolation here works on the written value and hands the
//! result back to the ordinary declaration parser. A property that can be
//! parsed can therefore be animated, and one whose two ends are not the same
//! shape simply swaps over at the half-way mark, which is what CSS calls a
//! discrete animation.

use super::{Declaration, LengthContext, parse_color_value};

/// How a value is paced between its two ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    Linear,
    CubicBezier(f32, f32, f32, f32),
    /// `steps(n)`; `jump_start` is `steps(n, start)`.
    Steps(u32, bool),
}

impl Default for Easing {
    /// CSS's own initial value for both `transition` and `animation`.
    fn default() -> Self {
        Easing::CubicBezier(0.25, 0.1, 0.25, 1.0)
    }
}

impl Easing {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        match value.as_str() {
            "linear" => return Some(Easing::Linear),
            "ease" => return Some(Easing::CubicBezier(0.25, 0.1, 0.25, 1.0)),
            "ease-in" => return Some(Easing::CubicBezier(0.42, 0.0, 1.0, 1.0)),
            "ease-out" => return Some(Easing::CubicBezier(0.0, 0.0, 0.58, 1.0)),
            "ease-in-out" => return Some(Easing::CubicBezier(0.42, 0.0, 0.58, 1.0)),
            "step-start" => return Some(Easing::Steps(1, true)),
            "step-end" => return Some(Easing::Steps(1, false)),
            _ => {}
        }

        if let Some(args) = strip_call(&value, "cubic-bezier") {
            let numbers: Vec<f32> = args
                .split(',')
                .filter_map(|part| part.trim().parse::<f32>().ok())
                .collect();
            if numbers.len() == 4 {
                return Some(Easing::CubicBezier(
                    numbers[0], numbers[1], numbers[2], numbers[3],
                ));
            }
        }
        if let Some(args) = strip_call(&value, "steps") {
            let mut parts = args.split(',');
            let count = parts.next()?.trim().parse::<u32>().ok()?.max(1);
            let at_start = parts.next().is_some_and(|keyword| {
                keyword.trim().starts_with("jump-start") || keyword.trim() == "start"
            });
            return Some(Easing::Steps(count, at_start));
        }
        None
    }

    /// The eased fraction for a linear fraction of the way through.
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match *self {
            Easing::Linear => t,
            Easing::CubicBezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, t),
            Easing::Steps(count, at_start) => {
                let count = count.max(1) as f32;
                let step = if at_start {
                    (t * count).floor() + 1.0
                } else {
                    (t * count).floor()
                };
                (step / count).clamp(0.0, 1.0)
            }
        }
    }
}

/// Solve a CSS cubic Bézier for `y` at the given `x`.
///
/// The curve is parametric, so `x` has to be inverted first; a handful of
/// Newton steps from a sensible guess gets well inside a pixel of accuracy,
/// and bisection catches the flat stretches where Newton stalls.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let curve = |a: f32, b: f32, t: f32| {
        let inv = 1.0 - t;
        3.0 * inv * inv * t * a + 3.0 * inv * t * t * b + t * t * t
    };
    let slope = |a: f32, b: f32, t: f32| {
        let inv = 1.0 - t;
        3.0 * inv * inv * (a) + 6.0 * inv * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };

    let mut t = x;
    for _ in 0..8 {
        let error = curve(x1, x2, t) - x;
        if error.abs() < 1e-5 {
            return curve(y1, y2, t);
        }
        let derivative = slope(x1, x2, t);
        if derivative.abs() < 1e-6 {
            break;
        }
        t -= error / derivative;
    }

    let (mut low, mut high) = (0.0f32, 1.0f32);
    let mut t = x;
    for _ in 0..24 {
        let value = curve(x1, x2, t);
        if (value - x).abs() < 1e-5 {
            break;
        }
        if value < x {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) / 2.0;
    }
    curve(y1, y2, t)
}

/// Parse a CSS `<time>` into seconds.
pub fn parse_time(token: &str) -> Option<f32> {
    let token = token.trim().to_ascii_lowercase();
    if let Some(number) = token.strip_suffix("ms") {
        return number.trim().parse::<f32>().ok().map(|ms| ms / 1000.0);
    }
    if let Some(number) = token.strip_suffix('s') {
        return number.trim().parse::<f32>().ok();
    }
    // A bare `0` is a valid time; anything else without a unit is not.
    token.parse::<f32>().ok().filter(|value| *value == 0.0)
}

/// A value part-way between two written CSS values.
///
/// Colours blend channel by channel; anything else blends the numbers it
/// contains, provided the text around them matches — so `10px` to `40px` is a
/// smooth slide, and `10px` to `auto` is a swap at the half-way mark.
pub fn interpolate_value(from: &str, to: &str, t: f32) -> String {
    if t <= 0.0 {
        return from.trim().to_string();
    }
    if t >= 1.0 {
        return to.trim().to_string();
    }

    if let (Some(start), Some(end)) = (parse_color_value(from), parse_color_value(to)) {
        let (sr, sg, sb, sa) = start.to_rgba();
        let (er, eg, eb, ea) = end.to_rgba();
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        return format!(
            "rgba({}, {}, {}, {})",
            mix(sr, er),
            mix(sg, eg),
            mix(sb, eb),
            mix(sa, ea) as f32 / 255.0
        );
    }

    // A unitless `0` is a zero length, so it blends with whatever the other
    // end is measured in rather than swapping over half way.
    let (from, to) = (zero_as(from, to), zero_as(to, from));
    let (from, to) = (from.as_str(), to.as_str());

    match (numeric_skeleton(from), numeric_skeleton(to)) {
        (Some((start_text, start_numbers)), Some((end_text, end_numbers)))
            if start_text == end_text && start_numbers.len() == end_numbers.len() =>
        {
            let blended: Vec<f32> = start_numbers
                .iter()
                .zip(&end_numbers)
                .map(|(a, b)| a + (b - a) * t)
                .collect();
            rebuild(from, &blended)
        }
        // Not the same shape: CSS calls this a discrete animation, and swaps
        // the value over at the middle of the interval.
        _ => {
            if t < 0.5 {
                from.trim().to_string()
            } else {
                to.trim().to_string()
            }
        }
    }
}

/// A bare `0` rewritten in `other`'s units, so the two can be blended.
///
/// Left alone unless `value` really is a lone zero and `other` is a single
/// number with something after it.
fn zero_as(value: &str, other: &str) -> String {
    let trimmed = value.trim();
    if trimmed != "0" {
        return trimmed.to_string();
    }
    match numeric_skeleton(other) {
        Some((text, numbers)) if numbers.len() == 1 && text != "#" => rebuild(other, &[0.0]),
        _ => trimmed.to_string(),
    }
}

/// Split a value into the numbers it contains and the text between them.
///
/// The text is what has to match for two values to be blendable: `10px 20px`
/// and `30px 40px` share the skeleton `#px #px`, while `10px` and `10%` do not.
fn numeric_skeleton(value: &str) -> Option<(String, Vec<f32>)> {
    let mut text = String::new();
    let mut numbers = Vec::new();
    let bytes: Vec<char> = value.trim().chars().collect();
    let mut index = 0;

    while index < bytes.len() {
        let ch = bytes[index];
        let starts_number = ch.is_ascii_digit()
            || ((ch == '-' || ch == '+' || ch == '.')
                && bytes
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_digit() || *next == '.'));
        if starts_number {
            let start = index;
            if bytes[index] == '-' || bytes[index] == '+' {
                index += 1;
            }
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == '.') {
                index += 1;
            }
            let literal: String = bytes[start..index].iter().collect();
            numbers.push(literal.parse::<f32>().ok()?);
            text.push('#');
            continue;
        }
        // Runs of whitespace all count as one separator, so `1px  2px` and
        // `3px 4px` still look alike.
        if ch.is_whitespace() {
            if !text.ends_with(' ') {
                text.push(' ');
            }
            index += 1;
            continue;
        }
        text.push(ch.to_ascii_lowercase());
        index += 1;
    }

    (!numbers.is_empty()).then_some((text, numbers))
}

/// Put a value back together with new numbers in place of its old ones.
fn rebuild(template: &str, numbers: &[f32]) -> String {
    let mut out = String::new();
    let bytes: Vec<char> = template.trim().chars().collect();
    let mut index = 0;
    let mut taken = 0;

    while index < bytes.len() {
        let ch = bytes[index];
        let starts_number = ch.is_ascii_digit()
            || ((ch == '-' || ch == '+' || ch == '.')
                && bytes
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_digit() || *next == '.'));
        if starts_number {
            if bytes[index] == '-' || bytes[index] == '+' {
                index += 1;
            }
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == '.') {
                index += 1;
            }
            let value = numbers.get(taken).copied().unwrap_or(0.0);
            taken += 1;
            out.push_str(&format_number(value));
            continue;
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// A number written the way CSS would, without a trailing `.0`.
fn format_number(value: f32) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}

/// One stop of an `@keyframes` rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    /// Where in the animation this stop sits, as a fraction. One stop can be
    /// written at several offsets (`0%, 100% { … }`), which becomes one entry
    /// per offset.
    pub offset: f32,
    pub declarations: Vec<Declaration>,
}

/// A parsed `@keyframes` rule.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframesRule {
    pub name: String,
    /// Stops in ascending offset order.
    pub keyframes: Vec<Keyframe>,
}

impl KeyframesRule {
    /// The declared value of `property` at `progress` through the animation.
    ///
    /// Only the stops that mention the property take part, so a property
    /// declared at `0%` and `100%` slides across a `50%` stop that says nothing
    /// about it — which is how `@keyframes` is specified to work.
    pub fn value_at(&self, property: &str, progress: f32) -> Option<String> {
        let stops: Vec<(f32, &str)> = self
            .keyframes
            .iter()
            .filter_map(|frame| {
                frame
                    .declarations
                    .iter()
                    .rev()
                    .find(|decl| decl.property.eq_ignore_ascii_case(property))
                    .map(|decl| (frame.offset, decl.value.as_str()))
            })
            .collect();

        let first = stops.first()?;
        if progress <= first.0 {
            return Some(first.1.to_string());
        }
        let last = stops.last()?;
        if progress >= last.0 {
            return Some(last.1.to_string());
        }

        for pair in stops.windows(2) {
            let (start_offset, start_value) = pair[0];
            let (end_offset, end_value) = pair[1];
            if progress >= start_offset && progress <= end_offset {
                let span = end_offset - start_offset;
                let local = if span <= 0.0 {
                    1.0
                } else {
                    (progress - start_offset) / span
                };
                return Some(interpolate_value(start_value, end_value, local));
            }
        }
        None
    }

    /// Every property any stop of this rule mentions.
    pub fn properties(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for frame in &self.keyframes {
            for decl in &frame.declarations {
                let name = decl.property.to_ascii_lowercase();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }
}

/// Parse the selector of one `@keyframes` stop into its offsets.
fn parse_keyframe_offsets(selector: &str) -> Vec<f32> {
    selector
        .split(',')
        .filter_map(|part| {
            let part = part.trim().to_ascii_lowercase();
            match part.as_str() {
                "from" => Some(0.0),
                "to" => Some(1.0),
                _ => part
                    .strip_suffix('%')
                    .and_then(|number| number.trim().parse::<f32>().ok())
                    .map(|percent| percent / 100.0),
            }
        })
        .collect()
}

/// Parse the body of an `@keyframes` block into its stops, in offset order.
pub fn parse_keyframes_block(name: &str, block: &str) -> Option<KeyframesRule> {
    let name = name.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if name.is_empty() {
        return None;
    }

    let mut keyframes = Vec::new();
    let mut rest = block;
    while let Some(open) = rest.find('{') {
        let selector = &rest[..open];
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let declarations = super::parse_declarations(&rest[open + 1..open + close]);
        for offset in parse_keyframe_offsets(selector) {
            keyframes.push(Keyframe {
                offset: offset.clamp(0.0, 1.0),
                declarations: declarations.clone(),
            });
        }
        rest = &rest[open + close + 1..];
    }

    if keyframes.is_empty() {
        return None;
    }
    keyframes.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    Some(KeyframesRule {
        name: name.to_string(),
        keyframes,
    })
}

/// The `transition-*` properties of one element.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transitions {
    /// One entry per transitioned property. `property` is lowercase, or `all`.
    pub entries: Vec<TransitionEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionEntry {
    pub property: String,
    pub duration: f32,
    pub delay: f32,
    pub easing: Easing,
}

impl Transitions {
    /// The transition covering `property`, if this element declares one.
    ///
    /// A named property wins over `all`, and a later entry over an earlier one,
    /// which is the order CSS resolves duplicates in.
    pub fn covering(&self, property: &str) -> Option<&TransitionEntry> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.property.eq_ignore_ascii_case(property))
            .or_else(|| {
                self.entries
                    .iter()
                    .rev()
                    .find(|entry| entry.property == "all")
            })
            .filter(|entry| entry.duration > 0.0)
    }

    /// Parse the `transition` shorthand, which lists one comma-separated group
    /// per property: `opacity .2s ease-in .1s, width 1s`.
    pub fn parse_shorthand(value: &str) -> Self {
        let mut entries = Vec::new();
        for group in split_top_level_commas(value) {
            let mut property = "all".to_string();
            let mut times = Vec::new();
            let mut easing = Easing::default();

            // `cubic-bezier(…)` and `steps(…)` hold spaces of their own, so
            // they come out before the rest is split into words — otherwise
            // half a bezier is mistaken for a property name.
            let (rest, function) = take_easing(&group);
            if let Some(parsed) = function {
                easing = parsed;
            }
            for token in rest.split_whitespace() {
                if let Some(seconds) = parse_time(token) {
                    times.push(seconds);
                } else if let Some(parsed) = Easing::parse(token) {
                    easing = parsed;
                } else if !token.eq_ignore_ascii_case("none") {
                    property = token.trim().to_ascii_lowercase();
                }
            }

            entries.push(TransitionEntry {
                property,
                duration: times.first().copied().unwrap_or(0.0),
                delay: times.get(1).copied().unwrap_or(0.0),
                easing,
            });
        }
        Self { entries }
    }
}

/// The `animation-*` properties of one element.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub delay: f32,
    pub easing: Easing,
    /// `None` is `infinite`.
    pub iterations: Option<f32>,
    pub direction: AnimationDirection,
    pub fill_mode: FillMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AnimationDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            name: String::new(),
            duration: 0.0,
            delay: 0.0,
            easing: Easing::default(),
            iterations: Some(1.0),
            direction: AnimationDirection::default(),
            fill_mode: FillMode::default(),
        }
    }
}

impl Animation {
    pub fn is_none(&self) -> bool {
        self.name.is_empty() || self.duration <= 0.0
    }

    /// How far through the animation `elapsed` seconds are, or `None` when the
    /// animation is not showing anything at that moment.
    ///
    /// Returning `None` before the delay has passed — unless a fill mode says
    /// otherwise — is what keeps an element at its cascaded style until its
    /// animation starts.
    pub fn progress_at(&self, elapsed: f32) -> Option<f32> {
        if self.is_none() {
            return None;
        }
        let fills_backwards = matches!(self.fill_mode, FillMode::Backwards | FillMode::Both);
        let fills_forwards = matches!(self.fill_mode, FillMode::Forwards | FillMode::Both);

        let active = elapsed - self.delay;
        if active < 0.0 {
            return fills_backwards.then(|| self.eased(self.iteration_progress(0.0, 0)));
        }

        let played = active / self.duration;
        let finished = self.iterations.is_some_and(|total| played >= total);
        let (iteration, within) = if finished {
            let total = self.iterations.unwrap_or(1.0);
            // The last frame of a finished animation is its final one, not the
            // first frame of the iteration after it.
            ((total.ceil() as u32).saturating_sub(1), 1.0)
        } else {
            (played.floor() as u32, played.fract())
        };
        if finished && !fills_forwards {
            return None;
        }
        Some(self.eased(self.iteration_progress(within, iteration)))
    }

    /// Where in the timeline one iteration sits, once `direction` has had its say.
    fn iteration_progress(&self, within: f32, iteration: u32) -> f32 {
        let odd = iteration % 2 == 1;
        match self.direction {
            AnimationDirection::Normal => within,
            AnimationDirection::Reverse => 1.0 - within,
            AnimationDirection::Alternate => {
                if odd {
                    1.0 - within
                } else {
                    within
                }
            }
            AnimationDirection::AlternateReverse => {
                if odd {
                    within
                } else {
                    1.0 - within
                }
            }
        }
    }

    fn eased(&self, progress: f32) -> f32 {
        self.easing.apply(progress)
    }

    /// Parse the `animation` shorthand: `slide 2s ease-in 0.5s infinite alternate both`.
    ///
    /// The first time is the duration and the second the delay, as CSS says;
    /// everything else is recognised by what it is.
    pub fn parse_shorthand(value: &str) -> Self {
        let mut animation = Animation {
            iterations: Some(1.0),
            ..Default::default()
        };
        let mut times = Vec::new();

        let first = split_top_level_commas(value)
            .first()
            .cloned()
            .unwrap_or_default();
        let (rest, function) = take_easing(&first);
        if let Some(easing) = function {
            animation.easing = easing;
        }

        for token in rest.split_whitespace() {
            let lower = token.to_ascii_lowercase();
            if let Some(seconds) = parse_time(token) {
                times.push(seconds);
            } else if let Some(easing) = Easing::parse(token) {
                animation.easing = easing;
            } else if lower == "infinite" {
                animation.iterations = None;
            } else if let Ok(count) = lower.parse::<f32>() {
                animation.iterations = Some(count.max(0.0));
            } else if let Some(direction) = parse_direction(&lower) {
                animation.direction = direction;
            } else if let Some(fill) = parse_fill_mode(&lower) {
                animation.fill_mode = fill;
            } else if lower != "none" && lower != "running" && lower != "paused" {
                animation.name = token.trim().to_string();
            }
        }
        animation.duration = times.first().copied().unwrap_or(0.0);
        animation.delay = times.get(1).copied().unwrap_or(0.0);
        animation
    }
}

pub fn parse_direction(value: &str) -> Option<AnimationDirection> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(AnimationDirection::Normal),
        "reverse" => Some(AnimationDirection::Reverse),
        "alternate" => Some(AnimationDirection::Alternate),
        "alternate-reverse" => Some(AnimationDirection::AlternateReverse),
        _ => None,
    }
}

pub fn parse_fill_mode(value: &str) -> Option<FillMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(FillMode::None),
        "forwards" => Some(FillMode::Forwards),
        "backwards" => Some(FillMode::Backwards),
        "both" => Some(FillMode::Both),
        _ => None,
    }
}

/// Lift the `cubic-bezier(…)` or `steps(…)` out of a longer value.
///
/// Returns what is left of the value and the timing function that was in it,
/// so the caller can split the remainder into words without a half-parsed
/// function among them.
fn take_easing(value: &str) -> (String, Option<Easing>) {
    let lower = value.to_ascii_lowercase();
    for name in ["cubic-bezier", "steps"] {
        let Some(start) = lower.find(name) else {
            continue;
        };
        let Some(end) = lower[start..].find(')') else {
            continue;
        };
        let end = start + end + 1;
        if let Some(easing) = Easing::parse(&lower[start..end]) {
            let mut rest = value[..start].to_string();
            rest.push(' ');
            rest.push_str(&value[end..]);
            return (rest, Some(easing));
        }
    }
    (value.to_string(), None)
}

/// `f(a, b)` → `a, b`, when `value` is a call to `name`.
fn strip_call(value: &str, name: &str) -> Option<String> {
    let rest = value.trim().strip_prefix(name)?.trim_start();
    let inner = rest.strip_prefix('(')?;
    let end = inner.rfind(')')?;
    Some(inner[..end].to_string())
}

/// Split on commas that are not inside parentheses.
pub fn split_top_level_commas(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in value.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// Apply an interpolated declaration to a set of computed values.
pub fn apply_animated(
    computed: super::ComputedValues,
    property: &str,
    value: &str,
    ctx: LengthContext,
) -> super::ComputedValues {
    let declaration = Declaration {
        property: property.to_string(),
        value: value.to_string(),
        important: false,
    };
    computed.from_declaration_with_ctx(&declaration, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_read_in_seconds_however_it_is_written() {
        assert_eq!(parse_time("2s"), Some(2.0));
        assert_eq!(parse_time("250ms"), Some(0.25));
        assert_eq!(parse_time("0"), Some(0.0));
        assert_eq!(parse_time("2"), None, "a bare number is not a time");
    }

    #[test]
    fn linear_easing_returns_what_it_is_given() {
        assert_eq!(Easing::Linear.apply(0.3), 0.3);
    }

    #[test]
    fn a_bezier_starts_and_ends_where_it_should() {
        let ease = Easing::parse("ease-in-out").unwrap();
        assert!((ease.apply(0.0) - 0.0).abs() < 1e-3);
        assert!((ease.apply(1.0) - 1.0).abs() < 1e-3);
        assert!(
            (ease.apply(0.5) - 0.5).abs() < 1e-2,
            "symmetric in the middle"
        );
    }

    #[test]
    fn ease_in_starts_slower_than_linear() {
        let ease_in = Easing::parse("ease-in").unwrap();
        assert!(ease_in.apply(0.25) < 0.25);
    }

    #[test]
    fn steps_jumps_rather_than_slides() {
        let steps = Easing::parse("steps(4)").unwrap();
        assert_eq!(steps.apply(0.1), 0.0);
        assert_eq!(steps.apply(0.3), 0.25);
        assert_eq!(steps.apply(0.99), 0.75);
    }

    #[test]
    fn lengths_slide_between_their_two_ends() {
        assert_eq!(interpolate_value("10px", "20px", 0.5), "15px");
        assert_eq!(interpolate_value("0", "100px", 0.25), "25px");
    }

    #[test]
    fn a_value_with_several_numbers_blends_each_of_them() {
        assert_eq!(interpolate_value("10px 0px", "20px 40px", 0.5), "15px 20px");
        assert_eq!(
            interpolate_value("translate(0px, 0px)", "translate(40px, 80px)", 0.25),
            "translate(10px, 20px)"
        );
    }

    #[test]
    fn colours_blend_channel_by_channel() {
        assert_eq!(
            interpolate_value("#000000", "#ffffff", 0.5),
            "rgba(128, 128, 128, 1)"
        );
        assert_eq!(interpolate_value("red", "red", 0.5), "rgba(255, 0, 0, 1)");
    }

    #[test]
    fn values_of_different_shapes_swap_over_half_way() {
        assert_eq!(interpolate_value("10px", "auto", 0.4), "10px");
        assert_eq!(interpolate_value("10px", "auto", 0.6), "auto");
        assert_eq!(interpolate_value("10px", "50%", 0.9), "50%");
    }

    #[test]
    fn the_ends_are_returned_untouched() {
        assert_eq!(interpolate_value("10px", "20px", 0.0), "10px");
        assert_eq!(interpolate_value("10px", "20px", 1.0), "20px");
    }

    fn slide() -> KeyframesRule {
        parse_keyframes_block(
            "slide",
            "from { left: 0px; opacity: 0 } 50% { left: 50px } to { left: 100px; opacity: 1 }",
        )
        .expect("the rule parses")
    }

    #[test]
    fn keyframes_are_kept_in_offset_order() {
        let offsets: Vec<f32> = slide().keyframes.iter().map(|f| f.offset).collect();
        assert_eq!(offsets, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn from_and_to_name_the_two_ends() {
        let rule = parse_keyframes_block("fade", "from { opacity: 0 } to { opacity: 1 }").unwrap();
        assert_eq!(rule.value_at("opacity", 0.0).as_deref(), Some("0"));
        assert_eq!(rule.value_at("opacity", 1.0).as_deref(), Some("1"));
        assert_eq!(rule.value_at("opacity", 0.5).as_deref(), Some("0.5"));
    }

    #[test]
    fn one_stop_can_be_written_at_several_offsets() {
        let rule = parse_keyframes_block("pulse", "0%, 100% { left: 0px } 50% { left: 20px }")
            .expect("the rule parses");
        assert_eq!(rule.keyframes.len(), 3);
        assert_eq!(rule.value_at("left", 0.0).as_deref(), Some("0px"));
        assert_eq!(rule.value_at("left", 1.0).as_deref(), Some("0px"));
    }

    #[test]
    fn a_stop_that_says_nothing_about_a_property_is_skipped_over() {
        // `opacity` is declared at 0% and 100% only, so it slides straight
        // across the 50% stop rather than stopping there.
        assert_eq!(slide().value_at("opacity", 0.5).as_deref(), Some("0.5"));
        // `left` is declared at all three, so it follows the middle stop.
        assert_eq!(slide().value_at("left", 0.25).as_deref(), Some("25px"));
        assert_eq!(slide().value_at("left", 0.75).as_deref(), Some("75px"));
    }

    #[test]
    fn a_property_no_stop_mentions_has_no_value() {
        assert_eq!(slide().value_at("color", 0.5), None);
    }

    #[test]
    fn a_rule_lists_the_properties_it_animates() {
        let mut properties = slide().properties();
        properties.sort();
        assert_eq!(properties, vec!["left", "opacity"]);
    }

    #[test]
    fn a_keyframes_block_with_no_stops_is_dropped() {
        assert!(parse_keyframes_block("empty", "  ").is_none());
        assert!(parse_keyframes_block("", "from { left: 0 }").is_none());
    }

    #[test]
    fn the_transition_shorthand_reads_duration_before_delay() {
        let transitions = Transitions::parse_shorthand("opacity 0.2s ease-in 0.1s");
        let entry = transitions
            .covering("opacity")
            .expect("opacity transitions");
        assert_eq!(entry.duration, 0.2);
        assert_eq!(entry.delay, 0.1);
        assert_eq!(entry.easing, Easing::parse("ease-in").unwrap());
    }

    #[test]
    fn one_shorthand_can_list_several_properties() {
        let transitions = Transitions::parse_shorthand("opacity .2s, width 1s linear");
        assert_eq!(transitions.entries.len(), 2);
        assert_eq!(transitions.covering("width").unwrap().duration, 1.0);
        assert_eq!(
            transitions.covering("width").unwrap().easing,
            Easing::Linear
        );
    }

    #[test]
    fn a_named_property_wins_over_all() {
        let transitions = Transitions::parse_shorthand("all 1s, color 3s");
        assert_eq!(transitions.covering("color").unwrap().duration, 3.0);
        assert_eq!(transitions.covering("width").unwrap().duration, 1.0);
    }

    #[test]
    fn a_transition_with_no_duration_does_nothing() {
        let transitions = Transitions::parse_shorthand("color");
        assert!(transitions.covering("color").is_none());
    }

    #[test]
    fn a_bezier_inside_a_transition_survives_its_commas() {
        let transitions = Transitions::parse_shorthand("width 1s cubic-bezier(0.1, 0.2, 0.3, 0.4)");
        assert_eq!(
            transitions.covering("width").unwrap().easing,
            Easing::CubicBezier(0.1, 0.2, 0.3, 0.4)
        );
    }

    #[test]
    fn the_animation_shorthand_reads_every_part_by_what_it_is() {
        let animation = Animation::parse_shorthand("slide 2s ease-in 0.5s infinite alternate both");
        assert_eq!(animation.name, "slide");
        assert_eq!(animation.duration, 2.0);
        assert_eq!(animation.delay, 0.5);
        assert_eq!(animation.iterations, None);
        assert_eq!(animation.direction, AnimationDirection::Alternate);
        assert_eq!(animation.fill_mode, FillMode::Both);
    }

    #[test]
    fn an_animation_with_no_duration_is_not_running() {
        assert!(Animation::parse_shorthand("slide").is_none());
        assert!(Animation::parse_shorthand("2s").is_none());
    }

    fn running(shorthand: &str) -> Animation {
        Animation::parse_shorthand(shorthand)
    }

    #[test]
    fn nothing_shows_before_the_delay_has_passed() {
        assert_eq!(running("slide 2s linear 1s").progress_at(0.5), None);
        assert_eq!(running("slide 2s linear 1s").progress_at(2.0), Some(0.5));
    }

    #[test]
    fn backwards_fill_holds_the_first_frame_during_the_delay() {
        let animation = running("slide 2s linear 1s backwards");
        assert_eq!(animation.progress_at(0.5), Some(0.0));
    }

    #[test]
    fn an_animation_stops_showing_once_it_has_run_its_course() {
        assert_eq!(running("slide 2s linear").progress_at(3.0), None);
        assert_eq!(
            running("slide 2s linear forwards").progress_at(3.0),
            Some(1.0),
            "unless it is asked to hold its last frame"
        );
    }

    #[test]
    fn an_infinite_animation_keeps_going_round() {
        let animation = running("slide 2s linear infinite");
        assert_eq!(animation.progress_at(1.0), Some(0.5));
        assert_eq!(animation.progress_at(5.0), Some(0.5));
    }

    #[test]
    fn alternate_runs_every_other_iteration_backwards() {
        let animation = running("slide 2s linear infinite alternate");
        assert_eq!(animation.progress_at(0.5), Some(0.25));
        assert_eq!(
            animation.progress_at(2.5),
            Some(0.75),
            "the second lap reverses"
        );
    }

    #[test]
    fn reverse_runs_the_timeline_the_other_way() {
        let animation = running("slide 2s linear infinite reverse");
        assert_eq!(animation.progress_at(0.5), Some(0.75));
    }
}
